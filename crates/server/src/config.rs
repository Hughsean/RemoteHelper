use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const CONFIG_PATH: &str = "config.toml";

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub web_panel: WebPanelConfig,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub service: Vec<ServiceConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct HistoryConfig {
    pub enabled: bool,
    pub dir: std::path::PathBuf,
    pub admin_keys: Vec<String>,
    pub idle_interval_ms: u64,
    pub min_interval_ms: u64,
    pub seal_interval_secs: u64,
    pub retention_days: u32,
    pub retention_bytes: u64,
    pub compression_level: i32,
    pub revision: u64,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            dir: "history".into(),
            admin_keys: Vec::new(),
            idle_interval_ms: 5_000,
            min_interval_ms: 250,
            seal_interval_secs: 300,
            retention_days: 30,
            retention_bytes: 512 * 1024 * 1024,
            compression_level: 3,
            revision: 0,
        }
    }
}

impl HistoryConfig {
    pub fn validate(&self) -> anyhow::Result<()> {
        use anyhow::ensure;
        use base64::prelude::*;

        ensure!(!self.dir.as_os_str().is_empty(), "history.dir 不能为空");
        ensure!(
            (250..=60_000).contains(&self.idle_interval_ms),
            "history.idle_interval_ms 超出范围"
        );
        ensure!(
            (100..=self.idle_interval_ms).contains(&self.min_interval_ms),
            "history.min_interval_ms 超出范围"
        );
        ensure!(
            (60..=3_600).contains(&self.seal_interval_secs),
            "history.seal_interval_secs 超出范围"
        );
        ensure!(
            self.retention_days <= 3_650,
            "history.retention_days 超出范围"
        );
        ensure!(
            self.retention_bytes == 0
                || (16 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&self.retention_bytes),
            "history.retention_bytes 超出范围"
        );
        ensure!(
            self.retention_days != 0 || self.retention_bytes != 0,
            "至少启用一种历史保留限制"
        );
        ensure!(
            (-7..=22).contains(&self.compression_level),
            "history.compression_level 超出范围"
        );
        for key in &self.admin_keys {
            let bytes: [u8; 32] = BASE64_STANDARD
                .decode(key)?
                .try_into()
                .map_err(|_| anyhow::anyhow!("历史管理员公钥必须为 32 字节"))?;
            ed25519_dalek::VerifyingKey::from_bytes(&bytes).context("无效的历史管理员公钥")?;
        }
        Ok(())
    }
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
    pub fn load() -> anyhow::Result<(Self, Vec<u8>)> {
        let config_path = Path::new(CONFIG_PATH);
        let content = read_config(config_path).context("无法读取 config.toml")?;
        let mut config: AppConfig =
            toml::from_str(std::str::from_utf8(&content)?).context("无法解析 config.toml")?;
        config.history.validate().context("无效的历史配置")?;
        let config_dir = std::path::absolute(config_path)?
            .parent()
            .context("配置没有父目录")?
            .to_owned();
        anyhow::ensure!(
            !config
                .history
                .dir
                .components()
                .any(|part| part == std::path::Component::ParentDir),
            "history.dir 不得含有父目录跳转"
        );
        config.history.dir = config_dir.join(&config.history.dir);
        validate_history_location(&config.history.dir, &config_dir.join("identity"))?;
        Ok((config, content))
    }
}

fn validate_history_location(history: &Path, identity: &Path) -> anyhow::Result<()> {
    fn resolved(path: &Path) -> std::io::Result<std::path::PathBuf> {
        let mut candidate = path;
        let mut suffix = Vec::new();
        loop {
            match candidate.canonicalize() {
                Ok(mut base) => {
                    for part in suffix.into_iter().rev() {
                        base.push(part);
                    }
                    return Ok(base);
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    let (Some(parent), Some(name)) = (candidate.parent(), candidate.file_name())
                    else {
                        return Err(err);
                    };
                    suffix.push(name);
                    candidate = parent;
                }
                Err(err) => return Err(err),
            }
        }
    }
    fn contains(parent: &Path, child: &Path) -> bool {
        #[cfg(windows)]
        {
            let mut child = child.components();
            parent.components().all(|part| {
                child.next().is_some_and(|next| {
                    part.as_os_str()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&next.as_os_str().to_string_lossy())
                })
            })
        }
        #[cfg(not(windows))]
        {
            child.starts_with(parent)
        }
    }
    let overlaps = |a: &Path, b: &Path| contains(a, b) || contains(b, a);
    let lexical = overlaps(history, identity);
    // 无法解析的只读或不可访问路径由记录器报告，不阻断实时监控。
    let physical = resolved(history)
        .ok()
        .zip(resolved(identity).ok())
        .is_some_and(|(history, identity)| overlaps(&history, &identity));
    anyhow::ensure!(!lexical && !physical, "历史目录必须与服务端身份目录分离");
    Ok(())
}

pub fn read_config(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    const MAX_CONFIG_BYTES: u64 = 4 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        return Err(std::io::Error::other("配置文件超出大小限制"));
    }
    Ok(bytes)
}
