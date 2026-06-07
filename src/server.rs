use crate::handlers;
use crate::state::AppState;
use anyhow::Result;
use base64::prelude::*;
use common::{Request, Response};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// 每条连接的认证会话状态。
struct ConnectionSession {
    authenticated: bool,
    current_challenge: Option<(String, Instant)>,
}

impl ConnectionSession {
    fn new() -> Self {
        Self {
            authenticated: false,
            current_challenge: None,
        }
    }
}

pub async fn handle_connection(socket: TcpStream, state: AppState) {
    let result = tokio::time::timeout(
        Duration::from_secs(state.config.web_panel.connection_timeout_secs),
        handle_connection_inner(socket, state.clone()),
    )
    .await;

    state.active_connections.fetch_sub(1, Ordering::Relaxed);

    match result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => error!("Connection error: {}", e),
        Err(_) => error!("Connection timeout"),
    }
}

async fn handle_connection_inner(socket: TcpStream, state: AppState) -> Result<()> {
    state.active_connections.fetch_add(1, Ordering::Relaxed);

    // --- 握手阶段 ---
    let (mut read_half, write_half, crypto) =
        crate::protocol::handshake::perform_handshake(socket).await?;

    // --- 响应发送任务 ---
    let (resp_tx, mut resp_rx) = mpsc::channel::<Response>(32);
    let writer_crypto = crypto.clone();

    let writer = tokio::spawn(async move {
        while let Some(response) = resp_rx.recv().await {
            let mut sock_guard = write_half.lock().await;
            if let Err(e) = crate::protocol::frame::send_encrypted_response(
                &mut sock_guard,
                &writer_crypto,
                &response,
            )
            .await
            {
                error!("Failed to send response: {}", e);
                break;
            }
        }
    });

    // --- 主请求读取循环 ---
    let mut session = ConnectionSession::new();
    let mut buf = [0u8; 10 * 1024];

    loop {
        use crate::protocol::frame::FrameResult;
        let req = match crate::protocol::frame::read_next_request(&mut read_half, &crypto, &mut buf)
            .await
        {
            Ok(FrameResult::Message(req)) => req,
            Ok(FrameResult::Invalid(msg)) => {
                // JSON 解析失败 — 返回错误但不断开连接
                let _ = resp_tx.send(Response::Error(msg)).await;
                continue;
            }
            Ok(FrameResult::Closed) => break,
            Err(e) => {
                error!("Frame read error: {}", e);
                break;
            }
        };

        tracing::debug!("Processing request: {:?}", std::mem::discriminant(&req));

        match &req {
            Request::GetChallenge => {
                let nonce: [u8; 32] = rand::random();
                let nonce_str = BASE64_STANDARD.encode(nonce);
                session.current_challenge = Some((nonce_str.clone(), Instant::now()));
                if resp_tx.send(Response::Challenge(nonce_str)).await.is_err() {
                    break;
                }
            }
            Request::Login {
                public_key,
                signature,
            } => {
                let resp = if let Some((challenge, timestamp)) = &session.current_challenge {
                    if timestamp.elapsed() > Duration::from_secs(30) {
                        session.current_challenge = None;
                        Response::Error("挑战已过期".to_string())
                    } else if handlers::verify_login(&state, public_key, signature, challenge) {
                        session.authenticated = true;
                        session.current_challenge = None;
                        info!("Client authenticated: key={}", public_key);
                        Response::Ok
                    } else {
                        warn!("Authentication failed for key: {}", public_key);
                        Response::Error("认证失败".to_string())
                    }
                } else {
                    Response::Error("未请求挑战".to_string())
                };
                if resp_tx.send(resp).await.is_err() {
                    break;
                }
            }
            _ => {
                if !session.authenticated {
                    if resp_tx
                        .send(Response::Error("未授权".to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
                if !handlers::dispatch(req, state.clone(), resp_tx.clone()) {
                    // 无匹配 handler — 返回错误但不断开连接
                    if resp_tx
                        .send(Response::Error("无效的请求状态".to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        }
    }

    drop(resp_tx);
    let _ = writer.await;

    Ok(())
}
