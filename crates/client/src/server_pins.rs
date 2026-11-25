//! 本机固定记录按地址保存；只有认证通过后，才在加锁的比较交换中原子替换记录。
use ed25519_dalek::SigningKey;
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::EncryptedConnection;
use crate::connection::authenticate_exact;

const MAX_RECORD: u64 = 4096;

type Result<T> = std::result::Result<T, String>;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    address: String,
    fingerprint: String,
}

fn pin_directory() -> Result<PathBuf> {
    Ok(dirs::config_dir()
        .ok_or("Cannot locate client configuration directory")?
        .join("remotehelper")
        .join("server-pins"))
}

pub(crate) fn canonical_address(address: &str) -> Result<String> {
    let address = address.trim();
    if address.is_empty()
        || address.len() > 1024
        || address
            .chars()
            .any(|ch| ch.is_whitespace() || ch.is_control())
    {
        return Err("Invalid server address".into());
    }
    Ok(address.to_ascii_lowercase())
}

pub(crate) fn validate_fingerprint(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
    {
        return Err("Expected a full lowercase hexadecimal SHA-256 fingerprint".into());
    }
    Ok(())
}

fn pin_path(dir: &Path, address: &str) -> PathBuf {
    let hash = common::server_auth::sha256_hex(address.as_bytes());
    dir.join(format!("{hash}.json"))
}

fn read_record(path: &Path) -> Result<Option<Pin>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_RECORD + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_RECORD {
        return Err("Server pin record exceeds size limit".into());
    }
    let pin: Pin =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid server pin record: {e}"))?;
    validate_fingerprint(&pin.fingerprint)?;
    if canonical_address(&pin.address)? != pin.address {
        return Err("Invalid pin address".into());
    }
    Ok(Some(pin))
}

fn read_pin(dir: &Path, address: &str) -> Result<Option<String>> {
    match read_record(&pin_path(dir, address))? {
        Some(pin) if pin.address == address => Ok(Some(pin.fingerprint)),
        Some(_) => Err("Server pin address mismatch".into()),
        None => Ok(None),
    }
}

pub fn load_server_pin(address: &str) -> Result<Option<String>> {
    read_pin(&pin_directory()?, &canonical_address(address)?)
}

pub fn list_server_pins() -> Result<BTreeMap<String, String>> {
    let dir = pin_directory()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(error.to_string()),
    };
    let mut pins = BTreeMap::new();
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            if let Some(pin) = read_record(&path)? {
                if path != pin_path(&dir, &pin.address) {
                    return Err("Server pin filename mismatch".into());
                }
                pins.insert(pin.address, pin.fingerprint);
            }
        }
    }
    Ok(pins)
}

fn replace_pin(dir: &Path, address: &str, old: Option<&str>, new: &str) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(".lock"))
        .map_err(|e| e.to_string())?;
    lock.lock().map_err(|e| e.to_string())?;
    if read_pin(dir, address)?.as_deref() != old {
        return Err("Server binding changed while authenticating; retry the confirmation".into());
    }
    if old == Some(new) {
        return Ok(());
    }
    let bytes = serde_json::to_vec(&Pin {
        address: address.into(),
        fingerprint: new.into(),
    })
    .map_err(|e| e.to_string())?;
    let temp = dir.join(format!(".pin-{:032x}.tmp", rand::random::<u128>()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&temp, pin_path(dir, address)));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result.map_err(|e| e.to_string())
}

/// 用户核对首次探测结果（或部署方提供的指纹）后调用；不会覆盖其他服务器绑定。
pub async fn confirm_server_identity(
    address: &str,
    fingerprint: &str,
    signing_key: &SigningKey,
    public_key: &str,
) -> Result<EncryptedConnection> {
    validate_fingerprint(fingerprint)?;
    let address = canonical_address(address)?;
    let old = load_server_pin(&address)?;
    if old.as_deref().is_some_and(|value| value != fingerprint) {
        return Err("Server binding already exists; use the separate rebind operation".into());
    }
    finish_binding(address, old, fingerprint, signing_key, public_key).await
}

/// 用户明确批准更换固定绑定后调用；旧绑定必须与核对时一致。
pub async fn rebind_server_identity(
    address: &str,
    old_fingerprint: &str,
    new_fingerprint: &str,
    signing_key: &SigningKey,
    public_key: &str,
) -> Result<EncryptedConnection> {
    validate_fingerprint(old_fingerprint)?;
    validate_fingerprint(new_fingerprint)?;
    let address = canonical_address(address)?;
    let old = load_server_pin(&address)?;
    if old.as_deref() != Some(old_fingerprint) || old_fingerprint == new_fingerprint {
        return Err(
            "Server rebind requires the current pin and a different verified candidate".into(),
        );
    }
    finish_binding(address, old, new_fingerprint, signing_key, public_key).await
}

async fn finish_binding(
    address: String,
    old: Option<String>,
    new: &str,
    signing_key: &SigningKey,
    public_key: &str,
) -> Result<EncryptedConnection> {
    let conn =
        authenticate_exact(&address, Some(new), old.as_deref(), signing_key, public_key).await?;
    replace_pin(&pin_directory()?, &address, old.as_deref(), new)?;
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_requires_previous_pin_and_preserves_it_on_conflict() {
        let dir =
            std::env::temp_dir().join(format!("remotehelper-pins-{:032x}", rand::random::<u128>()));
        let first = "a".repeat(64);
        let replacement = "0".repeat(64);
        replace_pin(&dir, "local:42", None, &first).unwrap();
        assert!(replace_pin(&dir, "local:42", None, &replacement).is_err());
        assert_eq!(read_pin(&dir, "local:42").unwrap(), Some(first.clone()));
        replace_pin(&dir, "local:42", Some(&first), &replacement).unwrap();
        assert_eq!(read_pin(&dir, "local:42").unwrap(), Some(replacement));
        fs::remove_dir_all(dir).unwrap();
    }
}
