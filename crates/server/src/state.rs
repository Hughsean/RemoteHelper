use crate::config::{AppConfig, ServiceConfig};
use crate::history::RecorderHandle;
use crate::monitor::Monitor;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::process::Child;
use tokio::sync::{Mutex, RwLock};

/// 服务进程信息（系统模式）
///
/// 目前项目以系统服务模式管理进程，持有 `tokio::process::Child`。使用结构体而不是枚举
/// 便于记录元数据（例如启动时间）并提供统一的操作方法。
pub struct ServiceProcess {
    pub child: Child,
}

impl ServiceProcess {
    pub fn new(child: Child) -> Self {
        Self { child }
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }

    pub fn try_wait(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
        self.child.try_wait()
    }

    pub async fn kill(&mut self) -> anyhow::Result<()> {
        self.child
            .kill()
            .await
            .map_err(|e| anyhow::anyhow!("kill failed: {}", e))
    }

    pub fn start_kill(&mut self) -> std::io::Result<()> {
        self.child.start_kill()
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub dynamic_services: Arc<RwLock<Vec<ServiceConfig>>>,
    pub monitor: Monitor,
    pub history: Option<RecorderHandle>,
    pub identity: Option<Arc<crate::identity::ServerIdentity>>,
    pub(crate) history_settings: Arc<Mutex<crate::handlers::settings::RuntimeSettings>>,
    pub service_processes: Arc<RwLock<HashMap<usize, ServiceProcess>>>,
    pub web_tunnel_process: Arc<Mutex<Option<Child>>>,
    pub active_connections: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new(config: AppConfig, history: Option<RecorderHandle>, config_bytes: &[u8]) -> Self {
        Self {
            history_settings: Arc::new(Mutex::new(
                crate::handlers::settings::RuntimeSettings::new(
                    config.history.clone(),
                    config_bytes,
                ),
            )),
            monitor: Monitor::new(&config.history),
            history,
            identity: None,
            config: Arc::new(config),
            dynamic_services: Arc::new(RwLock::new(Vec::new())),
            service_processes: Arc::new(RwLock::new(HashMap::new())),
            web_tunnel_process: Arc::new(Mutex::new(None)),
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[cfg(test)]
mod tests {
    use nvml_wrapper::Nvml;
    use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
    use nvml_wrapper::error::NvmlError;

    #[test]
    fn test_gpu_temperature_and_power() -> anyhow::Result<()> {
        match Nvml::init() {
            Ok(nvml) => {
                let device = match nvml.device_by_index(0) {
                    Ok(d) => d,
                    Err(e) => {
                        eprintln!("No NVML device found: {}", e);
                        // Skip test if no device
                        return Ok(());
                    }
                };

                // Temperature (°C)
                match device.temperature(TemperatureSensor::Gpu) {
                    Ok(temp) => {
                        // Basic sanity check
                        assert!(temp <= 200, "unreasonable GPU temperature: {} °C", temp);
                        println!("GPU temperature: {} °C", temp);
                    }
                    Err(NvmlError::NotSupported) => {
                        eprintln!("Temperature sensor not supported on this device");
                        return Ok(());
                    }
                    Err(e) => return Err(anyhow::anyhow!("Failed to read GPU temperature: {}", e)),
                }

                // Power usage (mW)
                match device.power_usage() {
                    Ok(power_mw) => {
                        // Allow 0 (idle) up to a sensible upper bound
                        assert!(
                            power_mw <= 1_000_000,
                            "unreasonable GPU power: {} mW",
                            power_mw
                        );
                        println!("GPU power usage: {} mW", power_mw);
                    }
                    Err(NvmlError::NotSupported) => {
                        eprintln!("Power usage not supported on this device");
                        return Ok(());
                    }
                    Err(e) => return Err(anyhow::anyhow!("Failed to read GPU power usage: {}", e)),
                }

                Ok(())
            }
            Err(e) => {
                eprintln!("NVML initialization failed: {}", e);
                // Skip test when NVML cannot be initialized
                Ok(())
            }
        }
    }
}
