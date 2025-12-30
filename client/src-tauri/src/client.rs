use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::prelude::*;
use common::{Request, Response, ServiceAction};
use ed25519_dalek::{Signer, SigningKey};
use hmac::Hmac;
use pbkdf2::pbkdf2;
use serde::Deserialize;
use sha2::Sha256;
use std::sync::LazyLock;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const SERVER_ADDR: &str = "frp-egg.com:28450";

// Global state to hold the decrypted private key
static SIGNING_KEY: LazyLock<Mutex<Option<(SigningKey, String)>>> =
    LazyLock::new(|| Mutex::new(None));

#[derive(Deserialize)]
struct KeyFile {
    pub_key: String,
    enc_priv_key: String,
    salt: String,
    nonce: String,
}

async fn send_request(req: Request) -> Result<Response, String> {
    // 1. Check if we are authenticated (have a key)
    let (signing_key, pub_key) = {
        let guard = SIGNING_KEY
            .lock()
            .map_err(|_| "Failed to lock key store".to_string())?;
        match &*guard {
            Some((k, p)) => (k.clone(), p.clone()),
            None => return Err("Not authenticated. Please login first.".to_string()),
        }
    };

    // 2. Connect
    let mut stream = TcpStream::connect(SERVER_ADDR)
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    // 3. Perform Login Handshake
    // 3.1 Get Challenge
    send_raw_request(&mut stream, Request::GetChallenge).await?;
    let resp = read_raw_response(&mut stream).await?;
    let challenge = match resp {
        Response::Challenge(c) => c,
        Response::Error(e) => return Err(format!("Server error during handshake: {}", e)),
        _ => return Err("Unexpected response during handshake".to_string()),
    };

    // 3.2 Sign Challenge
    let challenge_bytes = BASE64_STANDARD
        .decode(&challenge)
        .map_err(|e| format!("Invalid challenge format: {}", e))?;
    let signature = signing_key.sign(&challenge_bytes);
    let signature_str = BASE64_STANDARD.encode(signature.to_bytes());

    // 3.3 Send Login
    send_raw_request(
        &mut stream,
        Request::Login {
            public_key: pub_key,
            signature: signature_str,
        },
    )
    .await?;

    let resp = read_raw_response(&mut stream).await?;
    match resp {
        Response::Ok => {} // Login success
        Response::Error(e) => return Err(format!("Login failed: {}", e)),
        _ => return Err("Unexpected response during login".to_string()),
    }

    // 4. Send Actual Request
    send_raw_request(&mut stream, req).await?;
    read_raw_response(&mut stream).await
}

async fn send_raw_request(stream: &mut TcpStream, req: Request) -> Result<(), String> {
    let req_bytes =
        serde_json::to_vec(&req).map_err(|e| format!("Failed to serialize request: {}", e))?;
    stream
        .write_u32(req_bytes.len() as u32)
        .await
        .map_err(|e| format!("Failed to write length: {}", e))?;
    stream
        .write_all(&req_bytes)
        .await
        .map_err(|e| format!("Failed to write body: {}", e))?;
    Ok(())
}

async fn read_raw_response(stream: &mut TcpStream) -> Result<Response, String> {
    let len = stream
        .read_u32()
        .await
        .map_err(|e| format!("Failed to read length: {}", e))? as usize;
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;
    serde_json::from_slice(&buf).map_err(|e| format!("Failed to deserialize: {}", e))
}

#[tauri::command]
pub async fn authenticate(passphrase: String) -> Result<String, String> {
    // Hardcoded key data
    let key_file = KeyFile {
        pub_key: "cD4nEfSeBQF+aWZZzLusNAcUthuq2uw4kZRlCPhkgkQ=".to_string(),
        enc_priv_key: "IGvKSCMIOyF0b4fkLwf9rDDUgsPv/rZ/QOJPIU/YKKVRd+AznMwBWCwU/LsSYC9R"
            .to_string(),
        salt: "bLkBsCBwWIwjsWLuKM5fEg==".to_string(),
        nonce: "lBjdmc2vFXvVWB31".to_string(),
    };

    // Decrypt Key
    let salt = BASE64_STANDARD
        .decode(&key_file.salt)
        .map_err(|e| format!("Invalid salt: {}", e))?;
    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.trim().as_bytes(), &salt, 100_000, &mut key)
        .expect("HMAC can be initialized with any key length");

    let cipher = Aes256Gcm::new(&key.into());
    let nonce_bytes = BASE64_STANDARD
        .decode(&key_file.nonce)
        .map_err(|e| format!("Invalid nonce: {}", e))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let enc_bytes = BASE64_STANDARD
        .decode(&key_file.enc_priv_key)
        .map_err(|e| format!("Invalid encrypted key: {}", e))?;

    let priv_key_bytes = cipher
        .decrypt(nonce, enc_bytes.as_ref())
        .map_err(|_| "Invalid passphrase or corrupted key file".to_string())?;

    let signing_key = SigningKey::from_bytes(priv_key_bytes.as_slice().try_into().unwrap());

    // Store in global state
    {
        let mut guard = SIGNING_KEY
            .lock()
            .map_err(|_| "Failed to lock key store".to_string())?;
        *guard = Some((signing_key, key_file.pub_key));
    }

    Ok("Authenticated successfully".to_string())
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
