//! 后端通信模块
//!
//! 封装与 RemoteHelper 服务器的所有通信逻辑
//!
//! 使用 client 库的全局连接管理

mod api;

pub use api::*;

/// 初始化连接到服务器
pub async fn connect(addr: String, password: String) -> Result<(), String> {
    // 设置服务器地址
    {
        let mut server_addr = client::SERVER_ADDRESS
            .lock()
            .map_err(|e| format!("Failed to acquire server address lock: {}", e))?;
        *server_addr = addr;
    }

    // 加载签名密钥
    load_signing_key(&password)?;

    // 建立连接和认证（会自动存储到全局 CONNECTION）
    client::connect_and_auth().await?;

    Ok(())
}

/// 加载签名密钥
fn load_signing_key(password: &str) -> Result<(), String> {
    use std::fs;
    use std::path::PathBuf;

    let key_path: PathBuf = dirs::home_dir()
        .ok_or("Failed to get home directory")?
        .join("id_ed25519.json");

    let key_content =
        fs::read_to_string(&key_path).map_err(|e| format!("Failed to read key file: {}", e))?;

    let key_file: client::KeyFile = serde_json::from_str(&key_content)
        .map_err(|e| format!("Failed to parse key file: {}", e))?;

    // 解密私钥
    use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};
    use base64::prelude::*;
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    let salt = BASE64_STANDARD
        .decode(&key_file.salt)
        .map_err(|e| format!("Failed to decode salt: {}", e))?;
    let enc_key = BASE64_STANDARD
        .decode(&key_file.enc_priv_key)
        .map_err(|e| format!("Failed to decode encrypted key: {}", e))?;
    let nonce_bytes = BASE64_STANDARD
        .decode(&key_file.nonce)
        .map_err(|e| format!("Failed to decode nonce: {}", e))?;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, 100_000, &mut key);

    let cipher = Aes256Gcm::new(&key.into());
    let nonce = aes_gcm::Nonce::from_slice(&nonce_bytes);

    let priv_key_bytes = cipher
        .decrypt(nonce, enc_key.as_ref())
        .map_err(|_| "Wrong password or corrupted key file".to_string())?;

    let signing_key = ed25519_dalek::SigningKey::from_bytes(
        &priv_key_bytes
            .try_into()
            .map_err(|_| "Invalid key length")?,
    );

    // 存储密钥
    let mut guard = client::SIGNING_KEY
        .lock()
        .map_err(|_| "Failed to lock key store")?;
    *guard = Some((signing_key, key_file.pub_key));

    Ok(())
}

/// 断开连接
pub async fn disconnect() {
    let mut conn = client::CONNECTION.lock().await;
    *conn = None;
}

/// 检查是否已连接
pub async fn is_connected() -> bool {
    let conn = client::CONNECTION.lock().await;
    conn.is_some()
}
