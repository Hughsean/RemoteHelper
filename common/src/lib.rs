pub mod crypto;

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Handshake {
    ClientHello { public_key: String },
    ServerHello { public_key: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Request {
    GetChallenge,
    Login {
        public_key: String,
        signature: String,
    },
    GetStatus {
        interval_ms: Option<u64>,
    },
    ListServices,
    ControlService {
        id: usize,
        action: ServiceAction,
    },
    AddService {
        description: String,
        exe_path: String,
        args: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
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
    ServiceAdded(usize),
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
    pub cpu_model: String,
    pub gpu_model: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServiceData {
    pub id: usize,
    pub description: String,
    pub running: bool,
    pub pid: Option<u32>,
}
