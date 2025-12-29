use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::process::Child;
use sysinfo::System;
use nvml_wrapper::Nvml;
use crate::config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub sys: Arc<Mutex<System>>,
    pub nvml: Arc<Mutex<Option<Nvml>>>,
    pub service_processes: Arc<Mutex<HashMap<usize, Child>>>,
    pub web_tunnel_process: Arc<Mutex<Option<Child>>>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(config),
            sys: Arc::new(Mutex::new(System::new_all())),
            nvml: Arc::new(Mutex::new(Nvml::init().ok())),
            service_processes: Arc::new(Mutex::new(HashMap::new())),
            web_tunnel_process: Arc::new(Mutex::new(None)),
        }
    }
}
