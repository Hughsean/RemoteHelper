//! 服务端帧处理 —— 基于 [`common::transport`] 的薄封装。
//!
//! 将通用的 `send_frame` / `recv_frame` 特化为服务端的 `Request` / `Response` 类型，
//! 并处理 `Arc<Mutex<CryptoSession>>` 的锁获取。

use anyhow::Result;
use common::{Request, Response, crypto::CryptoSession};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::Mutex;

/// 帧读取结果 —— 特化为 `Request` 类型。
///
/// 直接重导出 [`common::transport::FrameResult`]，方便服务端匹配。
pub type FrameResult = common::transport::FrameResult<Request>;

/// 从流中读取一个加密帧并反序列化为 `Request`。
///
/// 内部获取 `crypto` 的互斥锁后委托给 [`common::transport::recv_frame`]。
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
        return Err(anyhow::anyhow!(
            "帧过大: {} 字节 (缓冲区上限: {} 字节)",
            len,
            buf.len()
        ));
    }

    if read_half.read_exact(&mut buf[..len]).await.is_err() {
        return Ok(FrameResult::Closed);
    }

    let plaintext = {
        let mut cry = crypto.lock().await;
        cry.decrypt(&buf[..len])?
    };

    match serde_json::from_slice(&plaintext) {
        Ok(req) => Ok(FrameResult::Message(req)),
        Err(e) => {
            let msg = format!("无效的 JSON: {}", e);
            tracing::warn!("{}", msg);
            Ok(FrameResult::Invalid(msg))
        }
    }
}

/// 加密 `Response` 后按帧格式写入流中。
///
/// 直接委托给 [`common::transport::send_frame`]。
pub async fn send_encrypted_response(
    write_half: &mut OwnedWriteHalf,
    crypto: &Arc<Mutex<CryptoSession>>,
    response: &Response,
) -> Result<()> {
    let ciphertext = {
        let plain_bytes = serde_json::to_vec(response)?;
        let mut cry = crypto.lock().await;
        cry.encrypt(&plain_bytes)?
    };

    write_half.write_u32(ciphertext.len() as u32).await?;
    write_half.write_all(&ciphertext).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::net::{TcpListener, TcpStream};

    #[tokio::test]
    async fn read_next_request_does_not_hold_crypto_lock_while_waiting_for_frame() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let client = TcpStream::connect(addr);
        let server = listener.accept();
        let (client_stream, server_accept) = tokio::join!(client, server);
        let client_stream = client_stream.unwrap();
        let (server_stream, _) = server_accept.unwrap();

        let (mut read_half, _) = server_stream.into_split();
        let crypto = Arc::new(Mutex::new(CryptoSession::new([7u8; 32], true)));
        let reader_crypto = crypto.clone();

        let read_task = tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            read_next_request(&mut read_half, &reader_crypto, &mut buf).await
        });

        tokio::time::sleep(Duration::from_millis(25)).await;
        assert!(
            crypto.try_lock().is_ok(),
            "read_next_request must not hold the crypto lock while awaiting network data"
        );

        drop(client_stream);
        let _ = read_task.await.unwrap();
    }
}
