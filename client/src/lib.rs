use base64::prelude::*;
use common::{Handshake, Request, Response, SystemInfo, crypto::CryptoSession};
use ed25519_dalek::{Signer, SigningKey};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::PublicKey;

pub use common::func::KeyFile;

// Global state to hold the decrypted private key
pub static SIGNING_KEY: LazyLock<Mutex<Option<(SigningKey, String)>>> =
    LazyLock::new(|| Mutex::new(None));

pub static SERVER_ADDRESS: LazyLock<Mutex<String>> =
    LazyLock::new(|| Mutex::new("frp-egg.com:28450".to_string()));

pub struct EncryptedConnection {
    stream: TcpStream,
    crypto: CryptoSession,
}

pub static CONNECTION: LazyLock<tokio::sync::Mutex<Option<EncryptedConnection>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

pub static MOCK_MODE: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

fn generate_mock_system_info() -> SystemInfo {
    let start = SystemTime::now();
    let elapsed = start
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();

    let t = elapsed;
    let cpu = 25.0 + 20.0 * (t / 3.0).sin() + 15.0 * (t / 1.7).sin();
    let mem_ratio = 0.55 + 0.15 * (t / 8.0).sin();
    let gpu = 35.0 + 25.0 * (t / 5.0).sin() + 20.0 * (t / 2.3).sin();
    let gpu_mem_ratio = 0.5 + 0.25 * (t / 7.0).sin();

    let total_mem: u64 = 16 * 1024 * 1024 * 1024;
    let gpu_total_mem: u64 = 8 * 1024 * 1024 * 1024;

    SystemInfo {
        timestamp: std::time::UNIX_EPOCH
            .elapsed()
            .unwrap_or_default()
            .as_millis() as u64,
        cpu_usage: cpu.clamp(0.0, 100.0) as f32,
        memory_usage: (total_mem as f64 * mem_ratio.clamp(0.1, 0.95)) as u64,
        total_memory: total_mem,
        uptime: (t / 3600.0) as u64,
        gpu_usage: Some(gpu.clamp(0.0, 100.0) as u32),
        gpu_memory_usage: Some((gpu_total_mem as f64 * gpu_mem_ratio.clamp(0.1, 0.95)) as u64),
        gpu_total_memory: Some(gpu_total_mem),
        cpu_model: "Mock CPU @ 3.50GHz (Debug Mode)".to_string(),
        gpu_model: Some("Mock GPU (Debug Mode)".to_string()),
        gpu_temperature: Some((55.0 + 20.0 * (t / 8.0).sin()) as f32),
        gpu_power_watts: Some((80.0 + 40.0 * (t / 6.0).sin()) as f32),
        cpu_temperature: Some((45.0 + 15.0 * (t / 10.0).sin()) as f32),
        cpu_package_power: Some((15.0 + 10.0 * (t / 4.0).sin()) as f32),
        network_tx_bytes: (t * 10_000_000.0) as u64,
        network_rx_bytes: (t * 50_000_000.0) as u64,
        network_tx_speed: (2_000_000.0 + 18_000_000.0 * (t / 3.0).sin().abs()) as u64,
        network_rx_speed: (5_000_000.0 + 45_000_000.0 * (t / 4.0).sin().abs()) as u64,
    }
}

pub async fn connect_and_auth() -> Result<EncryptedConnection, String> {
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
    let addr = {
        let guard = SERVER_ADDRESS
            .lock()
            .map_err(|_| "Failed to lock server address".to_string())?;
        guard.clone()
    };

    let mut stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    // --- Handshake Phase ---
    // 1. Generate Client Key
    let (secret, client_public) = common::crypto::generate_ephemeral();
    let client_pub_b64 = BASE64_STANDARD.encode(client_public.as_bytes());

    // 2. Send ClientHello
    let hello = Handshake::ClientHello {
        public_key: client_pub_b64,
    };
    let hello_bytes = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
    stream
        .write_u32(hello_bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(&hello_bytes)
        .await
        .map_err(|e| e.to_string())?;

    // 3. Read ServerHello
    let len = stream.read_u32().await.map_err(|e| e.to_string())? as usize;
    let mut buf = vec![0u8; len];
    stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| e.to_string())?;
    let server_hello: Handshake = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;

    let server_pub_bytes = match server_hello {
        Handshake::ServerHello { public_key } => BASE64_STANDARD
            .decode(public_key)
            .map_err(|e| e.to_string())?,
        _ => return Err("Expected ServerHello".to_string()),
    };

    // 4. Initialize Crypto Session
    let server_pub_array: [u8; 32] = server_pub_bytes
        .try_into()
        .map_err(|_| "Invalid server public key length".to_string())?;
    let server_public_key = PublicKey::from(server_pub_array);
    let shared_secret = secret.diffie_hellman(&server_public_key);
    let crypto = CryptoSession::new(shared_secret.to_bytes(), false); // is_server = false

    let mut conn = EncryptedConnection { stream, crypto };

    // --- Login Phase (Encrypted) ---
    // 3.1 Get Challenge
    send_raw_request(&mut conn, Request::GetChallenge).await?;
    let resp = read_raw_response(&mut conn).await?;
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
        &mut conn,
        Request::Login {
            public_key: pub_key,
            signature: signature_str,
        },
    )
    .await?;

    let resp = read_raw_response(&mut conn).await?;
    match resp {
        Response::Ok => Ok(conn), // Login success
        Response::Error(e) => Err(format!("Login failed: {}", e)),
        _ => Err("Unexpected response during login".to_string()),
    }
}

pub async fn send_request(req: Request) -> Result<Response, String> {
    if MOCK_MODE.load(Ordering::Relaxed) {
        return Ok(match req {
            Request::GetStatus { .. } => Response::Status(generate_mock_system_info()),
            Request::ListServices => Response::Services(vec![]),
            Request::QueryPath { .. } => Response::PathSuggestions(vec![]),
            _ => Response::Ok,
        });
    }

    let mut guard = CONNECTION.lock().await;

    if guard.is_none() {
        *guard = Some(connect_and_auth().await?);
    }

    // Try sending with current connection
    let conn = guard
        .as_mut()
        .ok_or("Connection not initialized".to_string())?;

    let result = async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            send_raw_request(conn, req.clone()).await?;
            read_raw_response(conn).await
        })
        .await
        .map_err(|_| "Request timed out".to_string())?
    }
    .await;

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => {
            log::warn!("Request failed ({}). Reconnecting...", e);
            *guard = None;
            *guard = Some(connect_and_auth().await?);
            let conn = guard
                .as_mut()
                .ok_or("Connection not initialized".to_string())?;

            let resp = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                send_raw_request(conn, req).await?;
                read_raw_response(conn).await
            })
            .await
            .map_err(|_| "Retry request timed out".to_string())??;

            Ok(resp)
        }
    }
}

pub async fn send_raw_request(conn: &mut EncryptedConnection, req: Request) -> Result<(), String> {
    let req_bytes =
        serde_json::to_vec(&req).map_err(|e| format!("Failed to serialize request: {}", e))?;

    // Encrypt
    let ciphertext = conn
        .crypto
        .encrypt(&req_bytes)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    conn.stream
        .write_u32(ciphertext.len() as u32)
        .await
        .map_err(|e| format!("Failed to write length: {}", e))?;
    conn.stream
        .write_all(&ciphertext)
        .await
        .map_err(|e| format!("Failed to write body: {}", e))?;
    Ok(())
}

pub async fn read_raw_response(conn: &mut EncryptedConnection) -> Result<Response, String> {
    let len = conn
        .stream
        .read_u32()
        .await
        .map_err(|e| format!("Failed to read length: {}", e))? as usize;

    let mut buf = vec![0u8; len];
    conn.stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;

    // Decrypt
    let plaintext = conn
        .crypto
        .decrypt(&buf)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    serde_json::from_slice(&plaintext).map_err(|e| format!("Failed to deserialize: {}", e))
}

pub async fn query_path(path: String) -> Result<Vec<common::PathItem>, String> {
    let req = Request::QueryPath { path };
    let resp = send_request(req).await?;

    match resp {
        Response::PathSuggestions(items) => Ok(items),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}
