use anyhow::Result;
use common::{Request, Response, crypto::CryptoSession};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

/// 从流中读取一个加密帧，解密并反序列化为 Request。
/// 返回 `Ok(None)` 表示连接正常关闭。
pub async fn read_next_request(
    read_half: &mut OwnedReadHalf,
    crypto: &Arc<Mutex<CryptoSession>>,
    buf: &mut [u8],
) -> Result<Option<Request>> {
    let len = match read_half.read_u32().await {
        Ok(n) => n as usize,
        Err(e) => {
            tracing::debug!("Failed to read frame length: {}", e);
            return Ok(None);
        }
    };

    if len > buf.len() {
        tracing::error!("Request too large: {} bytes", len);
        return Err(anyhow::anyhow!("Request too large: {} bytes", len));
    }

    if read_half.read_exact(&mut buf[..len]).await.is_err() {
        tracing::debug!("Connection closed during frame read");
        return Ok(None);
    }

    let plaintext = {
        let mut cry = crypto.lock().await;
        cry.decrypt(&buf[..len])
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?
    };

    let req = serde_json::from_slice(&plaintext)
        .map_err(|e| anyhow::anyhow!("Invalid JSON request: {}", e))?;

    Ok(Some(req))
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
