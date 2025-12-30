use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::prelude::*;
use common::{Request, Response, ServiceAction, Handshake, crypto::CryptoSession};
use ed25519_dalek::{Signer, SigningKey};
use hmac::Hmac;
use pbkdf2::pbkdf2;
use serde::Deserialize;
use sha2::Sha256;
use std::sync::LazyLock;
use std::sync::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::PublicKey;

// Global state to hold the decrypted private key
static SIGNING_KEY: LazyLock<Mutex<Option<(SigningKey, String)>>> =
    LazyLock::new(|| Mutex::new(None));

static SERVER_ADDRESS: LazyLock<Mutex<String>> =
    LazyLock::new(|| Mutex::new("frp-egg.com:28450".to_string()));

struct EncryptedConnection {
    stream: TcpStream,
    crypto: CryptoSession,
}

static CONNECTION: LazyLock<tokio::sync::Mutex<Option<EncryptedConnection>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

#[derive(Deserialize)]
struct KeyFile {
    pub_key: String,
    enc_priv_key: String,
    salt: String,
    nonce: String,
}

async fn connect_and_auth() -> Result<EncryptedConnection, String> {
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
        let guard = SERVER_ADDRESS.lock().map_err(|_| "Failed to lock server address".to_string())?;
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
    let hello = Handshake::ClientHello { public_key: client_pub_b64 };
    let hello_bytes = serde_json::to_vec(&hello).map_err(|e| e.to_string())?;
    stream.write_u32(hello_bytes.len() as u32).await.map_err(|e| e.to_string())?;
    stream.write_all(&hello_bytes).await.map_err(|e| e.to_string())?;

    // 3. Read ServerHello
    let len = stream.read_u32().await.map_err(|e| e.to_string())? as usize;
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await.map_err(|e| e.to_string())?;
    let server_hello: Handshake = serde_json::from_slice(&buf).map_err(|e| e.to_string())?;
    
    let server_pub_bytes = match server_hello {
        Handshake::ServerHello { public_key } => BASE64_STANDARD.decode(public_key).map_err(|e| e.to_string())?,
        _ => return Err("Expected ServerHello".to_string()),
    };

    // 4. Initialize Crypto Session
    let server_public_key = PublicKey::from(TryInto::<[u8; 32]>::try_into(server_pub_bytes).unwrap());
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
        _ => return Err("Unexpected response during login".to_string()),
    }
}

async fn send_request(req: Request) -> Result<Response, String> {
    let mut guard = CONNECTION.lock().await;

    if guard.is_none() {
        *guard = Some(connect_and_auth().await?);
    }

    // Try sending with current connection
    let conn = guard.as_mut().ok_or("Connection not initialized".to_string())?;
    
    let result = async {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            send_raw_request(conn, req.clone()).await?;
            read_raw_response(conn).await
        })
        .await
        .map_err(|_| "Request timed out".to_string())?
    }.await;

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => {
            log::warn!("Request failed ({}). Reconnecting...", e);
            *guard = None;
            *guard = Some(connect_and_auth().await?);
            let conn = guard.as_mut().ok_or("Connection not initialized".to_string())?;
            
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

async fn send_raw_request(conn: &mut EncryptedConnection, req: Request) -> Result<(), String> {
    let req_bytes = serde_json::to_vec(&req).map_err(|e| format!("Failed to serialize request: {}", e))?;
    
    // Encrypt
    let ciphertext = conn.crypto.encrypt(&req_bytes).map_err(|e| format!("Encryption failed: {}", e))?;

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

async fn read_raw_response(conn: &mut EncryptedConnection) -> Result<Response, String> {
    let len = conn.stream
        .read_u32()
        .await
        .map_err(|e| format!("Failed to read length: {}", e))? as usize;
    
    let mut buf = vec![0u8; len];
    conn.stream
        .read_exact(&mut buf)
        .await
        .map_err(|e| format!("Failed to read body: {}", e))?;
    
    // Decrypt
    let plaintext = conn.crypto.decrypt(&buf).map_err(|e| format!("Decryption failed: {}", e))?;

    serde_json::from_slice(&plaintext).map_err(|e| format!("Failed to deserialize: {}", e))
}

#[tauri::command]
pub async fn authenticate(passphrase: String, address: String) -> Result<String, String> {
    // Update address
    {
        let mut guard = SERVER_ADDRESS.lock().map_err(|_| "Failed to lock server address".to_string())?;
        *guard = address;
    }

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

    let signing_key = SigningKey::from_bytes(priv_key_bytes.as_slice().try_into().map_err(|_| "Invalid key length".to_string())?);

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
pub async fn get_status(interval_ms: Option<u64>) -> Result<common::StatusData, String> {
    match send_request(Request::GetStatus { interval_ms }).await? {
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

#[tauri::command]
pub async fn add_service(description: String, exe_path: String, args: Vec<String>) -> Result<usize, String> {
    match send_request(Request::AddService {
        description,
        exe_path,
        args,
    })
    .await?
    {
        Response::ServiceAdded(id) => Ok(id),
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}
