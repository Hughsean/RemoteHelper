mod config;
mod process;
mod server;
mod state;
mod utils;

use crate::config::AppConfig;
use crate::state::AppState;
use std::net::SocketAddr;
use std::time::Duration;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    let _guard = common::func::tracing_init(Some("logs"), Some("server.log"));

    // tracing::info!("\n\n================================================================================");
    tracing::info!("正在启动 RemoteHelper 服务器实例");
    // tracing::info!("================================================================================");

    // Load configuration
    let config = AppConfig::load()?;

    let no_url = config.web_panel.health_check_url.is_none();
    let no_delay = config.web_panel.startup_delay_secs == 0;

    if !no_url || !no_delay {
        tracing::info!(
            "等待网络初始化... {} 秒",
            config.web_panel.startup_delay_secs
        );

        let delay = config.web_panel.startup_delay_secs;

        if !no_delay && no_url {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        } else if !no_url {
            let timeout = delay.max(10);
            let mut sleep_time = 1;
            loop {
                let connected = utils::test_http_503(
                    config.web_panel.health_check_url.as_ref().unwrap(),
                    timeout,
                )
                .await
                .unwrap_or(false);

                if connected {
                    tracing::info!("检测到网络初始化完成");
                    break;
                };
                tokio::time::sleep(Duration::from_secs(sleep_time)).await;
                sleep_time = (sleep_time * 2).min(timeout * 6);
            }
        }
    }
    tracing::info!("配置文件加载成功");
    tracing::info!(
        "Web 面板配置 - 启用: {}, 端口: {}, 最大连接数: {}",
        config.web_panel.enabled,
        config.web_panel.local_port,
        config.web_panel.max_connections
    );
    tracing::info!("已配置服务数量: {}", config.service.len());

    // Initialize state
    let state = AppState::new(config.clone());
    tracing::info!("应用程序状态初始化完成");

    // Start background monitoring task
    let monitor_state = state.clone();
    tokio::spawn(async move {
        loop {
            let interval_ms = *monitor_state.refresh_interval.read().await;

            // Check if we should pause updates (no reads for 10s)
            let should_pause = {
                let last_read = *monitor_state.last_read_time.read().await;
                last_read.elapsed() > Duration::from_secs(10)
            };

            let timeout = tokio::select! {
                _ = if should_pause {
                    tokio::time::sleep(Duration::from_secs(1))
                }
                else {
                    tokio::time::sleep(Duration::from_millis(interval_ms))
                } => {
                    true
                    // Timer expired, refresh
                }
                _ = monitor_state.update_notify.notified() => {
                    false
                    // Config changed, wake up immediately (and refresh)
                }
            };

            if timeout && should_pause {
                // Skip this cycle
                tracing::trace!("监控已暂停 - 无最近读取");
                continue;
            }

            tracing::trace!("正在刷新系统指标...");
            {
                let mut sys = monitor_state.sys.write().await;
                sys.refresh_cpu_all();
                sys.refresh_memory();
            }
            {
                let mut networks = monitor_state.networks.write().await;
                networks.refresh(true);
            }

            // Update GPU Cache
            {
                let nvml_lock = monitor_state.nvml.read().await;
                let mut cache = monitor_state.gpu_cache.write().await;

                if let Some(nvml) = &*nvml_lock
                    && let Ok(device) = nvml.device_by_index(0)
                {
                    cache.usage = device.utilization_rates().map(|r| r.gpu).ok();
                    let (mem, total) = device
                        .memory_info()
                        .map(|i| (Some(i.used), Some(i.total)))
                        .unwrap_or((None, None));
                    cache.memory_used = mem;
                    cache.memory_total = total;
                    cache.model = device.name().ok();
                }
            }
        }
    });

    // Auto-start services
    tracing::info!("正在启动自动启动服务...");
    let auto_start_count = state.config.service.iter().filter(|s| s.auto_start).count();
    tracing::info!("发现 {} 个标记为自动启动的服务", auto_start_count);

    for (id, svc) in state.config.service.iter().enumerate() {
        if svc.auto_start {
            tracing::info!("自动启动服务 ID {}: {}", id, svc.description);
            if let Err(e) = process::start_service(&state, id, None, None).await {
                tracing::error!("自动启动服务失败 {} ({}): {}", id, svc.description, e);
            } else {
                tracing::info!("成功自动启动服务 ID {}", id);
            }
        }
    }

    // Auto-start web tunnel
    tracing::info!("正在启动 Web 隧道...");
    if let Err(e) = process::start_web_tunnel(&state).await {
        tracing::error!("启动 Web 隧道失败: {}", e);
    } else {
        tracing::info!("Web 隧道启动成功");
    }

    // Keep a clone for cleanup
    let cleanup_state = state.clone();

    // Run it
    let port = config.web_panel.local_port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("正在监听 {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;

    // Accept loop
    let server_state = state.clone();
    let max_connections = config.web_panel.max_connections;
    tokio::select! {
        _ = async {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        // Check connection limit
                        let current = server_state.active_connections.load(std::sync::atomic::Ordering::Relaxed);
                        if current >= max_connections {
                            tracing::warn!("连接数已达上限 ({}), 拒绝来自 {} 的连接", max_connections, addr);
                            drop(socket);
                            continue;
                        }

                        tracing::info!("来自 {} 的新连接 ({}/{})", addr, current + 1, max_connections);

                        let state = server_state.clone();
                        tokio::spawn(async move {
                            server::handle_connection(socket, state).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("接受连接失败: {}", e);
                    }
                }
            }
        } => {},
        _ = shutdown_signal() => {},
    }

    // Cleanup logic
    tracing::info!("正在关闭，终止子进程...");
    let active = cleanup_state
        .active_connections
        .load(std::sync::atomic::Ordering::Relaxed);
    if active > 0 {
        tracing::warn!("关闭时仍有 {} 个活动连接", active);
    }

    // Kill service processes with timeout
    {
        let mut processes = cleanup_state.service_processes.write().await;
        tracing::info!("发现 {} 个需要终止的服务进程", processes.len());
        for (id, handle) in processes.iter_mut() {
            tracing::info!("正在终止服务进程 {}", id);
            match handle {
                state::ProcessHandle::Child(child) => {
                    if let Err(e) = child.start_kill() {
                        tracing::error!("终止服务进程 {} 失败: {}", id, e);
                        continue;
                    }
                    // Wait for process to exit with 5 second timeout
                    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                        Ok(Ok(status)) => {
                            tracing::info!("服务 {} 退出，状态: {:?}", id, status)
                        }
                        Ok(Err(e)) => tracing::error!("等待服务 {} 时出错: {}", id, e),
                        Err(_) => {
                            tracing::warn!("服务 {} 未在超时时间内退出，强制终止", id);
                            let _ = child.kill().await;
                        }
                    }
                }
                state::ProcessHandle::Pid(pid) => {
                    // 用户模式：使用 taskkill
                    tracing::info!("正在终止用户模式进程，PID: {}", pid);
                    let _ = std::process::Command::new("taskkill")
                        .args(&["/F", "/PID", &pid.to_string()])
                        .output();
                }
            }
        }
    }

    // Kill web tunnel with timeout
    {
        let mut tunnel = cleanup_state.web_tunnel_process.lock().await;
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
    tracing::info!("关闭完成\n\n\n\n");
    Ok(())
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
