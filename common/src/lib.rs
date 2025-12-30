use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    GetChallenge,
    Login {
        public_key: String,
        signature: String,
    },
    GetStatus,
    ListServices,
    ControlService {
        id: usize,
        action: ServiceAction,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum ServiceAction {
    Start,
    Stop,
    Restart,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
    Ok,
    Challenge(String),
    Error(String),
    Status(StatusData),
    Services(Vec<ServiceData>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusData {
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub total_memory: u64,
    pub uptime: u64,
    pub gpu_usage: Option<u32>,
    pub gpu_memory_usage: Option<u64>,
    pub gpu_total_memory: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceData {
    pub id: usize,
    pub description: String,
    pub running: bool,
    pub pid: Option<u32>,
}
