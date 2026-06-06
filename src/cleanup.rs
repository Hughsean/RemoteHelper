use crate::state::AppState;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// 终止所有受管理的服务进程（带 5 秒超时后强制 kill）。
pub async fn terminate_services(state: &AppState) {
    let mut processes = state.service_processes.write().await;
    if processes.is_empty() {
        return;
    }
    tracing::info!("发现 {} 个需要终止的服务进程", processes.len());
    for (id, sp) in processes.iter_mut() {
        tracing::info!("正在终止服务进程 {}", id);
        if let Err(e) = sp.start_kill() {
            tracing::error!("终止服务进程 {} 失败: {}", id, e);
            continue;
        }
        match tokio::time::timeout(Duration::from_secs(5), sp.wait()).await {
            Ok(Ok(status)) => {
                tracing::info!("服务 {} 退出，状态: {:?}", id, status)
            }
            Ok(Err(e)) => tracing::error!("等待服务 {} 时出错: {}", id, e),
            Err(_) => {
                tracing::warn!("服务 {} 未在超时时间内退出，强制终止", id);
                let _ = sp.kill().await;
            }
        }
    }
}

/// 终止 Web 隧道进程（带 5 秒超时后强制 kill）。
pub async fn terminate_web_tunnel(state: &AppState) {
    let mut tunnel = state.web_tunnel_process.lock().await;
    if let Some(child) = tunnel.as_mut() {
        tracing::info!("正在终止 Web 隧道进程");
        if let Err(e) = child.start_kill() {
            tracing::error!("终止 Web 隧道失败: {}", e);
        } else {
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => tracing::info!("Web 隧道退出，状态: {:?}", status),
                Ok(Err(e)) => tracing::error!("等待 Web 隧道时出错: {}", e),
                Err(_) => {
                    tracing::warn!("Web 隧道未在超时时间内退出，强制终止");
                    let _ = child.kill().await;
                }
            }
        }
    }
}

/// 记录关闭时剩余的活动连接数。
pub fn log_remaining_connections(state: &AppState) {
    let active = state.active_connections.load(Ordering::Relaxed);
    if active > 0 {
        tracing::warn!("关闭时仍有 {} 个活动连接", active);
    }
}
