use crate::config::AppConfig;
use nvml_wrapper::Nvml;
use std::collections::HashMap;
use std::sync::Arc;
use sysinfo::System;
use tokio::process::Child;
use tokio::sync::{Mutex, RwLock};

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub sys: Arc<RwLock<System>>,
    pub nvml: Arc<RwLock<Option<Nvml>>>,
    pub service_processes: Arc<RwLock<HashMap<usize, Child>>>,
    pub web_tunnel_process: Arc<Mutex<Option<Child>>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(config),
            sys: Arc::new(RwLock::new(System::new_all())),
            nvml: Arc::new(RwLock::new(Nvml::init().ok())),
            service_processes: Arc::new(RwLock::new(HashMap::new())),
            web_tunnel_process: Arc::new(Mutex::new(None)),
        }
    }
}
