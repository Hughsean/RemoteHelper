//! 服务端身份证明与客户端登录使用不同的签名域。
use std::fmt::Write as _;

use base64::prelude::*;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};

pub const HANDSHAKE_MAX: usize = 1024;
pub const HISTORY_VERSION: u16 = 1;
pub const MAX_HISTORY_VERSIONS: usize = 32;
const HELLO_DOMAIN: &[u8] = b"RemoteHelper server history handshake v1\0";
const LOGIN_DOMAIN: &[u8] = b"RemoteHelper client history login v1\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerIdentity {
    pub fingerprint: String,
    pub instance_id: [u8; 16],
    pub history_version: u16,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerProof {
    pub version: u16,
    pub instance_id: [u8; 16],
    pub public_key: String,
    pub signature: String,
}

/// 扩展字段存在但为 null 或格式错误时，不按旧协议处理。
pub fn decode_proof<'de, D: Deserializer<'de>>(d: D) -> Result<Option<ServerProof>, D::Error> {
    ServerProof::deserialize(d).map(Some)
}

pub fn decode_versions<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u16>, D::Error> {
    let versions = Vec::<u16>::deserialize(d)?;
    if versions.is_empty() || versions.len() > MAX_HISTORY_VERSIONS {
        return Err(serde::de::Error::custom("invalid history version offer"));
    }
    Ok(versions)
}

pub fn decode_base64<const N: usize>(text: &str) -> anyhow::Result<[u8; N]> {
    BASE64_STANDARD
        .decode(text)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("invalid decoded key, signature or challenge length"))
}

pub fn fingerprint(public_key: &[u8; 32]) -> String {
    sha256_hex(public_key)
}

pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(64);
    for byte in sha256_bytes(bytes) {
        write!(hex, "{byte:02x}").expect("writing String cannot fail");
    }
    hex
}

/// 签名编码依次为固定域、双方的 X25519 公钥、实例 ID、Ed25519 公钥、
/// 版本数量（大端 u16）、按发送顺序排列的版本，以及最终选定的版本。
fn transcript(
    client: &[u8; 32],
    server: &[u8; 32],
    instance: &[u8; 16],
    signing_public: &[u8; 32],
    offered: &[u16],
    selected: u16,
) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        (1..=MAX_HISTORY_VERSIONS).contains(&offered.len()) && offered.contains(&selected),
        "invalid history selection"
    );
    let mut bytes = Vec::with_capacity(HELLO_DOMAIN.len() + 116 + 2 * offered.len());
    bytes.extend_from_slice(HELLO_DOMAIN);
    bytes.extend_from_slice(client);
    bytes.extend_from_slice(server);
    bytes.extend_from_slice(instance);
    bytes.extend_from_slice(signing_public);
    bytes.extend_from_slice(&(offered.len() as u16).to_be_bytes());
    for v in offered {
        bytes.extend_from_slice(&v.to_be_bytes());
    }
    bytes.extend_from_slice(&selected.to_be_bytes());
    Ok(bytes)
}

impl ServerProof {
    pub fn sign(
        key: &SigningKey,
        instance_id: [u8; 16],
        client: &[u8; 32],
        server: &[u8; 32],
        offered: &[u16],
        version: u16,
    ) -> anyhow::Result<(Self, [u8; 32])> {
        let public = key.verifying_key().to_bytes();
        let bytes = transcript(client, server, &instance_id, &public, offered, version)?;
        let signature = key.sign(&bytes);
        Ok((
            Self {
                version,
                instance_id,
                public_key: BASE64_STANDARD.encode(public),
                signature: BASE64_STANDARD.encode(signature.to_bytes()),
            },
            Sha256::digest(&bytes).into(),
        ))
    }

    pub fn verify(
        &self,
        client: &[u8; 32],
        server: &[u8; 32],
        offered: &[u16],
    ) -> anyhow::Result<(ServerIdentity, [u8; 32])> {
        let public = decode_base64(&self.public_key)?;
        let signature = Signature::from_bytes(&decode_base64(&self.signature)?);
        let key = VerifyingKey::from_bytes(&public)?;
        let bytes = transcript(
            client,
            server,
            &self.instance_id,
            &public,
            offered,
            self.version,
        )?;
        key.verify_strict(&bytes, &signature)?;
        Ok((
            ServerIdentity {
                fingerprint: fingerprint(&public),
                instance_id: self.instance_id,
                history_version: self.version,
            },
            Sha256::digest(&bytes).into(),
        ))
    }
}

/// 扩展登录的签名内容不能与旧协议的 32 字节挑战混淆。
pub fn history_login_message(challenge: &[u8; 32], transcript_hash: &[u8; 32]) -> Vec<u8> {
    [
        LOGIN_DOMAIN,
        challenge.as_slice(),
        transcript_hash.as_slice(),
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Handshake;

    #[test]
    fn proof_binds_keys_installation_version_offer_and_selection() {
        let key = SigningKey::from_bytes(&[8; 32]);
        let client = [1; 32];
        let server = [2; 32];
        let (mut proof, hash) =
            ServerProof::sign(&key, [3; 16], &client, &server, &[1, 2], 1).unwrap();
        let (identity, calculated) = proof.verify(&client, &server, &[1, 2]).unwrap();
        assert_eq!(
            identity.fingerprint,
            fingerprint(key.verifying_key().as_bytes())
        );
        assert_eq!(hash, calculated);
        assert!(proof.verify(&[0; 32], &server, &[1, 2]).is_err());
        assert!(proof.verify(&client, &[0; 32], &[1, 2]).is_err());
        assert!(proof.verify(&client, &server, &[2, 1]).is_err());
        assert!(proof.verify(&client, &server, &[1]).is_err());
        proof.version = 2;
        assert!(proof.verify(&client, &server, &[1, 2]).is_err());
        proof.version = 1;
        proof.instance_id[0] ^= 1;
        assert!(proof.verify(&client, &server, &[1, 2]).is_err());
    }

    #[test]
    fn extended_login_is_not_legacy_or_replayable_on_another_transcript() {
        let key = SigningKey::from_bytes(&[8; 32]);
        let msg = history_login_message(&[3; 32], &[4; 32]);
        let sig = key.sign(&msg);
        let verify = key.verifying_key();
        assert!(verify.verify_strict(&msg, &sig).is_ok());
        assert!(verify.verify_strict(&[3; 32], &sig).is_err());
        assert!(
            verify
                .verify_strict(&history_login_message(&[3; 32], &[5; 32]), &sig)
                .is_err()
        );
        assert!(
            verify
                .verify_strict(&history_login_message(&[4; 32], &[4; 32]), &sig)
                .is_err()
        );
    }

    #[test]
    fn absent_extensions_are_legacy_but_null_is_invalid() {
        assert!(
            matches!(serde_json::from_str(r#"{"ClientHello":{"public_key":"key"}}"#).unwrap(),
            Handshake::ClientHello { history_versions, .. } if history_versions.is_empty())
        );
        assert!(matches!(
            serde_json::from_str(r#"{"ServerHello":{"public_key":"key"}}"#).unwrap(),
            Handshake::ServerHello { history: None, .. }
        ));
        assert!(
            serde_json::from_str::<Handshake>(
                r#"{"ServerHello":{"public_key":"key","history":null}}"#
            )
            .is_err()
        );
    }
}
