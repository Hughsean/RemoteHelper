use crate::state::AppState;
use common::{Request, Response, ServiceAction, ServiceData, StatusData};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{error, info};

pub async fn handle_connection(mut socket: TcpStream, state: AppState) {
    let mut buf = [0;   100 * 1024]; // 100 KB buffer

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
                continue;
            }
        };

        // Process request
        let response = process_request(req, &state).await;

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

async fn process_request(req: Request, state: &AppState) -> Response {
    match req {
        Request::Auth { token } => {
            if token == state.config.web_panel.auth_secret {
                Response::Ok
            } else {
                Response::Error("Invalid token".to_string())
            }
        }
        Request::GetStatus => {
            let (cpu, mem, total, uptime) = {
                let sys = state.sys.lock().unwrap();
                (
                    sys.global_cpu_usage(),
                    sys.used_memory(),
                    sys.total_memory(),
                    sysinfo::System::uptime(),
                )
            };
            Response::Status(StatusData {
                cpu_usage: cpu,
                memory_usage: mem,
                total_memory: total,
                uptime,
            })
        }
        Request::ListServices => {
            let mut services = Vec::new();
            let processes = state.service_processes.lock().unwrap();
            
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
            Response::Services(services)
        }
        Request::ControlService { id, action } => {
            if id >= state.config.service.len() {
                return Response::Error("Service ID out of range".to_string());
            }

            match action {
                ServiceAction::Start => {
                    match crate::process::start_service(state, id).await {
                        Ok(_) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    }
                }
                ServiceAction::Stop => {
                    match crate::process::stop_service(state, id).await {
                        Ok(_) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    }
                }
                ServiceAction::Restart => {
                    let _ = crate::process::stop_service(state, id).await;
                    match crate::process::start_service(state, id).await {
                        Ok(_) => Response::Ok,
                        Err(e) => Response::Error(e.to_string()),
                    }
                }
            }
        }
    }
}
