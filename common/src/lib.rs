pub mod crypto;
pub mod func;
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
    Status(SystemInfo),
    Services(Vec<ServiceInfo>),
    ServiceAdded(usize),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SystemInfo {
    // #[serde(default)]
    pub nanoid: String,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub total_memory: u64,
    pub uptime: u64,
    pub gpu_usage: Option<u32>,
    pub gpu_memory_usage: Option<u64>,
    pub gpu_total_memory: Option<u64>,
    pub cpu_model: String,
    pub gpu_model: Option<String>,
    #[serde(default)]
    pub network_tx_bytes: u64,
    #[serde(default)]
    pub network_rx_bytes: u64,
    #[serde(default)]
    pub network_tx_speed: u64,
    #[serde(default)]
    pub network_rx_speed: u64,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ServiceInfo {
    #[serde(default)]
    pub nanoid: String,
    pub id: usize,
    pub description: String,
    pub running: bool,
    pub pid: Option<u32>,
}

impl PartialEq for SystemInfo {
    fn eq(&self, other: &Self) -> bool {
        self.nanoid == other.nanoid
    }
}

impl PartialEq for ServiceInfo {
    fn eq(&self, other: &Self) -> bool {
        self.nanoid == other.nanoid
    }
}
