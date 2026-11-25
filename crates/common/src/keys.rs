//! Ed25519 密钥文件的加密存储与解密。
//!
//! 使用 PBKDF2-HMAC-SHA256（10 万次迭代）从密码派生 AES-256-GCM 密钥，
//! 对 Ed25519 私钥进行加密后以 `KeyFile` 格式持久化。

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit as AesKeyInit},
};
use base64::prelude::*;
use ed25519_dalek::SigningKey;
use hmac::digest::KeyInit as HmacKeyInit;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

pub const PBKDF2_SHA256_ITERATIONS: u32 = 100_000;

pub fn pbkdf2_hmac_sha256(
    password: &[u8],
    salt: &[u8],
    iterations: u32,
    out: &mut [u8],
) -> Result<(), String> {
    if iterations == 0 {
        return Err("PBKDF2 iterations must be greater than 0".to_string());
    }

    let mut block_index: u32 = 1;
    let mut written = 0usize;

    while written < out.len() {
        let mut mac = <Hmac<Sha256> as HmacKeyInit>::new_from_slice(password)
            .map_err(|e| format!("HMAC init failed: {}", e))?;
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());

        let mut u = mac.finalize().into_bytes();
        let mut t = [0u8; 32];
        t.copy_from_slice(&u);

        for _ in 1..iterations {
            let mut iter_mac = <Hmac<Sha256> as HmacKeyInit>::new_from_slice(password)
                .map_err(|e| format!("HMAC init failed: {}", e))?;
            iter_mac.update(&u);
            u = iter_mac.finalize().into_bytes();

            for (ti, ui) in t.iter_mut().zip(u.iter()) {
                *ti ^= *ui;
            }
        }

        let remaining = out.len() - written;
        let take = remaining.min(t.len());
        out[written..written + take].copy_from_slice(&t[..take]);

        written += take;
        block_index = block_index
            .checked_add(1)
            .ok_or_else(|| "PBKDF2 block index overflow".to_string())?;
    }

    Ok(())
}

pub fn derive_passphrase_key(passphrase: &str, salt: &[u8]) -> Result<[u8; 32], String> {
    let mut key = [0u8; 32];
    pbkdf2_hmac_sha256(
        passphrase.as_bytes(),
        salt,
        PBKDF2_SHA256_ITERATIONS,
        &mut key,
    )?;
    Ok(key)
}

fn build_cipher_from_passphrase(passphrase: &str, salt: &[u8]) -> Result<Aes256Gcm, String> {
    let key = derive_passphrase_key(passphrase, salt)
        .map_err(|e| format!("Key derivation failed: {}", e))?;
    Ok(Aes256Gcm::new(&key.into()))
}

#[derive(Serialize, Deserialize)]
pub struct KeyFile {
    pub pub_key: String,
    pub enc_priv_key: String,
    pub salt: String,
    pub nonce: String,
}

/// 使用密码将 Ed25519 私钥加密并打包为 KeyFile。
pub fn encrypt_private_key(signing_key: &SigningKey, passphrase: &str) -> Result<KeyFile, String> {
    let salt: [u8; 16] = rand::random();
    let cipher = build_cipher_from_passphrase(passphrase, &salt)?;

    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let priv_key_bytes = signing_key.to_bytes();

    let ciphertext = cipher
        .encrypt(nonce, priv_key_bytes.as_ref())
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(KeyFile {
        pub_key: BASE64_STANDARD.encode(signing_key.verifying_key().to_bytes()),
        enc_priv_key: BASE64_STANDARD.encode(ciphertext),
        salt: BASE64_STANDARD.encode(salt),
        nonce: BASE64_STANDARD.encode(nonce_bytes),
    })
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

    // 从密码派生密钥并构造 cipher
    let cipher = build_cipher_from_passphrase(passphrase, &salt)?;
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
