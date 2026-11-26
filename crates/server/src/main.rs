mod cleanup;
mod config;
mod handlers;
mod health;
mod history;
mod identity;
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
    let _guard = common::logging::tracing_init(Some("logs"), Some("server"), tracing::Level::DEBUG);
    #[cfg(not(debug_assertions))]
    let _guard = common::logging::tracing_init(Some("logs"), Some("server"), tracing::Level::INFO);

    tracing::info!("正在启动 RemoteHelper 服务器实例");

    let mut args = std::env::args_os().skip(1);
    let config_path = std::path::Path::new(config::CONFIG_PATH);
    if let Some(arg) = args.next() {
        #[cfg(windows)]
        let usage = "用法：--init-server-identity";
        #[cfg(unix)]
        let usage = "用法：--init-server-identity [服务账户标识]";
        anyhow::ensure!(arg == "--init-server-identity", "不支持的命令；{usage}");
        let reader = args
            .next()
            .map(|reader| {
                reader
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("无效的服务账户标识"))
            })
            .transpose()?;
        anyhow::ensure!(args.next().is_none(), "{usage}");
        let identity = identity::initialize(config_path, reader.as_deref())?;
        println!("服务端公钥指纹：{}", identity.fingerprint());
        return Ok(());
    }
    let (config, config_bytes) = AppConfig::load()?;
    let identity = match identity::load(config_path) {
        Ok(identity) => {
            tracing::info!(fingerprint = %identity.fingerprint(), "已加载历史服务端身份");
            Some(std::sync::Arc::new(identity))
        }
        Err(err) => {
            tracing::warn!(%err, "历史服务端身份不可用；仅开放旧版实时连接及本地记录");
            None
        }
    };

    let recorder = match history::Recorder::start(
        config.history.dir.clone(),
        history::RecorderOptions {
            enabled: config.history.enabled,
            seal_interval: Duration::from_secs(config.history.seal_interval_secs),
            retention_days: config.history.retention_days,
            retention_bytes: config.history.retention_bytes,
            compression_level: config.history.compression_level,
        },
    ) {
        Ok(recorder) => Some(recorder),
        Err(err) => {
            tracing::error!(%err, "无法启动历史记录器；实时监控仍可用");
            None
        }
    };
    let mut state = AppState::new(
        config.clone(),
        recorder.as_ref().map(history::Recorder::handle),
        &config_bytes,
    );
    state.identity = identity;
    let monitor = state.monitor.spawn(state.history.clone());
    let result = match &monitor {
        Ok(_) => {
            handlers::register_default_handlers();
            // 可选硬件驱动初始化时，主机基础指标仍可开始采样。
            let _driver = std::thread::Builder::new()
                .name("hardware-driver".into())
                .spawn(|| {
                    if let Err(err) = hwlib::driver::init_driver() {
                        tracing::warn!(%err, "CPU 硬件传感器不可用");
                    }
                });
            tokio::select! {
                result = serve(&config, &state) => result,
                _ = shutdown_signal() => Ok(()),
            }
        }
        Err(err) => Err(anyhow::anyhow!("无法启动采样器：{err}")),
    };

    let monitor_result = match monitor {
        Ok(monitor) => monitor.stop(Duration::from_secs(5)).await,
        Err(_) => Ok(()),
    };
    let recorder_result = if let Some(recorder) = recorder {
        tokio::task::spawn_blocking(move || recorder.shutdown(Duration::from_secs(10)))
            .await
            .map_err(anyhow::Error::from)
            .and_then(|result| result.map_err(anyhow::Error::from))
    } else {
        Ok(())
    };
    if let Err(err) = &monitor_result {
        tracing::error!(%err, "采样器关闭失败");
    }
    if let Err(err) = &recorder_result {
        tracing::error!(%err, "历史记录器关闭失败");
    }

    tracing::info!("正在关闭，终止子进程...");
    cleanup::log_remaining_connections(&state);
    cleanup::terminate_services(&state).await;
    cleanup::terminate_web_tunnel(&state).await;
    result.and(monitor_result).and(recorder_result)
}

async fn serve(config: &AppConfig, state: &AppState) -> anyhow::Result<()> {
    wait_for_network(config).await;
    auto_start_services(state).await;
    if let Err(err) = process::start_web_tunnel(state).await {
        tracing::error!(%err, "启动 Web 隧道失败");
    }
    let addr = SocketAddr::from(([0, 0, 0, 0], config.web_panel.local_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("正在监听 {}", addr);
    run_accept_loop(listener, state.clone(), config.web_panel.max_connections).await;
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

                state
                    .active_connections
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
