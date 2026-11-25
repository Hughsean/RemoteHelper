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
    authenticated_key: Option<String>,
    current_challenge: Option<(String, Instant)>,
}

impl ConnectionSession {
    fn new() -> Self {
        Self {
            authenticated_key: None,
            current_challenge: None,
        }
    }
}

struct WriterGuard(tokio::task::AbortHandle);

impl Drop for WriterGuard {
    fn drop(&mut self) {
        self.0.abort();
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
    // --- 握手阶段 ---
    let (mut read_half, write_half, crypto, transcript_hash) =
        crate::protocol::handshake::perform_handshake(socket, state.identity.as_deref()).await?;

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
                transcript_hash.is_some(),
            )
            .await
            {
                error!("Failed to send response: {}", e);
                break;
            }
        }
    });

    // 连接超时会取消主循环，也必须立即停止持有写半端的任务。
    let _writer_guard = WriterGuard(writer.abort_handle());

    // --- 主请求读取循环 ---
    let mut session = ConnectionSession::new();
    let mut history_session = handlers::history::HistorySession::default();
    let mut sampling_lease = None;
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
            }
            | Request::LoginHistory {
                public_key,
                signature,
            } => {
                // 同一连接不可切换认证主体。
                let extended = matches!(req, Request::LoginHistory { .. });
                let resp = if session.authenticated_key.is_some() {
                    Response::Error("连接已认证".to_string())
                } else if let Some((challenge, timestamp)) = session.current_challenge.take() {
                    if timestamp.elapsed() > Duration::from_secs(30) {
                        Response::Error("挑战已过期".to_string())
                    } else if extended == transcript_hash.is_some()
                        && handlers::verify_login(
                            &state,
                            public_key,
                            signature,
                            &challenge,
                            transcript_hash.as_ref(),
                        )
                    {
                        session.authenticated_key = Some(public_key.clone());
                        info!(extended, "Client authenticated");
                        Response::Ok
                    } else {
                        warn!(extended, "Authentication rejected");
                        Response::Error("认证失败".to_string())
                    }
                } else {
                    Response::Error("未请求挑战".to_string())
                };
                if resp_tx.send(resp).await.is_err() {
                    break;
                }
            }
            Request::History(request) => {
                let verified_key = transcript_hash
                    .as_ref()
                    .and(session.authenticated_key.as_deref());
                let response =
                    handlers::history::handle(&state, &mut history_session, verified_key, request)
                        .await;
                let response =
                    crate::protocol::frame::bound_history_response(Response::History(response))?;
                if resp_tx.send(response).await.is_err() {
                    break;
                }
            }
            _ => {
                if session.authenticated_key.is_none() {
                    if resp_tx
                        .send(Response::Error("未授权".to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }
                if let Request::GetStatus { interval_ms } = &req {
                    let lease = sampling_lease.get_or_insert_with(|| state.monitor.lease());
                    lease.renew(*interval_ms);
                    if resp_tx
                        .send(handlers::system::get_status(&state).await)
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

    drop(sampling_lease);
    drop(resp_tx);
    let _ = writer.await;

    Ok(())
}
