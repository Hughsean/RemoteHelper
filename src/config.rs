use anyhow::Context;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub web_panel: WebPanelConfig,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct WebPanelConfig {
    pub enabled: bool,
    pub local_port: u16,
    pub frpc_exe_path: String,
    #[serde(default)]
    pub frpc_args: Vec<String>,
    #[serde(default)]
    pub authorized_keys: Vec<String>,
    #[serde(default = "default_startup_delay")]
    pub startup_delay_secs: u64,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_secs: u64,
    #[serde(default)]
    pub health_check_url: Option<String>,
    // #[serde(default = "default_cert_path")]
    // pub cert_path: String,
    // #[serde(default = "default_key_path")]
    // pub key_path: String,
}

fn default_startup_delay() -> u64 {
    0 // No delay by default
}

fn default_max_connections() -> usize {
    100 // Reasonable default
}

fn default_connection_timeout() -> u64 {
    300 // 5 minutes
}

// fn default_cert_path() -> String {
//     "cert.pem".to_string()
// }

// fn default_key_path() -> String {
//     "key.pem".to_string()
// }

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub description: String,
    pub exe_path: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub allow_web_control: bool,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Path::new("config.toml");
        let content = std::fs::read_to_string(config_path).context("Failed to read config.toml")?;
        let config: AppConfig = toml::from_str(&content).context("Failed to parse config.toml")?;
        Ok(config)
    }
}
