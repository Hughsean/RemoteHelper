use crate::state::AppState;
use anyhow::{Context, Result};
use std::process::Stdio;
use tokio::process::Command;

pub async fn start_service(state: &AppState, id: usize) -> Result<()> {
    let config = &state
        .config
        .service
        .get(id)
        .context("Service ID not found")?;

    if is_service_running(state, id).await {
        return Ok(()); // Already running
    }

    tracing::info!("Starting service: {}", config.description);

    let mut cmd = Command::new(&config.exe_path);
    cmd.args(&config.args)
        .stdout(Stdio::null()) // TODO: Redirect to log file?
        .stderr(Stdio::null());

    // Windows specific: create no window if needed, but tokio::process usually handles this for background tasks
    // For now, simple spawn
    let child = cmd
        .spawn()
        .context(format!("Failed to spawn {}", config.description))?;

    {
        let mut processes = state.service_processes.write().await;
        processes.insert(id, child);
    }

    Ok(())
}

pub async fn stop_service(state: &AppState, id: usize) -> Result<()> {
    // We need to be careful not to hold the lock across await points if we were using async kill,
    // but Child::kill is async in tokio.
    // However, we need to take the child out of the map to kill it.

    let child_opt = {
        let mut processes = state.service_processes.write().await;
        processes.remove(&id)
    };

    if let Some(mut child) = child_opt {
        tracing::info!("Stopping service ID: {}", id);
        child.kill().await.context("Failed to kill process")?;
    }

    Ok(())
}

pub async fn restart_service(state: &AppState, id: usize) -> Result<()> {
    stop_service(state, id).await?;
    start_service(state, id).await?;
    Ok(())
}

pub async fn is_service_running(state: &AppState, id: usize) -> bool {
    let mut processes = state.service_processes.write().await;
    if let Some(child) = processes.get_mut(&id) {
        // try_wait() returns Ok(Some(status)) if exited, Ok(None) if running
        match child.try_wait() {
            Ok(None) => true,
            Ok(Some(_)) => {
                // Process exited, remove it from map (cleanup)
                processes.remove(&id);
                false
            }
            Err(_) => false,
        }
    } else {
        false
    }
}

pub async fn start_web_tunnel(state: &AppState) -> Result<()> {
    if !state.config.web_panel.enabled {
        return Ok(());
    }

    let config = &state.config.web_panel;
    tracing::info!("Starting Web Panel Tunnel...");

    // Split args string into parts (simple split by space, might need better parsing for quotes)
    let args: Vec<&str> = config.frpc_arg.split_whitespace().collect();

    let mut cmd = Command::new(&config.frpc_exe_path);
    cmd.args(args).stdout(Stdio::null()).stderr(Stdio::null());

    let child = cmd.spawn().context("Failed to spawn Web Tunnel")?;

    {
        let mut tunnel = state.web_tunnel_process.lock().await;
        *tunnel = Some(child);
    }

    Ok(())
}
