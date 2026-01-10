use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::prelude::*;
use ed25519_dalek::SigningKey;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Serialize, Deserialize)]
pub struct KeyFile {
    pub pub_key: String,
    pub enc_priv_key: String,
    pub salt: String,
    pub nonce: String,
}

/// 从加密的密钥文件和密码解密私钥
pub fn decrypt_private_key(key_file: &KeyFile, passphrase: &str) -> Result<SigningKey, String> {
    // 解码 base64
    let salt = BASE64_STANDARD
        .decode(&key_file.salt)
        .map_err(|e| format!("Invalid salt format: {}", e))?;

    let nonce_bytes = BASE64_STANDARD
        .decode(&key_file.nonce)
        .map_err(|e| format!("Invalid nonce format: {}", e))?;

    let enc_priv_key = BASE64_STANDARD
        .decode(&key_file.enc_priv_key)
        .map_err(|e| format!("Invalid encrypted private key format: {}", e))?;

    // 从密码派生密钥
    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, 100_000, &mut key)
        .map_err(|e| format!("Key derivation failed: {}", e))?;

    // 解密
    let cipher = Aes256Gcm::new(&key.into());
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, enc_priv_key.as_ref())
        .map_err(|_| "Decryption failed. Wrong password?".to_string())?;

    // 构造 SigningKey
    let priv_key_bytes: [u8; 32] = plaintext
        .try_into()
        .map_err(|_| "Invalid private key length".to_string())?;

    Ok(SigningKey::from_bytes(&priv_key_bytes))
}

/// 初始化 tracing 日志系统
///
/// # 参数
/// - `dir`: 可选的文件日志目录
/// - `file`: 可选的文件日志文件名
///
/// # 返回值
/// 返回 `Some(WorkerGuard)` 如果启用了文件日志，否则返回 `None`。
/// 返回的 guard 必须保留以便后续同步缓冲区到文件。
#[must_use]
pub fn tracing_init(dir: Option<&str>, file: Option<&str>) -> Option<WorkerGuard> {
    // 创建标准输出日志层
    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_file(true)
        .with_line_number(true)
        .with_writer(std::io::stdout)
        .with_thread_ids(true)
        .with_target(false);

    // 创建环境过滤器，默认级别为 info
    let env_filter =
        tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into());

    if let (Some(dir), Some(file)) = (dir, file) {
        let file_appender = tracing_appender::rolling::never(dir, file);
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let file_layer = tracing_subscriber::fmt::layer()
            .with_file(true)
            .with_line_number(true)
            .with_ansi(false)
            .with_thread_ids(true)
            .with_target(false)
            .with_writer(non_blocking);

        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .with(env_filter)
            .init();

        Some(guard)
    } else {
        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(env_filter)
            .init();

        None
    }
}
