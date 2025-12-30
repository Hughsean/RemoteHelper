use crate::state::AppState;
use base64::prelude::*;
use common::{Request, Response, ServiceAction, ServiceData, StatusData};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::server::TlsStream;
use tracing::{error, info, warn};

pub async fn handle_connection(mut socket: TlsStream<TcpStream>, state: AppState) {
    let mut buf = [0; 5 * 1024]; // 5 KB buffer
    let mut authenticated = false;
    let mut current_challenge: Option<String> = None;

    loop {
        // Read length prefix (4 bytes, big endian)
        let len = match socket.read_u32().await {
            Ok(n) => n as usize,
            Err(_) => return, // Connection closed or error
        };

        if len > buf.len() {
            error!("Request too large: {} bytes", len);
            return;
        }

        // Read body
        if let Err(e) = socket.read_exact(&mut buf[..len]).await {
            error!("Failed to read body: {}", e);
            return;
        }

        // Deserialize request
        let req: Request = match serde_json::from_slice(&buf[..len]) {
            Ok(r) => r,
            Err(e) => {
                error!("Invalid JSON: {}", e);
                let err_resp = Response::Error(format!("Invalid JSON request: {}", e));
                if let Ok(bytes) = serde_json::to_vec(&err_resp) {
                    let _ = socket.write_u32(bytes.len() as u32).await;
                    let _ = socket.write_all(&bytes).await;
                }
                continue;
            }
        };

        // Process request
        let response = match req {
            Request::GetChallenge => {
                let nonce: [u8; 32] = rand::thread_rng().r#gen();
                let nonce_str = BASE64_STANDARD.encode(nonce);
                current_challenge = Some(nonce_str.clone());
                Response::Challenge(nonce_str)
            }
            Request::Login {
                public_key,
                signature,
            } => {
                if let Some(challenge) = &current_challenge {
                    if verify_login(&state, &public_key, &signature, challenge) {
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

        // Serialize response
        let resp_bytes = match serde_json::to_vec(&response) {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to serialize response: {}", e);
                continue;
            }
        };

        // Write length prefix
        if let Err(e) = socket.write_u32(resp_bytes.len() as u32).await {
            error!("Failed to write length prefix: {}", e);
            return;
        }

        // Write body
        if let Err(e) = socket.write_all(&resp_bytes).await {
            error!("Failed to write body: {}", e);
            return;
        }
    }
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
            // Update refresh interval if provided
            if let Some(ms) = interval_ms {
                if ms >= 100 {
                    let mut lock = state.refresh_interval.write().await;
                    if *lock != ms {
                        *lock = ms;
                        state.update_notify.notify_one();
                    }
                }
            }

            let (cpu, mem, total, uptime, cpu_model) = {
                let sys = state.sys.read().await;
                let cpu_model = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_default();
                (
                    sys.global_cpu_usage(),
                    sys.used_memory(),
                    sys.total_memory(),
                    sysinfo::System::uptime(),
                    cpu_model,
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

            Response::Status(StatusData {
                cpu_usage: cpu,
                memory_usage: mem,
                total_memory: total,
                uptime,
                gpu_usage,
                gpu_memory_usage,
                gpu_total_memory,
                cpu_model,
                gpu_model,
            })
        }
        Request::ListServices => {
            let mut services = Vec::new();
            let processes = state.service_processes.read().await;

            // Static services
            for (id, svc_config) in state.config.service.iter().enumerate() {
                let running = processes.contains_key(&id);
                let pid = processes.get(&id).map(|c| c.id()).flatten();

                services.push(ServiceData {
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
                let pid = processes.get(&id).map(|c| c.id()).flatten();

                services.push(ServiceData {
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
            let id = state.config.service.len() + dynamic.len();
            dynamic.push(crate::config::ServiceConfig {
                description,
                exe_path,
                args,
                auto_start: false,
                allow_web_control: true,
            });
            Response::ServiceAdded(id)
        }
        _ => Response::Error("Invalid request state".to_string()),
    }
}
