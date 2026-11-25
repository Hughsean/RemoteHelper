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
pub mod history;
mod mock;
mod requests;
mod server_pins;
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
    server_identity: Option<common::server_auth::ServerIdentity>,
}

impl EncryptedConnection {
    async fn recv_history(
        self,
        mut buffer: Vec<u8>,
    ) -> Result<(Self, Vec<u8>, common::Response), String> {
        use tokio::io::AsyncReadExt;
        let mut conn = self;
        let len = conn.stream.read_u32().await.map_err(|e| e.to_string())? as usize;
        if !(16..=buffer.len().min(common::history_api::HISTORY_FRAME_LIMIT)).contains(&len) {
            return Err("无效的历史响应帧长度".into());
        }
        conn.stream
            .read_exact(&mut buffer[..len])
            .await
            .map_err(|e| e.to_string())?;
        // 连接与缓冲区一并移交，取消后不会被下一请求误用。
        tokio::task::spawn_blocking(move || {
            let plain = conn
                .crypto
                .decrypt(&buffer[..len])
                .map_err(|e| e.to_string())?;
            let response = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
            Ok((conn, buffer, response))
        })
        .await
        .map_err(|e| e.to_string())?
    }

    /// 从已握手的流和加密会话创建新实例。
    pub fn new(stream: TcpStream, crypto: CryptoSession) -> Self {
        Self {
            stream,
            crypto,
            server_identity: None,
        }
    }

    /// 持久绑定且完成扩展登录的服务端身份；旧协议返回 None。
    pub fn server_identity(&self) -> Option<&common::server_auth::ServerIdentity> {
        self.server_identity.as_ref()
    }

    /// 加密并发送一个请求帧。
    ///
    /// 内部调用 [`common::transport::send_frame`]。
    pub async fn send(&mut self, req: &common::Request) -> Result<(), String> {
        if matches!(req, common::Request::History(_)) && self.server_identity.is_none() {
            return Err("History requires a verified and pinned server identity".into());
        }
        common::transport::send_frame(&mut self.stream, &mut self.crypto, req)
            .await
            .map_err(|e| format!("Send failed: {}", e))
    }

    /// 接收并解密一个响应帧。
    ///
    /// `buf` 是复用缓冲区 —— 帧数据不能超过 `buf.len()` 字节。
    /// 内部调用 [`common::transport::recv_frame`]。
    pub async fn recv(&mut self, buf: &mut [u8]) -> Result<common::Response, String> {
        let frame_limit = if self.server_identity.is_some() {
            buf.len().min(common::history_api::HISTORY_FRAME_LIMIT)
        } else {
            buf.len()
        };
        match common::transport::recv_frame::<common::Response>(
            &mut self.stream,
            &mut self.crypto,
            &mut buf[..frame_limit],
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

pub use connection::{
    ServerProbe, connect_and_auth, connect_verified, connect_with_pin, probe_server,
};
pub use ed25519_dalek::SigningKey;
pub use mock::generate_mock_system_info;
pub use requests::{query_path, send_request};
pub use server_pins::{
    confirm_server_identity, list_server_pins, load_server_pin, rebind_server_identity,
};
pub use state::{CONNECTION, MOCK_MODE, SERVER_ADDRESS, SIGNING_KEY};
