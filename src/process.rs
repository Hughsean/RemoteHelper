use crate::state::{AppState, ProcessHandle};
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
        tracing::info!(
            "轮转日志文件 {} -> {} (大小: {} MB)",
            log_path,
            backup_path,
            metadata.len() / (1024 * 1024)
        );
        let _ = fs::rename(&log_path, backup_path);
    }

    let file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    Ok(file)
}

pub async fn start_service(
    state: &AppState,
    id: usize,
    override_user_name: Option<String>,
    override_user_password: Option<String>,
) -> Result<()> {
    let static_count = state.config.service.len();
    let config = if id < static_count {
        state.config.service[id].clone()
    } else {
        let dynamic = state.dynamic_services.read().await;
        dynamic
            .get(id - static_count)
            .cloned()
            .context("服务 ID 未找到")?
    };

    if is_service_running(state, id).await {
        tracing::info!("服务 ID {} ({}) 已在运行，跳过启动", id, config.description);
        return Ok(()); // Already running
    }

    tracing::info!(
        "正在启动服务 ID {}: {} (用户模式: {}, 自动启动: {})",
        id,
        config.description,
        config.run_as_user,
        config.auto_start
    );
    tracing::debug!(
        "服务配置 - 可执行文件: {}, 参数: {:?}",
        config.exe_path,
        config.args
    );

    let log_name = format!("service_id({})_{}", id, config.description);
    // We need separate handles for stdout and stderr because Stdio::from consumes the file
    let stdout_file = get_log_file(&log_name).context("无法创建日志文件")?;
    let stderr_file = get_log_file(&log_name).context("无法创建日志文件")?;

    // 根据 run_as_user 选择启动方式
    if config.run_as_user {
        // 用户模式必须提供凭据：优先使用临时凭据，否则使用配置中的凭据
        let (username, password) = match (
            override_user_name.or(config.user_name.clone()),
            override_user_password.or(config.user_password.clone()),
        ) {
            (Some(u), Some(p)) => (u, p),
            _ => {
                return Err(anyhow::anyhow!(
                    "服务 '{}' 需要用户模式但未提供凭据",
                    config.description
                ));
            }
        };

        tracing::info!("以用户身份启动: {} (使用凭据)", username);

        // 构造参数字符串
        let args_str = if config.args.is_empty() {
            String::new()
        } else {
            config
                .args
                .iter()
                .map(|a| format!("'{}'", a.replace("'", "''")))
                .collect::<Vec<_>>()
                .join(",")
        };

        // 转义路径中的单引号
        let exe_escaped = config.exe_path.replace("'", "''");

        // 提取可执行文件名（用于后续进程查找）
        let exe_name = std::path::Path::new(&config.exe_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // 构造 PowerShell 命令，启动进程但不依赖返回的 PID
        let ps_command = if args_str.is_empty() {
            format!(
                "$password = ConvertTo-SecureString '{}' -AsPlainText -Force; \
                 $credential = New-Object System.Management.Automation.PSCredential('{}', $password); \
                 Start-Process -FilePath '{}' -Credential $credential -WindowStyle Hidden",
                password.replace("'", "''"),
                username.replace("'", "''"),
                exe_escaped
            )
        } else {
            format!(
                "$password = ConvertTo-SecureString '{}' -AsPlainText -Force; \
                 $credential = New-Object System.Management.Automation.PSCredential('{}', $password); \
                 Start-Process -FilePath '{}' -ArgumentList {} -Credential $credential -WindowStyle Hidden",
                password.replace("'", "''"),
                username.replace("'", "''"),
                exe_escaped,
                args_str
            )
        };

        tracing::info!("用户模式: 使用凭据的 PowerShell Start-Process");

        // 执行 PowerShell 命令启动进程
        let mut cmd = Command::new("powershell");
        cmd.args(&["-NoProfile", "-Command", &ps_command])
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));

        cmd.spawn()
            .context(format!("无法生成用户模式进程: {}", config.description))?;

        tracing::info!("用户模式启动器进程已成功生成: {}", config.description);

        // 等待进程启动（给程序时间完成启动和可能的 fork）
        tracing::debug!("等待 1.5 秒让进程稳定...");
        tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

        // 通过完整路径查找实际运行的进程 PID（更精确，避免同名进程混淆）
        tracing::info!("正在按路径查找进程: {}", config.exe_path);

        let find_cmd = format!(
            "Get-Process | Where-Object {{$_.Path -eq '{}'}} | Select-Object -First 1 -ExpandProperty Id",
            config.exe_path.replace("'", "''").replace("\\", "\\\\")
        );

        let find_output = Command::new("powershell")
            .args(&["-NoProfile", "-Command", &find_cmd])
            .output()
            .await
            .context("无法按路径查找进程")?;

        let pid_str = String::from_utf8(find_output.stdout)
            .context("无法解码进程 ID")?
            .trim()
            .to_string();

        if pid_str.is_empty() {
            // 如果按路径没找到，回退到按进程名查找
            tracing::warn!("按路径未找到进程，尝试按名称查找: {}", exe_name);

            let fallback_cmd = format!(
                "Get-Process -Name '{}' -ErrorAction SilentlyContinue | Select-Object -First 1 -ExpandProperty Id",
                exe_name.replace("'", "''")
            );

            let fallback_output = Command::new("powershell")
                .args(&["-NoProfile", "-Command", &fallback_cmd])
                .output()
                .await
                .context("无法按名称查找进程")?;

            let fallback_pid_str = String::from_utf8(fallback_output.stdout)
                .context("无法解码进程 ID")?
                .trim()
                .to_string();

            if fallback_pid_str.is_empty() {
                return Err(anyhow::anyhow!(
                    "未找到运行中的进程: {} (已尝试路径和名称)",
                    config.description
                ));
            }

            let pid: u32 = fallback_pid_str
                .parse()
                .context(format!("无法解析进程 ID: {}", fallback_pid_str))?;

            tracing::info!(
                "启动用户模式进程 {} ，PID: {} (按名称查找)",
                config.description,
                pid
            );

            {
                let mut processes = state.service_processes.write().await;
                processes.insert(id, ProcessHandle::Pid(pid));
            }

            return Ok(());
        }

        let pid: u32 = pid_str
            .parse()
            .context(format!("无法解析进程 ID: {}", pid_str))?;

        tracing::info!(
            "启动用户模式进程 {} ，PID: {} (按路径查找)",
            config.description,
            pid
        );

        // 将 PID 存储到进程表
        {
            let mut processes = state.service_processes.write().await;
            processes.insert(id, ProcessHandle::Pid(pid));
        }

        // 直接返回，不需要 Child 句柄
        return Ok(());
    } else {
        // 系统服务模式：直接启动
        tracing::info!("以系统服务模式启动 (直接生成)");
        let mut cmd = Command::new(&config.exe_path);
        cmd.args(&config.args)
            .stdout(Stdio::from(stdout_file))
            .stderr(Stdio::from(stderr_file));

        let child = cmd
            .spawn()
            .context(format!("无法生成系统服务: {}", config.description))?;

        let pid = child.id();
        tracing::info!("系统服务已生成，PID: {:?}", pid);

        // 将 Child 句柄存储到进程表
        {
            let mut processes = state.service_processes.write().await;
            processes.insert(id, ProcessHandle::Child(child));
        }
    }

    tracing::info!("服务 ID {} ({}) 启动成功", id, config.description);

    Ok(())
}

pub async fn stop_service(state: &AppState, id: usize) -> Result<()> {
    tracing::info!("正在尝试停止服务 ID: {}", id);

    let handle_opt = {
        let mut processes = state.service_processes.write().await;
        processes.remove(&id)
    };

    if let Some(handle) = handle_opt {
        tracing::info!("找到服务 ID {} 的运行中进程，正在终止...", id);

        match handle {
            ProcessHandle::Child(mut child) => {
                let pid = child.id();
                tracing::debug!("正在终止 Child 进程，PID: {:?}", pid);
                // 系统模式：使用 Child 句柄 kill
                if let Ok(Some(status)) = child.try_wait() {
                    tracing::info!("服务 ID {} 已终止，状态: {:?}", id, status);
                    return Ok(());
                }

                child.kill().await.context("无法终止进程")?;
                tracing::info!("已发送终止信号到 Child 进程 PID: {:?}", pid);

                let wait_result =
                    tokio::time::timeout(tokio::time::Duration::from_secs(5), child.wait()).await;

                match wait_result {
                    Ok(Ok(status)) => {
                        tracing::info!("服务 ID {} 退出，状态: {:?}", id, status)
                    }
                    Ok(Err(e)) => tracing::warn!("等待服务 ID {} 时出错: {}", id, e),
                    Err(_) => tracing::warn!("等待服务 ID {} 退出超时", id),
                }
            }
            ProcessHandle::Pid(pid) => {
                // 用户模式：先尝试优雅关闭，失败后强制终止
                tracing::info!("正在优雅关闭用户模式进程，PID: {}", pid);

                // 第一步：不带 /F 的 taskkill（发送 WM_CLOSE 或 CTRL_C_EVENT）
                let gentle_result = Command::new("taskkill")
                    .args(&["/PID", &pid.to_string()])
                    .output()
                    .await
                    .context("无法执行 taskkill")?;

                if gentle_result.status.success() {
                    tracing::info!("已发送优雅关闭信号到进程 {}", pid);

                    // 等待最多 5 秒让进程优雅退出
                    for i in 0..10 {
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

                        // 检查进程是否已退出
                        let check = Command::new("tasklist")
                            .args(&["/FI", &format!("PID eq {}", pid), "/NH"])
                            .output()
                            .await;

                        if let Ok(output) = check {
                            let stdout = String::from_utf8_lossy(&output.stdout);
                            if !stdout.contains(&pid.to_string()) {
                                tracing::info!("进程 {} 在 {} 毫秒后优雅退出", pid, (i + 1) * 500);
                                return Ok(());
                            }
                        }
                    }

                    tracing::warn!("进程 {} 在 5 秒内未优雅退出，强制终止", pid);
                }

                // 第二步：进程未响应优雅关闭，强制终止
                tracing::info!("强制终止进程 {}", pid);
                let force_result = Command::new("taskkill")
                    .args(&["/F", "/PID", &pid.to_string()])
                    .output()
                    .await
                    .context("无法执行 taskkill /F")?;

                if !force_result.status.success() {
                    let stderr = String::from_utf8_lossy(&force_result.stderr);
                    tracing::warn!("强制终止失败: {}", stderr);
                    return Err(anyhow::anyhow!("无法强制终止进程 {}: {}", pid, stderr));
                }

                tracing::info!("成功强制终止进程 {}", pid);
            }
        }
    }

    Ok(())
}

pub async fn restart_service(
    state: &AppState,
    id: usize,
    override_user_name: Option<String>,
    override_user_password: Option<String>,
) -> Result<()> {
    tracing::info!("正在重启服务 ID: {}", id);

    stop_service(state, id).await?;
    tracing::debug!("服务已停止，等待 500ms 后重启...");

    // Wait for process to fully terminate to avoid port conflicts
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    start_service(state, id, override_user_name, override_user_password).await?;
    tracing::info!("服务 ID {} 重启成功", id);
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
    if let Some(handle) = processes.get_mut(&id) {
        match handle {
            ProcessHandle::Child(child) => {
                // 系统模式：使用 try_wait 检查
                match child.try_wait() {
                    Ok(None) => true,
                    Ok(Some(_)) => {
                        // Process exited, remove it from map (cleanup)
                        processes.remove(&id);
                        false
                    }
                    Err(_) => false,
                }
            }
            ProcessHandle::Pid(pid) => {
                // 用户模式：使用 tasklist 检查进程是否存在
                let output = std::process::Command::new("tasklist")
                    .args(&["/FI", &format!("PID eq {}", pid), "/NH"])
                    .output();

                match output {
                    Ok(output) => {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        // 如果输出包含 PID，说明进程还在运行
                        let running = stdout.contains(&pid.to_string());
                        if !running {
                            processes.remove(&id);
                        }
                        running
                    }
                    Err(_) => {
                        // 无法检查，假定已停止
                        processes.remove(&id);
                        false
                    }
                }
            }
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
    tracing::info!("正在启动 Web 面板隧道...");
    tracing::debug!(
        "Web 隧道可执行文件: {}, 参数: {:?}",
        config.frpc_exe_path,
        config.frpc_args
    );

    let stdout_file = get_log_file("web_tunnel").context("无法创建日志文件")?;
    let stderr_file = get_log_file("web_tunnel").context("无法创建日志文件")?;

    let mut cmd = Command::new(&config.frpc_exe_path);
    cmd.args(&config.frpc_args)
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));

    let child = cmd.spawn().context("无法生成 Web 隧道")?;
    let pid = child.id();
    tracing::info!("Web 隧道已生成，PID: {:?}", pid);

    {
        let mut tunnel = state.web_tunnel_process.lock().await;
        *tunnel = Some(child);
    }

    tracing::info!("Web 隧道进程注册成功");
    Ok(())
}
