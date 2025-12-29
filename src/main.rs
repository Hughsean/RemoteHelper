mod config;
mod state;
mod server;
mod process;

use std::net::SocketAddr;
use std::time::Duration;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use crate::config::AppConfig;
use crate::state::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true),
        )
        .with(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    tracing::info!("Starting RemoteHelper...");

    // Load configuration
    let config = AppConfig::load()?;
    tracing::info!("Configuration loaded successfully.");

    // Initialize state
    let state = AppState::new(config.clone());

    // Start background monitoring task
    let monitor_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        loop {
            interval.tick().await;
            {
                let mut sys = monitor_state.sys.lock().unwrap();
                sys.refresh_cpu_all();
                sys.refresh_memory();
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
        let mut processes = cleanup_state.service_processes.lock().unwrap();
        for (id, child) in processes.iter_mut() {
            tracing::info!("Killing service process {}", id);
            if let Err(e) = child.start_kill() {
                tracing::error!("Failed to kill service process {}: {}", id, e);
            }
        }
    }

    // Kill web tunnel
    {
        let mut tunnel = cleanup_state.web_tunnel_process.lock().unwrap();
        if let Some(child) = tunnel.as_mut() {
            tracing::info!("Killing web tunnel process");
            if let Err(e) = child.start_kill() {
                tracing::error!("Failed to kill web tunnel: {}", e);
            }
        }
    }

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
