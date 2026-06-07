//! 业务请求模块 —— 将底层的加密帧传输封装为类型安全的业务方法。
//!
//! 本模块是前端与服务器交互的唯一入口：
//! - `send_request()`: 统一的请求-响应通道，内置自动重连
//! - `query_path()`: 文件路径补全的便捷封装

use crate::state::{CONNECTION, MOCK_MODE};
use common::{PathItem, Request, Response};
use std::sync::atomic::Ordering;
use std::time::Duration;

/// 向服务器发送请求并等待响应。
///
/// 在 Mock 模式下直接返回合成数据。
/// 若连接断开则自动重建并重试一次（5 秒超时）。
pub async fn send_request(req: Request) -> Result<Response, String> {
    if MOCK_MODE.load(Ordering::Relaxed) {
        return Ok(match req {
            Request::GetStatus { .. } => Response::Status(crate::mock::generate_mock_system_info()),
            Request::ListServices => Response::Services(vec![]),
            Request::QueryPath { .. } => Response::PathSuggestions(vec![]),
            _ => Response::Ok,
        });
    }

    let mut guard = CONNECTION.lock().await;

    if guard.is_none() {
        *guard = Some(crate::connection::connect_and_auth().await?);
    }

    let conn = guard
        .as_mut()
        .ok_or("Connection not initialized".to_string())?;

    let mut recv_buf = vec![0u8; 65536];

    // 第一次尝试
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        conn.send(&req).await?;
        conn.recv(&mut recv_buf).await
    })
    .await
    .map_err(|_| "Request timed out".to_string())?;

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => {
            log::warn!("Request failed ({}). Reconnecting...", e);
            // 丢弃旧连接，重建
            *guard = None;
            *guard = Some(crate::connection::connect_and_auth().await?);
            let conn = guard
                .as_mut()
                .ok_or("Connection not initialized".to_string())?;

            // 重试一次
            let resp = tokio::time::timeout(Duration::from_secs(5), async {
                conn.send(&req).await?;
                conn.recv(&mut recv_buf).await
            })
            .await
            .map_err(|_| "Retry request timed out".to_string())??;

            Ok(resp)
        }
    }
}

/// 查询服务器上某个路径下的文件和目录列表。
///
/// 用于前端文件选择器的路径补全功能。
pub async fn query_path(path: String) -> Result<Vec<PathItem>, String> {
    let req = Request::QueryPath { path };
    let resp = send_request(req).await?;

    match resp {
        Response::PathSuggestions(items) => Ok(items),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}
