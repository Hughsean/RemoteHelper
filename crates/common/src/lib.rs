pub mod crypto;
pub mod history_api;
pub mod keys;
pub mod logging;
pub mod server_auth;
pub mod transport;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Handshake {
    ClientHello {
        public_key: String,
        #[serde(
            default,
            skip_serializing_if = "Vec::is_empty",
            deserialize_with = "server_auth::decode_versions"
        )]
        history_versions: Vec<u16>,
    },
    ServerHello {
        public_key: String,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "server_auth::decode_proof"
        )]
        history: Option<server_auth::ServerProof>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Request {
    GetChallenge,
    Login {
        public_key: String,
        signature: String,
    },
    LoginHistory {
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
    QueryPath {
        path: String,
    },
    History(history_api::HistoryRequest),
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
    History(history_api::HistoryResponse),
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampleIdentity {
    pub session_id: [u8; 16],
    pub sample_seq: u64,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct SystemInfo {
    pub timestamp: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_identity: Option<SampleIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_ms: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_tx_speed_valid: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_rx_speed_valid: Option<bool>,
    pub cpu_usage: f32,
    pub memory_usage: u64,
    pub total_memory: u64,
    pub uptime: u64,
    pub gpu_usage: Option<u32>,
    pub gpu_memory_usage: Option<u64>,
    pub gpu_total_memory: Option<u64>,
    pub cpu_model: String,
    pub gpu_model: Option<String>,
    /// GPU 温度（摄氏度），如不可用则为 None
    #[serde(default)]
    pub gpu_temperature: Option<f32>,
    /// GPU 功率（瓦特），如不可用则为 None
    #[serde(default)]
    pub gpu_power_watts: Option<f32>,
    /// CPU 包温（摄氏度），如不可用则为 None
    #[serde(default)]
    pub cpu_temperature: Option<f32>,
    /// CPU 封装功率（瓦特），如不可用则为 None
    #[serde(default)]
    pub cpu_package_power: Option<f32>,
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
}

impl PartialEq for SystemInfo {
    fn eq(&self, other: &Self) -> bool {
        match (self.sample_identity, other.sample_identity) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.timestamp == other.timestamp,
            _ => false,
        }
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
