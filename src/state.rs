use crate::config::{AppConfig, ServiceConfig};
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use sysinfo::{Networks, System};
use tokio::process::Child;
use tokio::sync::{Mutex, Notify, RwLock};

/// 服务进程信息（系统模式）
///
/// 目前项目以系统服务模式管理进程，持有 `tokio::process::Child`。使用结构体而不是枚举
/// 便于记录元数据（例如启动时间）并提供统一的操作方法。
pub struct ServiceProcess {
    pub child: Child,
    #[allow(dead_code)]
    pub started_at: std::time::SystemTime,
}

impl ServiceProcess {
    pub fn new(child: Child) -> Self {
        Self {
            child,
            started_at: std::time::SystemTime::now(),
        }
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

#[derive(Clone, Default, Debug)]
pub struct GpuCache {
    pub usage: Option<u32>,
    pub memory_used: Option<u64>,
    pub memory_total: Option<u64>,
    pub model: Option<String>,
    /// GPU 温度（摄氏度），从 NVML 读取
    pub temperature: Option<f32>,
    /// GPU 功率（瓦特），从 NVML 读取并转换为 W
    pub power_watts: Option<f32>,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub dynamic_services: Arc<RwLock<Vec<ServiceConfig>>>,
    pub sys: Arc<RwLock<System>>,
    pub networks: Arc<RwLock<Networks>>,
    pub nvml: Arc<RwLock<Option<Nvml>>>,
    pub gpu_cache: Arc<RwLock<GpuCache>>,
    pub cpu_temp_cache: Arc<AtomicU32>,
    pub cpu_power_cache: Arc<AtomicU32>,
    pub refresh_interval: Arc<RwLock<u64>>,
    pub last_read_time: Arc<RwLock<std::time::Instant>>,
    pub update_notify: Arc<Notify>,
    pub service_processes: Arc<RwLock<HashMap<usize, ServiceProcess>>>,
    pub web_tunnel_process: Arc<Mutex<Option<Child>>>,
    pub active_connections: Arc<AtomicUsize>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(config),
            dynamic_services: Arc::new(RwLock::new(Vec::new())),
            sys: Arc::new(RwLock::new(System::new_all())),
            networks: Arc::new(RwLock::new(Networks::new_with_refreshed_list())),
            nvml: Arc::new(RwLock::new(Nvml::init().ok())),
            gpu_cache: Arc::new(RwLock::new(GpuCache::default())),
            cpu_temp_cache: Arc::new(AtomicU32::new(u32::MAX)),
            cpu_power_cache: Arc::new(AtomicU32::new(u32::MAX)),
            refresh_interval: Arc::new(RwLock::new(1000)), // 默认 1 秒
            last_read_time: Arc::new(RwLock::new(std::time::Instant::now())),
            update_notify: Arc::new(Notify::new()),
            service_processes: Arc::new(RwLock::new(HashMap::new())),
            web_tunnel_process: Arc::new(Mutex::new(None)),
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }

    // Atomics helpers for CPU metric caches (no external dependencies)
    fn store_opt_f32_atomic(a: &Arc<AtomicU32>, val: Option<f32>) {
        let bits = val.map_or(u32::MAX, |v| v.to_bits());
        a.store(bits, Ordering::Relaxed);
    }

    fn load_opt_f32_atomic(a: &Arc<AtomicU32>) -> Option<f32> {
        let bits = a.load(Ordering::Relaxed);
        if bits == u32::MAX {
            None
        } else {
            Some(f32::from_bits(bits))
        }
    }

    pub fn set_cpu_temp(&self, val: Option<f32>) {
        Self::store_opt_f32_atomic(&self.cpu_temp_cache, val);
    }

    pub fn get_cpu_temp(&self) -> Option<f32> {
        Self::load_opt_f32_atomic(&self.cpu_temp_cache)
    }

    pub fn set_cpu_power(&self, val: Option<f32>) {
        Self::store_opt_f32_atomic(&self.cpu_power_cache, val);
    }

    pub fn get_cpu_power(&self) -> Option<f32> {
        Self::load_opt_f32_atomic(&self.cpu_power_cache)
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
