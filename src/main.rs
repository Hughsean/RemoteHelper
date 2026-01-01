mod config;
mod process;
mod server;
mod state;
mod utils;

use crate::config::AppConfig;
use crate::state::AppState;
use std::net::SocketAddr;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    let file_appender = tracing_appender::rolling::never("logs", "server.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .with_writer(std::io::stdout),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true)
                .with_ansi(false)
                .with_writer(non_blocking),
        )
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    // tracing::info!("\n\n================================================================================");
    tracing::info!("Starting RemoteHelper Server Instance");
    // tracing::info!("================================================================================");

    // Load configuration
    let config = AppConfig::load()?;

    let no_url = config.web_panel.health_check_url.is_none();
    let no_delay = config.web_panel.startup_delay_secs == 0;

    if !no_url || !no_delay {
        tracing::info!(
            "Waiting for network initialization... {} seconds",
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
                    tracing::info!("Network initialization detected");
                    break;
                };
                tokio::time::sleep(Duration::from_secs(sleep_time)).await;
                sleep_time = (sleep_time * 2).min(timeout * 6);
            }
        }
    }
    tracing::info!("Configuration loaded successfully");

    // Initialize state
    let state = AppState::new(config.clone());

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
                continue;
            }

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
    for (id, svc) in state.config.service.iter().enumerate() {
        if svc.auto_start
            && let Err(e) = process::start_service(&state, id).await
        {
            tracing::error!("Failed to auto-start service {}: {}", svc.description, e);
        }
    }

    // Auto-start web tunnel
    if let Err(e) = process::start_web_tunnel(&state).await {
        tracing::error!("Failed to start web tunnel: {}", e);
    }

    // Keep a clone for cleanup
    let cleanup_state = state.clone();

    // Run it
    let port = config.web_panel.local_port;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("Listening on {}", addr);

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
                            tracing::warn!("Connection limit reached ({}), rejecting connection from {}", max_connections, addr);
                            drop(socket);
                            continue;
                        }

                        tracing::info!("New connection from {} ({}/{})", addr, current + 1, max_connections);

                        let state = server_state.clone();
                        tokio::spawn(async move {
                            server::handle_connection(socket, state).await;
                        });
                    }
                    Err(e) => {
                        tracing::error!("Failed to accept connection: {}", e);
                    }
                }
            }
        } => {},
        _ = shutdown_signal() => {},
    }

    // Cleanup logic
    tracing::info!("Shutting down, killing child processes...");

    // Kill service processes with timeout
    {
        let mut processes = cleanup_state.service_processes.write().await;
        for (id, child) in processes.iter_mut() {
            tracing::info!("Killing service process {}", id);
            if let Err(e) = child.start_kill() {
                tracing::error!("Failed to kill service process {}: {}", id, e);
                continue;
            }
            // Wait for process to exit with 5 second timeout
            match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                Ok(Ok(status)) => tracing::info!("Service {} exited with status: {:?}", id, status),
                Ok(Err(e)) => tracing::error!("Error waiting for service {}: {}", id, e),
                Err(_) => {
                    tracing::warn!("Service {} did not exit within timeout, force killing", id);
                    let _ = child.kill().await;
                }
            }
        }
    }

    // Kill web tunnel with timeout
    {
        let mut tunnel = cleanup_state.web_tunnel_process.lock().await;
        if let Some(child) = tunnel.as_mut() {
            tracing::info!("Killing web tunnel process");
            if let Err(e) = child.start_kill() {
                tracing::error!("Failed to kill web tunnel: {}", e);
            } else {
                match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
                    Ok(Ok(status)) => tracing::info!("Web tunnel exited with status: {:?}", status),
                    Ok(Err(e)) => tracing::error!("Error waiting for web tunnel: {}", e),
                    Err(_) => {
                        tracing::warn!("Web tunnel did not exit within timeout, force killing");
                        let _ = child.kill().await;
                    }
                }
            }
        }
    }
    tracing::info!("Shutdown complete\n\n\n\n");
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

    tracing::info!("Signal received, starting graceful shutdown");
}
