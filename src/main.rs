mod cleanup;
mod config;
mod handlers;
mod health;
mod monitor;
mod process;
mod protocol;
mod server;
mod state;

use crate::config::AppConfig;
use crate::state::AppState;
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // 初始化 tracing
    #[cfg(debug_assertions)]
    let _guard =
        common::logging::tracing_init(Some("logs"), Some("server.log"), tracing::Level::DEBUG);
    #[cfg(not(debug_assertions))]
    let _guard =
        common::logging::tracing_init(Some("logs"), Some("server.log"), tracing::Level::INFO);

    tracing::info!("正在启动 RemoteHelper 服务器实例");

    // 加载配置
    let config = AppConfig::load()?;

    // 网络初始化等待
    wait_for_network(&config).await;

    tracing::info!("配置文件加载成功");
    tracing::info!(
        "Web 面板配置 - 启用: {}, 端口: {}, 最大连接数: {}",
        config.web_panel.enabled,
        config.web_panel.local_port,
        config.web_panel.max_connections
    );
    tracing::info!("已配置服务数量: {}", config.service.len());

    // 初始化状态
    let state = AppState::new(config.clone());
    tracing::info!("应用程序状态初始化完成");

    // 注册请求处理器
    handlers::register_default_handlers();

    // 启动后台监控任务
    let _monitor = monitor::spawn(state.clone());

    // 自动启动服务
    auto_start_services(&state).await;

    // 自动启动 Web 隧道
    tracing::info!("正在启动 Web 隧道...");
    if let Err(e) = process::start_web_tunnel(&state).await {
        tracing::error!("启动 Web 隧道失败: {}", e);
    } else {
        tracing::info!("Web 隧道启动成功");
    }

    // 启动 TCP 监听
    let cleanup_state = state.clone();
    let port = config.web_panel.local_port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("正在监听 {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let server_state = state.clone();
    let max_connections = config.web_panel.max_connections;

    tokio::select! {
        _ = run_accept_loop(listener, server_state, max_connections) => {},
        _ = shutdown_signal() => {},
    }

    // 优雅关闭
    tracing::info!("正在关闭，终止子进程...");
    cleanup::log_remaining_connections(&cleanup_state);
    cleanup::terminate_services(&cleanup_state).await;
    cleanup::terminate_web_tunnel(&cleanup_state).await;
    tracing::info!("关闭完成\n\n\n\n");
    Ok(())
}

async fn wait_for_network(config: &AppConfig) {
    let no_url = config.web_panel.health_check_url.is_none();
    let no_delay = config.web_panel.startup_delay_secs == 0;

    if no_url && no_delay {
        return;
    }

    tracing::info!(
        "等待网络初始化... {} 秒",
        config.web_panel.startup_delay_secs
    );

    let delay = config.web_panel.startup_delay_secs;

    if !no_delay && no_url {
        tokio::time::sleep(Duration::from_secs(delay)).await;
    } else if !no_url {
        let timeout = delay.max(3);
        let mut sleep_time = 3;
        loop {
            let connected =
                health::test_http_503(config.web_panel.health_check_url.as_ref().unwrap(), timeout)
                    .await
                    .unwrap_or(false);

            if connected {
                tracing::info!("检测到网络初始化完成");
                break;
            }
            tracing::warn!("网络尚未初始化完成，{} 秒后重试...", sleep_time);
            tokio::time::sleep(Duration::from_secs(sleep_time)).await;
            sleep_time = (sleep_time * 2).min(timeout * 6);
        }
    }
}

async fn auto_start_services(state: &AppState) {
    tracing::info!("正在启动自动启动服务...");
    let auto_start_count = state
        .config
        .service
        .iter()
        .filter(|s| s.auto_start != crate::config::AutoStart::None)
        .count();
    tracing::info!("发现 {} 个标记为自动启动的服务", auto_start_count);

    for (id, svc) in state.config.service.iter().enumerate() {
        match svc.auto_start {
            crate::config::AutoStart::None => {}
            crate::config::AutoStart::OneShot => {
                tracing::info!("OneShot 自动启动服务 ID {}: {}", id, svc.description);
                if let Err(e) = process::start_service(state, id).await {
                    tracing::error!("自动启动服务失败 {} ({}): {}", id, svc.description, e);
                } else {
                    tracing::info!("成功 OneShot 启动服务 ID {}", id);
                }
            }
            crate::config::AutoStart::Continuous => {
                tracing::info!(
                    "Continuous 自动启动服务 ID {}: {}（持久运行，保留 PID 以便后续销毁）",
                    id,
                    svc.description
                );
                if let Err(e) = process::start_service(state, id).await {
                    tracing::error!("自动启动服务失败 {} ({}): {}", id, svc.description, e);
                } else {
                    tracing::info!("成功 Continuous 启动服务 ID {}", id);
                }
            }
        }
    }
}

async fn run_accept_loop(
    listener: tokio::net::TcpListener,
    state: AppState,
    max_connections: usize,
) {
    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                let current = state
                    .active_connections
                    .load(std::sync::atomic::Ordering::Relaxed);
                if current >= max_connections {
                    tracing::warn!(
                        "连接数已达上限 ({}), 拒绝来自 {} 的连接",
                        max_connections,
                        addr
                    );
                    drop(socket);
                    continue;
                }

                tracing::info!(
                    "来自 {} 的新连接 ({}/{})",
                    addr,
                    current + 1,
                    max_connections
                );

                let state = state.clone();
                tokio::spawn(async move {
                    server::handle_connection(socket, state).await;
                });
            }
            Err(e) => {
                tracing::error!("接受连接失败: {}", e);
            }
        }
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("收到信号，开始优雅关闭");
}
