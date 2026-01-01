use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::prelude::*;
use common::{Request, Response, ServiceAction};
use ed25519_dalek::SigningKey;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use sha2::Sha256;

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub async fn authenticate(passphrase: String, address: String) -> Result<String, String> {
    println!("Authenticating to address: {}", address);
    // Update address
    {
        let mut guard = client::SERVER_ADDRESS
            .lock()
            .map_err(|_| "Failed to lock server address".to_string())?;
        *guard = address;
    }

    // Hardcoded key data
    let key_file = client::KeyFile {
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

    let signing_key = SigningKey::from_bytes(
        priv_key_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid key length".to_string())?,
    );

    // Store in global state
    {
        let mut guard = client::SIGNING_KEY
            .lock()
            .map_err(|_| "Failed to lock key store".to_string())?;
        *guard = Some((signing_key, key_file.pub_key));
    }

    Ok("Authenticated successfully".to_string())
}

#[tauri::command]
pub async fn get_status(interval_ms: Option<u64>) -> Result<common::SystemInfo, String> {
    match client::send_request(Request::GetStatus { interval_ms }).await? {
        Response::Status(mut data) => {
            if data.nanoid.is_empty() {
                data.nanoid = common::func::nanoid_gen();
            }
            Ok(data)
        }
        Response::Error(e) => Err(e),
        _ => Err("Unexpected response".to_string()),
    }
}

#[tauri::command]
pub async fn list_services() -> Result<Vec<common::ServiceInfo>, String> {
    match client::send_request(Request::ListServices).await? {
        Response::Services(mut data) => {
            for svc in &mut data {
                if svc.nanoid.is_empty() {
                    svc.nanoid = common::func::nanoid_gen();
                }
            }
            Ok(data)
        }
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

    match client::send_request(Request::ControlService {
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
pub async fn add_service(
    description: String,
    exe_path: String,
    args: Vec<String>,
) -> Result<usize, String> {
    match client::send_request(Request::AddService {
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

#[tauri::command]
pub async fn query_path(path: String) -> Result<Vec<common::PathItem>, String> {
    client::query_path(path).await
}
