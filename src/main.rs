mod config;
mod process;
mod server;
mod state;

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
    tracing::info!("Waiting for Network Connection... 20 seconds");
    tokio::time::sleep(Duration::from_secs(20)).await;
    // Load configuration
    let config = AppConfig::load()?;
    tracing::info!("Configuration loaded successfully.");

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
                if last_read.elapsed() > Duration::from_secs(10) {
                    true
                } else {
                    false
                }
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

                if let Some(nvml) = &*nvml_lock {
                    if let Ok(device) = nvml.device_by_index(0) {
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
        }
    });

    // Auto-start services
    for (id, svc) in state.config.service.iter().enumerate() {
        if svc.auto_start {
            if let Err(e) = process::start_service(&state, id).await {
                tracing::error!("Failed to auto-start service {}: {}", svc.description, e);
            }
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
    tokio::select! {
        _ = async {
            loop {
                match listener.accept().await {
                    Ok((socket, addr)) => {
                        tracing::info!("New connection from {}", addr);
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

    // Kill service processes
    {
        let mut processes = cleanup_state.service_processes.write().await;
        for (id, child) in processes.iter_mut() {
            tracing::info!("Killing service process {}", id);
            if let Err(e) = child.start_kill() {
                tracing::error!("Failed to kill service process {}: {}", id, e);
            }
        }
    }

    // Kill web tunnel
    {
        let mut tunnel = cleanup_state.web_tunnel_process.lock().await;
        if let Some(child) = tunnel.as_mut() {
            tracing::info!("Killing web tunnel process");
            if let Err(e) = child.start_kill() {
                tracing::error!("Failed to kill web tunnel: {}", e);
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
