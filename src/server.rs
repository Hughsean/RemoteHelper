use crate::state::AppState;
use anyhow::Result;
use base64::prelude::*;
use common::{
    Handshake, Request, Response, ServiceAction, ServiceInfo, SystemInfo, crypto::CryptoSession,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand::Rng;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use x25519_dalek::PublicKey;

pub async fn handle_connection(socket: TcpStream, state: AppState) {
    let result = tokio::time::timeout(
        Duration::from_secs(state.config.web_panel.connection_timeout_secs),
        handle_connection_inner(socket, state.clone()),
    )
    .await;

    // Decrement connection counter
    state.active_connections.fetch_sub(1, Ordering::Relaxed);

    match result {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => error!("Connection error: {}", e),
        Err(_) => error!("Connection timeout"),
    }
}

async fn handle_connection_inner(mut socket: TcpStream, state: AppState) -> Result<()> {
    let peer_addr = socket.peer_addr().ok();
    tracing::info!("正在处理来自 {:?} 的连接", peer_addr);

    // Increment connection counter when actually starting to handle connection
    state.active_connections.fetch_add(1, Ordering::Relaxed);

    // --- Handshake Phase ---
    let mut buf = [0u8; 1024];

    // 1. Read ClientHello
    let len = socket.read_u32().await? as usize;
    if len > buf.len() {
        return Err(anyhow::anyhow!("Handshake message too large"));
    }
    socket.read_exact(&mut buf[..len]).await?;

    let client_hello: Handshake = serde_json::from_slice(&buf[..len])?;
    let client_pub_bytes = match client_hello {
        Handshake::ClientHello { public_key } => BASE64_STANDARD.decode(public_key)?,
        _ => return Err(anyhow::anyhow!("Expected ClientHello")),
    };

    // 2. Generate Server Key & Shared Secret
    let (secret, server_public) = common::crypto::generate_ephemeral();
    let server_pub_b64 = BASE64_STANDARD.encode(server_public.as_bytes());

    // 3. Send ServerHello
    let resp = Handshake::ServerHello {
        public_key: server_pub_b64,
    };
    let resp_bytes = serde_json::to_vec(&resp)?;
    socket.write_u32(resp_bytes.len() as u32).await?;
    socket.write_all(&resp_bytes).await?;

    // 4. Initialize Crypto Session
    let client_pub_array: [u8; 32] = client_pub_bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid client public key length"))?;
    let client_public_key = PublicKey::from(client_pub_array);
    let shared_secret = secret.diffie_hellman(&client_public_key);
    let crypto = CryptoSession::new(shared_secret.to_bytes(), true);

    tracing::info!("与 {:?} 建立加密会话", peer_addr);

    // --- Split socket into reader and writer ---
    let (socket_read, socket_write) = socket.into_split();

    // Wrap writer in Arc<Mutex> for shared access
    let socket_write = std::sync::Arc::new(tokio::sync::Mutex::new(socket_write));
    let crypto = std::sync::Arc::new(tokio::sync::Mutex::new(crypto));

    // --- Encrypted Loop with Async Request Processing ---
    let mut authenticated = false;
    let mut current_challenge: Option<(String, Instant)> = None;
    let mut read_buf = [0u8; 10 * 1024]; // 10 KB buffer for encrypted frames
    const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(30);

    // 创建响应发送队列
    let (resp_tx, mut resp_rx) = mpsc::channel::<Response>(32);

    // Spawn dedicated response sender task
    let socket_write_clone = socket_write.clone();
    let crypto_clone = crypto.clone();
    let resp_sender_handle = tokio::spawn(async move {
        while let Some(response) = resp_rx.recv().await {
            let mut sock = socket_write_clone.lock().await;
            let mut cry = crypto_clone.lock().await;
            if let Err(e) = send_response_inner(&mut *sock, &mut *cry, &response).await {
                error!("Failed to send response: {}", e);
                break;
            }
        }
    });

    // Main request reading loop
    let mut socket_read = socket_read;
    loop {
        // Read encrypted length
        let len = match socket_read.read_u32().await {
            Ok(n) => n as usize,
            Err(_) => break, // Connection closed
        };

        if len > read_buf.len() {
            error!("Request too large: {} bytes", len);
            break;
        }

        // Read encrypted body
        if socket_read.read_exact(&mut read_buf[..len]).await.is_err() {
            break;
        }

        // Decrypt
        let plaintext = {
            let mut cry = crypto.lock().await;
            match cry.decrypt(&read_buf[..len]) {
                Ok(pt) => pt,
                Err(e) => {
                    error!("Decryption failed: {}", e);
                    break;
                }
            }
        };

        // Deserialize request
        tracing::debug!("收到加密请求，大小: {} 字节", len);
        let req: Request = match serde_json::from_slice(&plaintext) {
            Ok(r) => r,
            Err(e) => {
                error!("Invalid JSON: {}", e);
                let err_resp = Response::Error(format!("无效的 JSON 请求: {}", e));
                if resp_tx.send(err_resp).await.is_err() {
                    break;
                }
                continue;
            }
        };

        // Process request - differentiate auth requests from business requests
        tracing::debug!(
            "Processing request type: {:?}",
            std::mem::discriminant(&req)
        );
        match req {
            Request::GetChallenge => {
                // Auth request - process immediately
                let nonce: [u8; 32] = rand::thread_rng().r#gen();
                let nonce_str = BASE64_STANDARD.encode(nonce);
                current_challenge = Some((nonce_str.clone(), Instant::now()));
                let response = Response::Challenge(nonce_str);
                if resp_tx.send(response).await.is_err() {
                    break;
                }
            }
            Request::Login {
                public_key,
                signature,
            } => {
                // Auth request - process immediately
                let response = if let Some((challenge, timestamp)) = &current_challenge {
                    if timestamp.elapsed() > CHALLENGE_TIMEOUT {
                        current_challenge = None;
                        Response::Error("挑战已过期".to_string())
                    } else if verify_login(&state, &public_key, &signature, challenge) {
                        authenticated = true;
                        current_challenge = None;
                        info!("Client authenticated successfully with key: {}", public_key);
                        Response::Ok
                    } else {
                        warn!("Authentication failed for key: {}", public_key);
                        Response::Error("认证失败".to_string())
                    }
                } else {
                    Response::Error("未请求挑战".to_string())
                };
                if resp_tx.send(response).await.is_err() {
                    break;
                }
            }
            _ => {
                // Business request - process asynchronously
                if !authenticated {
                    if resp_tx
                        .send(Response::Error("未授权".to_string()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

                // Spawn async task to process request without blocking main loop
                let state_clone = state.clone();
                let resp_tx_clone = resp_tx.clone();
                tokio::spawn(async move {
                    let response = process_authenticated_request(req, &state_clone).await;
                    let _ = resp_tx_clone.send(response).await;
                });
            }
        }
    }

    // Wait for response sender to finish
    drop(resp_tx);
    let _ = resp_sender_handle.await;

    Ok(())
}

#[allow(dead_code)]
async fn send_response(
    socket: &mut TcpStream,
    crypto: &mut CryptoSession,
    response: &Response,
) -> Result<()> {
    let resp_bytes = serde_json::to_vec(response)?;
    let ciphertext = crypto.encrypt(&resp_bytes)?;

    socket.write_u32(ciphertext.len() as u32).await?;
    socket.write_all(&ciphertext).await?;
    Ok(())
}

async fn send_response_inner(
    socket: &mut tokio::net::tcp::OwnedWriteHalf,
    crypto: &mut CryptoSession,
    response: &Response,
) -> Result<()> {
    let resp_bytes = serde_json::to_vec(response)?;
    let ciphertext = crypto.encrypt(&resp_bytes)?;

    socket.write_u32(ciphertext.len() as u32).await?;
    socket.write_all(&ciphertext).await?;
    Ok(())
}

fn verify_login(state: &AppState, pub_key_b64: &str, sig_b64: &str, challenge: &str) -> bool {
    tracing::debug!(
        "Verifying login for public key: {}...",
        &pub_key_b64.chars().take(16).collect::<String>()
    );

    // Check if key is authorized
    if !state
        .config
        .web_panel
        .authorized_keys
        .contains(&pub_key_b64.to_string())
    {
        tracing::warn!("未授权的公钥尝试登录");
        return false;
    }

    let pub_key_bytes = match BASE64_STANDARD.decode(pub_key_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let sig_bytes = match BASE64_STANDARD.decode(sig_b64) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let challenge_bytes = match BASE64_STANDARD.decode(challenge) {
        Ok(b) => b,
        Err(_) => return false,
    };

    let verifying_key =
        match VerifyingKey::from_bytes(pub_key_bytes.as_slice().try_into().unwrap_or(&[0; 32])) {
            Ok(k) => k,
            Err(_) => return false,
        };

    let signature = match Signature::from_slice(&sig_bytes) {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("无效的签名格式");
            return false;
        }
    };

    let result = verifying_key.verify(&challenge_bytes, &signature).is_ok();
    if result {
        tracing::info!("登录验证成功");
    } else {
        tracing::warn!("登录验证失败 - 签名无效");
    }
    result
}

async fn process_authenticated_request(req: Request, state: &AppState) -> Response {
    tracing::debug!("正在处理已认证请求");
    match req {
        Request::GetStatus { interval_ms } => {
            // Update last read time
            {
                let mut last_read = state.last_read_time.write().await;
                *last_read = std::time::Instant::now();
            }

            // Update refresh interval if provided
            if let Some(ms) = interval_ms
                && ms >= 100
            {
                let mut lock = state.refresh_interval.write().await;
                if *lock != ms {
                    *lock = ms;
                    state.update_notify.notify_one();
                }
            }

            let (cpu, mem, total, uptime, cpu_model, net_tx, net_rx, net_tx_spd, net_rx_spd) = {
                let sys = state.sys.read().await;
                let networks = state.networks.read().await;
                let cpu_model = sys
                    .cpus()
                    .first()
                    .map(|c| c.brand().to_string())
                    .unwrap_or_default();

                let mut tx = 0;
                let mut rx = 0;
                let mut tx_spd = 0;
                let mut rx_spd = 0;

                for (_name, data) in networks.iter() {
                    tx += data.total_transmitted();
                    rx += data.total_received();
                    tx_spd += data.transmitted();
                    rx_spd += data.received();
                }

                (
                    sys.global_cpu_usage(),
                    sys.used_memory(),
                    sys.total_memory(),
                    sysinfo::System::uptime(),
                    cpu_model,
                    tx,
                    rx,
                    tx_spd,
                    rx_spd,
                )
            };

            let (gpu_usage, gpu_memory_usage, gpu_total_memory, gpu_model) = {
                let cache = state.gpu_cache.read().await;
                (
                    cache.usage,
                    cache.memory_used,
                    cache.memory_total,
                    cache.model.clone(),
                )
            };

            Response::Status(SystemInfo {
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis() as u64,
                cpu_usage: cpu,
                memory_usage: mem,
                total_memory: total,
                uptime,
                gpu_usage,
                gpu_memory_usage,
                gpu_total_memory,
                cpu_model,
                gpu_model,
                network_tx_bytes: net_tx,
                network_rx_bytes: net_rx,
                network_tx_speed: net_tx_spd,
                network_rx_speed: net_rx_spd,
            })
        }
        Request::ListServices => {
            tracing::debug!("正在列出所有服务");
            let mut services = Vec::new();
            let processes = state.service_processes.read().await;

            // Static services
            for (id, svc_config) in state.config.service.iter().enumerate() {
                let running = processes.contains_key(&id);
                let pid = processes.get(&id).and_then(|handle| match handle {
                    crate::state::ProcessHandle::Child(c) => c.id(),
                    crate::state::ProcessHandle::Pid(p) => Some(*p),
                });

                services.push(ServiceInfo {
                    id,
                    description: svc_config.description.clone(),
                    running,
                    pid,
                    run_as_user: svc_config.run_as_user,
                });
            }

            // Dynamic services
            let dynamic = state.dynamic_services.read().await;
            let offset = state.config.service.len();
            for (i, svc_config) in dynamic.iter().enumerate() {
                let id = offset + i;
                let running = processes.contains_key(&id);
                let pid = processes.get(&id).and_then(|handle| match handle {
                    crate::state::ProcessHandle::Child(c) => c.id(),
                    crate::state::ProcessHandle::Pid(p) => Some(*p),
                });

                services.push(ServiceInfo {
                    id,
                    description: svc_config.description.clone(),
                    running,
                    pid,
                    run_as_user: svc_config.run_as_user,
                });
            }

            Response::Services(services)
        }
        Request::ControlService {
            id,
            action,
            user_name,
            user_password,
        } => {
            tracing::info!("服务控制请求 - ID: {}, 操作: {:?}", id, action);
            let static_count = state.config.service.len();
            let dynamic_count = state.dynamic_services.read().await.len();

            if id >= static_count + dynamic_count {
                return Response::Error("服务 ID 超出范围".to_string());
            }

            let allowed = if id < static_count {
                state.config.service[id].allow_web_control
            } else {
                true
            };

            if !allowed {
                return Response::Error("此服务的 Web 控制已禁用".to_string());
            }

            match action {
                ServiceAction::Start => {
                    match crate::process::start_service(state, id, user_name, user_password).await {
                        Ok(_) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    }
                }
                ServiceAction::Stop => match crate::process::stop_service(state, id).await {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                },
                ServiceAction::Restart => {
                    match crate::process::restart_service(state, id, user_name, user_password).await
                    {
                        Ok(_) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    }
                }
            }
        }
        Request::AddService {
            description,
            exe_path,
            args,
            run_as_user,
            user_name,
            user_password,
        } => {
            tracing::info!(
                "正在添加新服务: {} (可执行文件: {}, 用户模式: {})",
                description,
                exe_path,
                run_as_user
            );
            let mut dynamic = state.dynamic_services.write().await;
            dynamic.push(crate::config::ServiceConfig {
                description,
                exe_path,
                args,
                auto_start: false,
                allow_web_control: true,
                run_as_user,
                user_name,
                user_password,
            });

            // Return actual service ID: static count + new dynamic index
            let id = state.config.service.len() + dynamic.len() - 1;
            Response::ServiceAdded(id)
        }
        Request::QueryPath { path } => {
            tracing::debug!("路径查询请求: {}", path);
            query_path_suggestions(&path)
        }
        _ => Response::Error("无效的请求状态".to_string()),
    }
}

fn query_path_suggestions(path: &str) -> Response {
    use std::path::{Path, PathBuf};

    // 处理路径：如果是空的，使用当前目录
    let input_path = if path.is_empty() {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    } else {
        PathBuf::from(path)
    };

    // 确定要搜索的目录和前缀
    let (search_dir, prefix) = if input_path.exists() && input_path.is_dir() {
        // 如果是一个存在的目录，搜索该目录
        (input_path.clone(), String::new())
    } else {
        // 否则，搜索父目录，并使用文件名作为前缀
        let parent = input_path.parent().unwrap_or(Path::new("."));
        let file_name = input_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        (parent.to_path_buf(), file_name)
    };

    // 读取目录并过滤
    let mut suggestions = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&search_dir) {
        for entry in entries.flatten() {
            if let Ok(metadata) = entry.metadata() {
                let file_name = entry.file_name();
                let name_str = file_name.to_string_lossy();

                // 如果有前缀，进行过滤（不区分大小写）
                if !prefix.is_empty()
                    && !name_str.to_lowercase().starts_with(&prefix.to_lowercase())
                {
                    continue;
                }

                let is_dir = metadata.is_dir();
                let full_path = entry.path();
                let path_str = full_path.to_string_lossy().to_string();

                // 检查是否可执行（Windows下检查.exe扩展名）
                let is_executable = if cfg!(windows) {
                    path_str.to_lowercase().ends_with(".exe")
                } else {
                    // Unix系统检查执行权限
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode() & 0o111 != 0
                    }
                    #[cfg(not(unix))]
                    false
                };

                suggestions.push(common::PathItem {
                    path: path_str,
                    is_dir,
                    is_executable,
                });
            }
        }
    }

    // 按类型和名称排序：目录优先，然后是可执行文件，最后是其他文件
    suggestions.sort_by(|a, b| {
        use std::cmp::Ordering;
        match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => match (a.is_executable, b.is_executable) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => a.path.to_lowercase().cmp(&b.path.to_lowercase()),
            },
        }
    });

    // 限制返回数量，避免过多结果
    suggestions.truncate(50);

    Response::PathSuggestions(suggestions)
}
