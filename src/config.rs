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
    pub frpc_arg: String,
    #[serde(default)]
    pub authorized_keys: Vec<String>,
}

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
