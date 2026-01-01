use crate::state::AppState;
use anyhow::Result;
use base64::prelude::*;
use common::func;
use common::{
    Handshake, Request, Response, ServiceAction, ServiceInfo, SystemInfo, crypto::CryptoSession,
};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand::Rng;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{error, info, warn};
use x25519_dalek::PublicKey;

pub async fn handle_connection(mut socket: TcpStream, state: AppState) {
    let result = tokio::time::timeout(
        Duration::from_secs(state.config.web_panel.connection_timeout_secs),
        handle_connection_inner(&mut socket, state.clone()),
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

async fn handle_connection_inner(socket: &mut TcpStream, state: AppState) -> Result<()> {
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
    let mut crypto = CryptoSession::new(shared_secret.to_bytes(), true);

    info!("Encrypted session established");

    // --- Encrypted Loop ---
    let mut authenticated = false;
    let mut current_challenge: Option<(String, Instant)> = None;
    let mut read_buf = [0u8; 10 * 1024]; // 10 KB buffer for encrypted frames
    const CHALLENGE_TIMEOUT: Duration = Duration::from_secs(30);

    while let Ok(n) = socket.read_u32().await {
        // Read encrypted length (4 bytes)
        // Note: This length is the length of the CIPHERTEXT (including tag)
        let len = n as usize;

        if len > read_buf.len() {
            return Err(anyhow::anyhow!("Request too large: {} bytes", len));
        }

        // Read encrypted body
        socket.read_exact(&mut read_buf[..len]).await?;

        // Decrypt
        let plaintext = match crypto.decrypt(&read_buf[..len]) {
            Ok(pt) => pt,
            Err(e) => {
                error!("Decryption failed: {}", e);
                return Err(e);
            }
        };

        // Deserialize request
        let req: Request = match serde_json::from_slice(&plaintext) {
            Ok(r) => r,
            Err(e) => {
                error!("Invalid JSON: {}", e);
                // We can try to send an encrypted error response, but if JSON is bad, maybe just close?
                // Let's try to send error.
                let err_resp = Response::Error(format!("Invalid JSON request: {}", e));
                send_response(socket, &mut crypto, &err_resp).await?;
                continue;
            }
        };

        // Process request
        let response = match req {
            Request::GetChallenge => {
                let nonce: [u8; 32] = rand::thread_rng().r#gen();
                let nonce_str = BASE64_STANDARD.encode(nonce);
                current_challenge = Some((nonce_str.clone(), Instant::now()));
                Response::Challenge(nonce_str)
            }
            Request::Login {
                public_key,
                signature,
            } => {
                if let Some((challenge, timestamp)) = &current_challenge {
                    // Check if challenge has expired
                    if timestamp.elapsed() > CHALLENGE_TIMEOUT {
                        current_challenge = None;
                        Response::Error("Challenge expired".to_string())
                    } else if verify_login(&state, &public_key, &signature, challenge) {
                        authenticated = true;
                        current_challenge = None; // Clear challenge after use
                        info!("Client authenticated successfully with key: {}", public_key);
                        Response::Ok
                    } else {
                        warn!("Authentication failed for key: {}", public_key);
                        Response::Error("Authentication failed".to_string())
                    }
                } else {
                    Response::Error("No challenge requested".to_string())
                }
            }
            _ => {
                if authenticated {
                    process_authenticated_request(req, &state).await
                } else {
                    Response::Error("Unauthorized".to_string())
                }
            }
        };

        send_response(socket, &mut crypto, &response).await?;
    }
    Ok(())
}

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

fn verify_login(state: &AppState, pub_key_b64: &str, sig_b64: &str, challenge: &str) -> bool {
    // Check if key is authorized
    if !state
        .config
        .web_panel
        .authorized_keys
        .contains(&pub_key_b64.to_string())
    {
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
        Err(_) => return false,
    };

    verifying_key.verify(&challenge_bytes, &signature).is_ok()
}

async fn process_authenticated_request(req: Request, state: &AppState) -> Response {
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
                nanoid: func::nanoid_gen(),
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
            let mut services = Vec::new();
            let processes = state.service_processes.read().await;

            // Static services
            for (id, svc_config) in state.config.service.iter().enumerate() {
                let running = processes.contains_key(&id);
                let pid = processes.get(&id).and_then(|c| c.id());

                services.push(ServiceInfo {
                    nanoid: func::nanoid_gen(),
                    id,
                    description: svc_config.description.clone(),
                    running,
                    pid,
                });
            }

            // Dynamic services
            let dynamic = state.dynamic_services.read().await;
            let offset = state.config.service.len();
            for (i, svc_config) in dynamic.iter().enumerate() {
                let id = offset + i;
                let running = processes.contains_key(&id);
                let pid = processes.get(&id).and_then(|c| c.id());

                services.push(ServiceInfo {
                    nanoid: func::nanoid_gen(),
                    id,
                    description: svc_config.description.clone(),
                    running,
                    pid,
                });
            }

            Response::Services(services)
        }
        Request::ControlService { id, action } => {
            let static_count = state.config.service.len();
            let dynamic_count = state.dynamic_services.read().await.len();

            if id >= static_count + dynamic_count {
                return Response::Error("Service ID out of range".to_string());
            }

            let allowed = if id < static_count {
                state.config.service[id].allow_web_control
            } else {
                true
            };

            if !allowed {
                return Response::Error("Web control is disabled for this service".to_string());
            }

            match action {
                ServiceAction::Start => match crate::process::start_service(state, id).await {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                },
                ServiceAction::Stop => match crate::process::stop_service(state, id).await {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                },
                ServiceAction::Restart => match crate::process::restart_service(state, id).await {
                    Ok(_) => Response::Ok,
                    Err(e) => Response::Error(e.to_string()),
                },
            }
        }
        Request::AddService {
            description,
            exe_path,
            args,
        } => {
            let mut dynamic = state.dynamic_services.write().await;
            dynamic.push(crate::config::ServiceConfig {
                description,
                exe_path,
                args,
                auto_start: false,
                allow_web_control: true,
            });

            // Return actual service ID: static count + new dynamic index
            let id = state.config.service.len() + dynamic.len() - 1;
            Response::ServiceAdded(id)
        }
        _ => Response::Error("Invalid request state".to_string()),
    }
}
