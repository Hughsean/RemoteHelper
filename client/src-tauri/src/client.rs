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
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;
use rustls::pki_types::ServerName;

// Global state to hold the decrypted private key
static SIGNING_KEY: LazyLock<Mutex<Option<(SigningKey, String)>>> =
    LazyLock::new(|| Mutex::new(None));

static SERVER_ADDRESS: LazyLock<Mutex<String>> =
    LazyLock::new(|| Mutex::new("frp-egg.com:28450".to_string()));

static CONNECTION: LazyLock<tokio::sync::Mutex<Option<TlsStream<TcpStream>>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));

#[derive(Deserialize)]
struct KeyFile {
    pub_key: String,
    enc_priv_key: String,
    salt: String,
    nonce: String,
}

async fn connect_and_auth() -> Result<TlsStream<TcpStream>, String> {
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
    
    let stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| format!("Failed to connect to server: {}", e))?;

    // TLS Handshake
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    
    // Allow invalid certs for now (self-signed) or use native certs
    // For strict security, we should pin the cert or use a proper CA.
    // Here we use native certs + webpki roots.
    let certs = rustls_native_certs::load_native_certs();
    for cert in certs.certs {
        root_store.add(cert).ok();
    }

    // Embed and trust the self-signed certificate
    // This allows the client to connect to our server without installing the cert in the OS
    const CERT_BYTES: &[u8] = include_bytes!("../../../cert.pem");
    let mut reader = std::io::BufReader::new(std::io::Cursor::new(CERT_BYTES));
    for cert in rustls_pemfile::certs(&mut reader) {
        if let Ok(cert) = cert {
            root_store.add(cert).ok();
        }
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
        
    let connector = TlsConnector::from(Arc::new(config));
    let domain = addr.split(':').next().unwrap_or("localhost");
    let server_name = ServerName::try_from(domain)
        .map_err(|_| "Invalid server name".to_string())?
        .to_owned();

    let mut stream = connector.connect(server_name, stream).await
        .map_err(|e| format!("TLS handshake failed: {}", e))?;

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
        Response::Ok => Ok(stream), // Login success
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
    let stream = guard.as_mut().ok_or("Connection not initialized".to_string())?;
    
    // We need to handle the error here. If send or read fails, we reconnect.
    let result = async {
        // Add timeout for the whole operation (send + receive)
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            send_raw_request(stream, req.clone()).await?;
            read_raw_response(stream).await
        })
        .await
        .map_err(|_| "Request timed out".to_string())?
    }.await;

    match result {
        Ok(resp) => Ok(resp),
        Err(e) => {
            // Failed, assume broken connection. Reconnect.
            // Only retry if it's not a logic error from server (which would be Ok(Response::Error))
            // But here we catch network/timeout errors.
            log::warn!("Request failed ({}). Reconnecting...", e);
            *guard = None;
            *guard = Some(connect_and_auth().await?);
            let stream = guard.as_mut().ok_or("Connection not initialized".to_string())?;
            
            // Retry once with timeout
            let resp = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                send_raw_request(stream, req).await?;
                read_raw_response(stream).await
            })
            .await
            .map_err(|_| "Retry request timed out".to_string())??;
            
            Ok(resp)
        }
    }
}

async fn send_raw_request(stream: &mut TlsStream<TcpStream>, req: Request) -> Result<(), String> {
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

async fn read_raw_response(stream: &mut TlsStream<TcpStream>) -> Result<Response, String> {
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
