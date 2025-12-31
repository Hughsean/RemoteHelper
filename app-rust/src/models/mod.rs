use serde::{Deserialize, Serialize};

pub use common::{ServiceInfo, SystemInfo};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomServiceConfig {
    pub description: String,
    pub exe_path: String,
    pub args: String,
}

pub fn default_system_status() -> SystemInfo {
    SystemInfo {
        // uuid: uuid::Uuid::nil(),
        nanoid: String::new(),
        cpu_usage: 0.0,
        memory_usage: 0,
        total_memory: 0,
        uptime: 0,
        gpu_usage: None,
        gpu_memory_usage: None,
        gpu_total_memory: None,
        cpu_model: String::new(),
        gpu_model: None,
    }
}
