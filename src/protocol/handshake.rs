use anyhow::{Result, anyhow};
use base64::prelude::*;
use common::{Handshake, crypto::CryptoSession};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, tcp::OwnedReadHalf, tcp::OwnedWriteHalf};
use tokio::sync::Mutex;
use x25519_dalek::PublicKey;

/// 执行 X25519 密钥交换握手，之后返回拆分的读写半端和加密会话。
///
/// 所有参数由所有权转移，确保调用侧无需持有中间态。
pub async fn perform_handshake(
    mut socket: TcpStream,
) -> Result<(
    OwnedReadHalf,
    Arc<Mutex<OwnedWriteHalf>>,
    Arc<Mutex<CryptoSession>>,
)> {
    let peer_addr = socket.peer_addr().ok();
    tracing::info!("正在处理来自 {:?} 的连接", peer_addr);

    let mut buf = [0u8; 1024];

    // 1. 读取 ClientHello
    let len = socket.read_u32().await? as usize;
    if len > buf.len() {
        return Err(anyhow!("Handshake message too large"));
    }
    socket.read_exact(&mut buf[..len]).await?;

    let client_hello: Handshake = serde_json::from_slice(&buf[..len])?;
    let client_pub_bytes = match client_hello {
        Handshake::ClientHello { public_key } => BASE64_STANDARD.decode(public_key)?,
        _ => return Err(anyhow!("Expected ClientHello")),
    };

    // 2. 生成服务器密钥与共享密钥
    let (secret, server_public) = common::crypto::generate_ephemeral();
    let server_pub_b64 = BASE64_STANDARD.encode(server_public.as_bytes());

    // 3. 发送 ServerHello
    let resp = Handshake::ServerHello {
        public_key: server_pub_b64,
    };
    let resp_bytes = serde_json::to_vec(&resp)?;
    socket.write_u32(resp_bytes.len() as u32).await?;
    socket.write_all(&resp_bytes).await?;

    // 4. 初始化加密会话
    let client_pub_array: [u8; 32] = client_pub_bytes
        .try_into()
        .map_err(|_| anyhow!("Invalid client public key length"))?;
    let client_public_key = PublicKey::from(client_pub_array);
    let shared_secret = secret.diffie_hellman(&client_public_key);
    let crypto = CryptoSession::new(shared_secret.to_bytes(), true);

    tracing::info!("与 {:?} 建立加密会话", peer_addr);

    // 5. 拆分 socket
    let (socket_read, socket_write) = socket.into_split();
    let socket_write = Arc::new(Mutex::new(socket_write));
    let crypto = Arc::new(Mutex::new(crypto));

    Ok((socket_read, socket_write, crypto))
}
