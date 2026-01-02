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
        user_name: Option<String>,
        user_password: Option<String>,
    },
    AddService {
        description: String,
        exe_path: String,
        args: Vec<String>,
        run_as_user: bool,
        user_name: Option<String>,
        user_password: Option<String>,
    },
    QueryPath {
        path: String,
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
    PathSuggestions(Vec<PathItem>),
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SystemInfo {
    pub timestamp: u64,
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
    pub id: usize,
    pub description: String,
    pub running: bool,
    pub pid: Option<u32>,
    pub run_as_user: bool,
}

impl PartialEq for SystemInfo {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl PartialEq for ServiceInfo {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.pid == other.pid
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PathItem {
    pub path: String,
    pub is_dir: bool,
    pub is_executable: bool,
}
