//! 连接建立与认证模块。
//!
//! 实现完整的连接生命周期：
//! 1. TCP 连接 → 2. X25519 密钥交换 → 3. Ed25519 挑战-应答认证
//!
//! 认证成功后返回 `EncryptedConnection` 供上层请求模块使用。

use crate::EncryptedConnection;
use crate::state::{SERVER_ADDRESS, SIGNING_KEY};
use base64::prelude::*;
use common::{Handshake, Request, Response, crypto::CryptoSession};
use ed25519_dalek::Signer;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::PublicKey;

/// 完整的连接 + 握手 + 认证流程。
///
/// # 流程
/// 1. 从 `SIGNING_KEY` 获取已解密的 Ed25519 密钥
/// 2. 连接 `SERVER_ADDRESS`
/// 3. X25519 ECDH 握手（ClientHello / ServerHello）
/// 4. 建立 AES-256-GCM 加密会话
/// 5. GetChallenge → Ed25519 签名 → Login
///
/// # 返回
/// 认证成功返回 `EncryptedConnection`，失败返回错误描述。
pub async fn connect_and_auth() -> Result<EncryptedConnection, String> {
    // 1. 获取签名密钥
    let (signing_key, pub_key) = {
        let guard = SIGNING_KEY
            .lock()
            .map_err(|_| "Failed to lock key store".to_string())?;
        match &*guard {
            Some((k, p)) => (k.clone(), p.clone()),
            None => return Err("Not authenticated. Please login first.".to_string()),
        }
    };

    // 2. TCP 连接
    let addr = {
        let guard = SERVER_ADDRESS
            .lock()
            .map_err(|_| "Failed to lock server address".to_string())?;
        guard.clone()
    };

    let mut stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    // ─── X25519 握手阶段 ───

    // 2.1 生成临时密钥并发送 ClientHello
    let (secret, client_public) = common::crypto::generate_ephemeral();
    let client_pub_b64 = BASE64_STANDARD.encode(client_public.as_bytes());

    let hello = Handshake::ClientHello {
        public_key: client_pub_b64,
    };
    let hello_bytes = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
    stream
        .write_u32(hello_bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(&hello_bytes)
        .await
        .map_err(|e| e.to_string())?;

    // 2.2 读取 ServerHello
    let len = stream.read_u32().await.map_err(|e| e.to_string())? as usize;
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| e.to_string())?;
    let server_hello: Handshake = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;

    let server_pub_bytes = match server_hello {
        Handshake::ServerHello { public_key } => BASE64_STANDARD
            .decode(public_key)
            .map_err(|e| e.to_string())?,
        _ => return Err("Expected ServerHello".to_string()),
    };

    // 2.3 初始化加密会话
    let server_pub_array: [u8; 32] = server_pub_bytes
        .try_into()
        .map_err(|_| "Invalid server public key length".to_string())?;
    let server_public_key = PublicKey::from(server_pub_array);
    let shared_secret = secret.diffie_hellman(&server_public_key);
    let crypto = CryptoSession::new(shared_secret.to_bytes(), false); // is_server = false

    let mut conn = EncryptedConnection::new(stream, crypto);

    // ─── 认证阶段 (已加密) ───

    let mut recv_buf = vec![0u8; 4096];

    // 3.1 获取挑战
    conn.send(&Request::GetChallenge).await?;
    let challenge = match conn.recv(&mut recv_buf).await? {
        Response::Challenge(c) => c,
        Response::Error(e) => return Err(format!("Server error during handshake: {}", e)),
        _ => return Err("Unexpected response during handshake".to_string()),
    };

    // 3.2 签名挑战
    let challenge_bytes = BASE64_STANDARD
        .decode(&challenge)
        .map_err(|e| format!("Invalid challenge format: {}", e))?;
    let signature = signing_key.sign(&challenge_bytes);
    let signature_str = BASE64_STANDARD.encode(signature.to_bytes());

    // 3.3 发送 Login
    conn.send(&Request::Login {
        public_key: pub_key,
        signature: signature_str,
    })
    .await?;

    match conn.recv(&mut recv_buf).await? {
        Response::Ok => Ok(conn),
        Response::Error(e) => Err(format!("Login failed: {}", e)),
        _ => Err("Unexpected response during login".to_string()),
    }
}
