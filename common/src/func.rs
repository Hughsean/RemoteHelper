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
