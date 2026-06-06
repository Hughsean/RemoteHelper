use anyhow::Result;
use common::{Request, Response, crypto::CryptoSession};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

/// 帧读取结果：区分正常请求、可恢复的无效帧、连接关闭。
pub enum FrameResult {
    /// 成功解析的请求
    Request(Request),
    /// 帧格式有效但 JSON 无法解析（可恢复，不应断开连接）
    Invalid(String),
    /// 连接正常关闭（EOF）
    Closed,
}

/// 从流中读取一个加密帧。
pub async fn read_next_request(
    read_half: &mut OwnedReadHalf,
    crypto: &Arc<Mutex<CryptoSession>>,
    buf: &mut [u8],
) -> Result<FrameResult> {
    let len = match read_half.read_u32().await {
        Ok(n) => n as usize,
        Err(_) => return Ok(FrameResult::Closed),
    };

    if len > buf.len() {
        tracing::error!("Request too large: {} bytes", len);
        return Err(anyhow::anyhow!("Request too large: {} bytes", len));
    }

    if read_half.read_exact(&mut buf[..len]).await.is_err() {
        return Ok(FrameResult::Closed);
    }

    let plaintext = {
        let mut cry = crypto.lock().await;
        cry.decrypt(&buf[..len])
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?
    };

    match serde_json::from_slice(&plaintext) {
        Ok(req) => Ok(FrameResult::Request(req)),
        Err(e) => {
            let msg = format!("无效的 JSON 请求: {}", e);
            tracing::warn!("{}", msg);
            Ok(FrameResult::Invalid(msg))
        }
    }
}

/// 加密响应并写入流中。
pub async fn send_encrypted_response(
    write_half: &mut OwnedWriteHalf,
    crypto: &mut CryptoSession,
    response: &Response,
) -> Result<()> {
    let resp_bytes = serde_json::to_vec(response)?;
    let ciphertext = crypto.encrypt(&resp_bytes)?;

    write_half.write_u32(ciphertext.len() as u32).await?;
    write_half.write_all(&ciphertext).await?;
    Ok(())
}
