use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Request {
    Auth { token: String },
    GetStatus,
    ListServices,
    ControlService { id: usize, action: ServiceAction },
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceData {
    pub id: usize,
    pub description: String,
    pub running: bool,
    pub pid: Option<u32>,
}
