use anyhow::Context;
use serde::{Deserialize, Serialize};
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

use std::fmt;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum AutoStart {
    #[default]
    None,
    OneShot,
    Continuous,
}

impl fmt::Display for AutoStart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AutoStart::None => write!(f, "None"),
            AutoStart::OneShot => write!(f, "OneShot"),
            AutoStart::Continuous => write!(f, "Continuous"),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServiceConfig {
    pub description: String,
    pub exe_path: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub auto_start: AutoStart,
    #[serde(default)]
    pub allow_web_control: bool,
}

impl AppConfig {
    pub fn load() -> anyhow::Result<Self> {
        let config_path = Path::new("config.toml");
        let content = std::fs::read_to_string(config_path).context("无法读取 config.toml")?;
        let config: AppConfig = toml::from_str(&content).context("无法解析 config.toml")?;
        Ok(config)
    }
}
