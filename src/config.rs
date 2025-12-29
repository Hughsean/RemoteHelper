use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Clone)]
#[deprecated(since = "0.2.0", note = "Email functionality is deprecated")]
pub struct SmtpConfig {
    pub enabled: bool,
    pub server: String,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: String,
}

#[derive(Deserialize, Clone)]
pub struct WebPanelConfig {
    pub enabled: bool,
    pub local_port: u16,
    pub frpc_arg: String,
    pub auth_secret: String,
    #[serde(default)]
    pub frpc_exe_path: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct ServiceConfig {
    pub description: String,
    #[serde(default)]
    pub exe_path: Option<String>,
    #[serde(default)]
    pub args: Option<Vec<String>>, // 优先使用新的 args 数组
    #[serde(default)]
    pub arg: Option<String>, // 兼容旧版单个 frpc 参数
    pub auto_start: Option<bool>,
    #[serde(default)]
    pub working_dir: Option<String>,
    #[serde(default)]
    pub allow_web_control: Option<bool>,
}

#[derive(Deserialize, Clone)]
#[allow(deprecated)]
pub struct Config {
    // #[deprecated(since = "0.2.0", note = "Email functionality is deprecated")]
    // pub smtp: SmtpConfig,
    #[serde(rename = "web_panel", alias = "web_control")]
    pub web_panel: Option<WebPanelConfig>,
    #[serde(rename = "service", alias = "frpc")]
    pub service: Vec<ServiceConfig>,
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let content = fs::read_to_string("config.toml")
        .map_err(|e| format!("Failed to read config.toml: {}", e))?;
    let mut config: Config =
        toml::from_str(&content).map_err(|e| format!("Failed to parse config.toml: {}", e))?;

    // 默认值兼容：auto_start, exe_path, args (含旧版 arg 映射)
    config.service.iter_mut().for_each(|svc| {
        if svc.auto_start.is_none() {
            svc.auto_start = Some(false);
        }

        if svc.exe_path.is_none() {
            svc.exe_path = Some("frpc.exe".to_string());
        }

        if svc.args.is_none() {
            if let Some(arg) = &svc.arg {
                svc.args = Some(vec!["-f".to_string(), arg.clone()]);
            } else {
                svc.args = Some(Vec::new());
            }
        }

        if svc.allow_web_control.is_none() {
            svc.allow_web_control = Some(true);
        }
    });

    Ok(config)
}
