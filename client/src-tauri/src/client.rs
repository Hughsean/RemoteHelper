use common::{Request, Response, ServiceAction};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "127.0.0.1:8088";

async fn send_request(req: Request) -> Result<Response, String> {
    let mut stream = TcpStream::connect(SERVER_ADDR)
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    // Serialize request
    let req_bytes = serde_json::to_vec(&req).map_err(|e| format!("Failed to serialize request: {}", e))?;

    // Write length prefix
    stream
        .write_u32(req_bytes.len() as u32)
        .await
        .map_err(|e| format!("Failed to write length prefix: {}", e))?;

    // Write body
    stream
        .write_all(&req_bytes)
        .await
        .map_err(|e| format!("Failed to write body: {}", e))?;

    // Read response length
    let len = stream
        .read_u32()
        .await
        .map_err(|e| format!("Failed to read response length: {}", e))? as usize;

    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    // Deserialize response
    let resp: Response = serde_json::from_slice(&buf).map_err(|e| format!("Failed to deserialize response: {}", e))?;

    Ok(resp)
}

#[tauri::command]
pub async fn authenticate(token: String) -> Result<String, String> {
    match send_request(Request::Auth { token }).await? {
        Response::Ok => Ok("Authenticated".to_string()),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}

#[tauri::command]
pub async fn get_status() -> Result<common::StatusData, String> {
    match send_request(Request::GetStatus).await? {
        Response::Status(data) => Ok(data),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}

#[tauri::command]
pub async fn list_services() -> Result<Vec<common::ServiceData>, String> {
    match send_request(Request::ListServices).await? {
        Response::Services(data) => Ok(data),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}

#[tauri::command]
pub async fn control_service(id: usize, action: String) -> Result<(), String> {
    let action_enum = match action.as_str() {
        "start" => ServiceAction::Start,
        "stop" => ServiceAction::Stop,
        "restart" => ServiceAction::Restart,
        _ => return Err("Invalid action".to_string()),
    };

    match send_request(Request::ControlService {
        id,
        action: action_enum,
    })
    .await?
    {
        Response::Ok => Ok(()),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}
