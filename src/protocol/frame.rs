//! 服务端帧处理 —— 基于 [`common::transport`] 的薄封装。
//!
//! 将通用的 `send_frame` / `recv_frame` 特化为服务端的 `Request` / `Response` 类型，
//! 并处理 `Arc<Mutex<CryptoSession>>` 的锁获取。

use anyhow::Result;
use common::{Request, Response, crypto::CryptoSession};
use std::sync::Arc;
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
    let mut cry = crypto.lock().await;
    common::transport::recv_frame::<Request>(read_half, &mut *cry, buf).await
}

/// 加密 `Response` 后按帧格式写入流中。
///
/// 直接委托给 [`common::transport::send_frame`]。
pub async fn send_encrypted_response(
    write_half: &mut OwnedWriteHalf,
    crypto: &mut CryptoSession,
    response: &Response,
) -> Result<()> {
    common::transport::send_frame(write_half, crypto, response).await
}
