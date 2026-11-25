//! 服务端部署身份。不存在或校验失败时，调用方必须禁用历史协议，不能自动生成。
use anyhow::{Context, Result, ensure};
use common::server_auth::{HISTORY_VERSION, ServerProof, fingerprint, sha256_bytes};
use ed25519_dalek::SigningKey;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
#[path = "identity/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "identity/windows.rs"]
mod platform;

const DIRECTORY: &str = "identity";
const FILENAME: &str = "server-identity.v1";
const HEADER: &[u8; 8] = b"RHID\0\0\0\x01";
const PAYLOAD_SIZE: usize = HEADER.len() + 16 + 32;
const FILE_SIZE: usize = PAYLOAD_SIZE + 32;

pub struct ServerIdentity {
    key: SigningKey,
    instance_id: [u8; 16],
}

impl ServerIdentity {
    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            key: SigningKey::from_bytes(&[73; 32]),
            instance_id: [74; 16],
        }
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(self.key.verifying_key().as_bytes())
    }

    pub fn proof(
        &self,
        client: &[u8; 32],
        server: &[u8; 32],
        offered: &[u16],
    ) -> Result<(ServerProof, [u8; 32])> {
        ServerProof::sign(
            &self.key,
            self.instance_id,
            client,
            server,
            offered,
            HISTORY_VERSION,
        )
    }
}

fn config_directory(config: &Path) -> Result<PathBuf> {
    ensure!(
        !config.components().any(|c| c == Component::ParentDir),
        "identity path cannot contain '..'"
    );
    let config = std::path::absolute(config)?;
    Ok(config
        .parent()
        .context("configuration path has no parent")?
        .to_owned())
}

pub fn initialize(config: &Path, service_reader: Option<&str>) -> Result<ServerIdentity> {
    let directory = config_directory(config)?;
    let mut seed: [u8; 32] = rand::random();
    let identity = ServerIdentity {
        key: SigningKey::from_bytes(&seed),
        instance_id: rand::random(),
    };
    let mut bytes = [0u8; FILE_SIZE];
    bytes[..8].copy_from_slice(HEADER);
    bytes[8..24].copy_from_slice(&identity.instance_id);
    bytes[24..PAYLOAD_SIZE].copy_from_slice(&seed);
    let checksum = sha256_bytes(&bytes[..PAYLOAD_SIZE]);
    bytes[PAYLOAD_SIZE..].copy_from_slice(&checksum);
    seed.fill(0);
    let result = platform::publish(&directory, service_reader, |mut file| {
        file.write_all(&bytes)?;
        file.sync_all()?;
        Ok(())
    });
    bytes.fill(0);
    result.context("unable to publish server identity")?;
    Ok(identity)
}

pub fn load(config: &Path) -> Result<ServerIdentity> {
    let directory = config_directory(config)?;
    let mut bytes = [0u8; FILE_SIZE];
    let result = platform::read(&directory, |mut file: File| {
        file.read_exact(&mut bytes)?;
        ensure!(
            file.read(&mut [0u8; 1])? == 0 && &bytes[..8] == HEADER,
            "invalid identity file format"
        );
        let instance_id = bytes[8..24].try_into().expect("fixed identity length");
        ensure!(
            bytes[PAYLOAD_SIZE..] == sha256_bytes(&bytes[..PAYLOAD_SIZE]),
            "identity file checksum mismatch"
        );
        let key = SigningKey::from_bytes(
            bytes[24..PAYLOAD_SIZE]
                .try_into()
                .expect("fixed key length"),
        );
        Ok(ServerIdentity { key, instance_id })
    });
    bytes.fill(0);
    result.context("server identity is unavailable")
}
