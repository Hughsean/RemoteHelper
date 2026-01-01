use crate::state::AppState;
use anyhow::{Context, Result};
use std::fs;
use std::process::Stdio;
use tokio::process::Command;

fn get_log_file(name: &str) -> Result<std::fs::File> {
    fs::create_dir_all("logs")?;

    // Implement simple log rotation: if file > 10MB, rotate it
    let log_path = format!("logs/{}.log", name);
    if let Ok(metadata) = fs::metadata(&log_path)
        && metadata.len() > 10 * 1024 * 1024
    {
        // 10MB
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(std::time::Duration::from_secs(0))
            .as_millis();
        let backup_path = format!("logs/{}.{}.log", name, timestamp);
        let _ = fs::rename(&log_path, backup_path);
    }

    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    Ok(file)
}

pub async fn start_service(state: &AppState, id: usize) -> Result<()> {
    let static_count = state.config.service.len();
    let config = if id < static_count {
        state.config.service[id].clone()
    } else {
        let dynamic = state.dynamic_services.read().await;
        dynamic
            .get(id - static_count)
            .cloned()
            .context("Service ID not found")?
    };

    if is_service_running(state, id).await {
        return Ok(()); // Already running
    }

    tracing::info!("Starting service: {}", config.description);

    let log_name = format!("service_id({})_{}", id, config.description);
    // We need separate handles for stdout and stderr because Stdio::from consumes the file
    let stdout_file = get_log_file(&log_name).context("Failed to create log file")?;
    let stderr_file = get_log_file(&log_name).context("Failed to create log file")?;

    let mut cmd = Command::new(&config.exe_path);
    cmd.args(&config.args)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));

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

    // Wait for process to fully terminate to avoid port conflicts
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    start_service(state, id).await?;
    Ok(())
}

pub async fn is_service_running(state: &AppState, id: usize) -> bool {
    // First check if process exists with a read lock
    let has_process = {
        let processes = state.service_processes.read().await;
        processes.contains_key(&id)
    };

    if !has_process {
        return false;
    }

    // Only acquire write lock if we need to check/clean up
    let mut processes = state.service_processes.write().await;
    if let Some(child) = processes.get_mut(&id) {
        // try_wait() is non-blocking and fast
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

    let stdout_file = get_log_file("web_tunnel").context("Failed to create log file")?;
    let stderr_file = get_log_file("web_tunnel").context("Failed to create log file")?;

    let mut cmd = Command::new(&config.frpc_exe_path);
    cmd.args(&config.frpc_args)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));

    let child = cmd.spawn().context("Failed to spawn Web Tunnel")?;

    {
        let mut tunnel = state.web_tunnel_process.lock().await;
        *tunnel = Some(child);
    }

    Ok(())
}
