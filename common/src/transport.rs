//! 加密帧传输层 —— 前后端共享的底层帧编码/解码。
//!
//! 帧格式:
//! ```text
//! ┌────────────────┬──────────────────────────────────────┐
//! │  4 bytes (BE)  │  AES-256-GCM 密文 (含 16-byte tag)    │
//! │  payload len   │                                      │
//! └────────────────┴──────────────────────────────────────┘
//! ```
//!
//! 本模块提供与 `Request`/`Response` 类型无关的泛型函数，
//! 只要实现了 `Serialize` / `DeserializeOwned` 的类型都可直接使用。

use crate::crypto::CryptoSession;
use anyhow::Result;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// 帧读取结果：区分正常消息、可恢复的无效帧、和连接关闭。
pub enum FrameResult<T> {
    /// 成功解析的消息
    Message(T),
    /// 帧格式有效但 JSON 无法解析（可恢复）
    Invalid(String),
    /// 连接正常关闭（EOF）
    Closed,
}

/// 将可序列化的数据加密后按帧格式写入流。
///
/// `writer` 可以是 `TcpStream`、`OwnedWriteHalf` 或任何实现 `AsyncWrite + Unpin` 的类型。
/// `crypto` 需要 `&mut` 引用，因为每次加密会递增内部 nonce 计数器。
pub async fn send_frame(
    writer: &mut (impl AsyncWrite + Unpin),
    crypto: &mut CryptoSession,
    payload: &impl Serialize,
) -> Result<()> {
    let plain_bytes = serde_json::to_vec(payload)?;
    let ciphertext = crypto.encrypt(&plain_bytes)?;

    writer.write_u32(ciphertext.len() as u32).await?;
    writer.write_all(&ciphertext).await?;
    Ok(())
}

/// 从流中读取一个加密帧，解密后反序列化为目标类型。
///
/// `buf` 是调用者提供的复用缓冲区 —— 若帧长度超过 `buf.len()` 则直接报错，
/// 避免恶意超大长度前缀导致的内存分配。
///
/// `reader` 可以是 `TcpStream`、`OwnedReadHalf` 或任何实现 `AsyncRead + Unpin` 的类型。
pub async fn recv_frame<T: DeserializeOwned>(
    reader: &mut (impl AsyncRead + Unpin),
    crypto: &mut CryptoSession,
    buf: &mut [u8],
) -> Result<FrameResult<T>> {
    let len = match reader.read_u32().await {
        Ok(n) => n as usize,
        Err(_) => return Ok(FrameResult::Closed),
    };

    if len > buf.len() {
        return Err(anyhow::anyhow!(
            "帧过大: {} 字节 (缓冲区上限: {} 字节)",
            len,
            buf.len()
        ));
    }

    if reader.read_exact(&mut buf[..len]).await.is_err() {
        return Ok(FrameResult::Closed);
    }

    let plaintext = crypto.decrypt(&buf[..len])?;

    match serde_json::from_slice(&plaintext) {
        Ok(msg) => Ok(FrameResult::Message(msg)),
        Err(e) => {
            let msg = format!("无效的 JSON: {}", e);
            tracing::warn!("{}", msg);
            Ok(FrameResult::Invalid(msg))
        }
    }
}
