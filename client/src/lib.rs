//! RemoteHelper TCP 客户端库。
//!
//! 提供从连接建立、认证到加密请求-响应的完整客户端功能。
//!
//! # 模块结构
//! - [`state`]: 全局状态（签名密钥、服务器地址、连接句柄、Mock 开关）
//! - [`connection`]: 连接建立 + X25519 握手 + Ed25519 认证
//! - [`requests`]: 业务请求方法（`send_request`, `query_path`）
//! - [`mock`]:   离线 Mock 数据生成
//!
//! # 使用示例
//! ```ignore
//! // 1. 登录页注入密钥
//! *client::SIGNING_KEY.lock().unwrap() = Some((signing_key, pub_key));
//! *client::SERVER_ADDRESS.lock().unwrap() = "host:port".to_string();
//!
//! // 2. 建立连接
//! client::connect_and_auth().await?;
//!
//! // 3. 发送请求
//! let resp = client::send_request(Request::GetStatus { interval_ms: Some(1000) }).await?;
//! ```

mod connection;
mod mock;
mod requests;
mod state;

use common::crypto::CryptoSession;
use tokio::net::TcpStream;

// ─── EncryptedConnection ────────────────────────────────────

/// 已建立加密会话的 TCP 连接。
///
/// 持有底层的 `TcpStream` 和 `CryptoSession`，
/// 通过 [`send`](EncryptedConnection::send) /
/// [`recv`](EncryptedConnection::recv) 方法进行加密通信。
pub struct EncryptedConnection {
    stream: TcpStream,
    crypto: CryptoSession,
}

impl EncryptedConnection {
    /// 从已握手的流和加密会话创建新实例。
    pub fn new(stream: TcpStream, crypto: CryptoSession) -> Self {
        Self { stream, crypto }
    }

    /// 加密并发送一个请求帧。
    ///
    /// 内部调用 [`common::transport::send_frame`]。
    pub async fn send(&mut self, req: &common::Request) -> Result<(), String> {
        common::transport::send_frame(&mut self.stream, &mut self.crypto, req)
            .await
            .map_err(|e| format!("Send failed: {}", e))
    }

    /// 接收并解密一个响应帧。
    ///
    /// `buf` 是复用缓冲区 —— 帧数据不能超过 `buf.len()` 字节。
    /// 内部调用 [`common::transport::recv_frame`]。
    pub async fn recv(&mut self, buf: &mut [u8]) -> Result<common::Response, String> {
        match common::transport::recv_frame::<common::Response>(
            &mut self.stream,
            &mut self.crypto,
            buf,
        )
        .await
        .map_err(|e| format!("Receive failed: {}", e))?
        {
            common::transport::FrameResult::Message(resp) => Ok(resp),
            common::transport::FrameResult::Invalid(msg) => Err(msg),
            common::transport::FrameResult::Closed => Err("Connection closed".to_string()),
        }
    }
}

// ─── 公开 API 重导出 ────────────────────────────────────────

pub use common::keys::KeyFile;

pub use connection::connect_and_auth;
pub use mock::generate_mock_system_info;
pub use requests::{query_path, send_request};
pub use state::{CONNECTION, MOCK_MODE, SERVER_ADDRESS, SIGNING_KEY};
