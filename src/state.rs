use crate::config::{AppConfig, ServiceConfig};
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
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
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub dynamic_services: Arc<RwLock<Vec<ServiceConfig>>>,
    pub sys: Arc<RwLock<System>>,
    pub networks: Arc<RwLock<Networks>>,
    pub nvml: Arc<RwLock<Option<Nvml>>>,
    pub gpu_cache: Arc<RwLock<GpuCache>>,
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
            refresh_interval: Arc::new(RwLock::new(1000)), // 默认 1 秒
            last_read_time: Arc::new(RwLock::new(std::time::Instant::now())),
            update_notify: Arc::new(Notify::new()),
            service_processes: Arc::new(RwLock::new(HashMap::new())),
            web_tunnel_process: Arc::new(Mutex::new(None)),
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }
}
