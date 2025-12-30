use crate::config::{AppConfig, ServiceConfig};
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;
use tokio::process::Child;
use tokio::sync::{Mutex, Notify, RwLock};

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
    pub nvml: Arc<RwLock<Option<Nvml>>>,
    pub gpu_cache: Arc<RwLock<GpuCache>>,
    pub refresh_interval: Arc<RwLock<u64>>,
    pub update_notify: Arc<Notify>,
    pub service_processes: Arc<RwLock<HashMap<usize, Child>>>,
    pub web_tunnel_process: Arc<Mutex<Option<Child>>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(config),
            dynamic_services: Arc::new(RwLock::new(Vec::new())),
            sys: Arc::new(RwLock::new(System::new_all())),
            nvml: Arc::new(RwLock::new(Nvml::init().ok())),
            gpu_cache: Arc::new(RwLock::new(GpuCache::default())),
            refresh_interval: Arc::new(RwLock::new(1000)), // Default 1s
            update_notify: Arc::new(Notify::new()),
            service_processes: Arc::new(RwLock::new(HashMap::new())),
            web_tunnel_process: Arc::new(Mutex::new(None)),
        }
    }
}
