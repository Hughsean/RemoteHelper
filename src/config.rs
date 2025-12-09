use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone)]
pub struct SmtpConfig {
    pub enabled: bool,
    pub server: String,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: String,
}

#[derive(Deserialize, Clone)]
pub struct WebControlConfig {
    pub enabled: bool,
    pub local_port: u16,
    pub frpc_arg: String,
    pub auth_secret: String,
}

#[derive(Deserialize, Clone)]
pub struct FrpcConfig {
    pub description: String,
    pub arg: String,
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub smtp: SmtpConfig,
    pub web_control: Option<WebControlConfig>,
    pub frpc: Vec<FrpcConfig>,
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string("config.toml")
        .map_err(|e| format!("Failed to read config.toml: {}", e))?;
    let config: Config =
        toml::from_str(&content).map_err(|e| format!("Failed to parse config.toml: {}", e))?;
    Ok(config)
}
