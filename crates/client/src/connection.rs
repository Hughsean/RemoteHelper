//! 建立加密连接，先验证服务端身份，再进行客户端签名认证。
use crate::EncryptedConnection;
use crate::server_pins::{canonical_address, load_server_pin, validate_fingerprint};
use crate::state::{SERVER_ADDRESS, SIGNING_KEY};
use base64::prelude::*;
use common::server_auth::{
    HANDSHAKE_MAX, HISTORY_VERSION, ServerIdentity, decode_base64, history_login_message,
};
use common::{Handshake, Request, Response, crypto::CryptoSession};
use ed25519_dalek::{Signer, SigningKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use x25519_dalek::PublicKey;

const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
type Result<T> = std::result::Result<T, String>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerProbe {
    Legacy,
    History(ServerIdentity),
}

/// 验证自述身份的签名，不发送客户端公钥、挑战请求或登录签名。
pub async fn probe_server(address: &str) -> Result<ServerProbe> {
    let (_, proof) =
        tokio::time::timeout(HANDSHAKE_TIMEOUT, negotiate(&canonical_address(address)?))
            .await
            .map_err(|_| "Server probe timed out")??;
    Ok(match proof {
        Some((identity, _)) => ServerProbe::History(identity),
        None => ServerProbe::Legacy,
    })
}

/// 根据当前本机绑定自动验证；从不自动信任首次出现的历史身份。
pub async fn connect_and_auth() -> Result<EncryptedConnection> {
    let address = SERVER_ADDRESS
        .lock()
        .map_err(|_| "Failed to lock server address")?
        .clone();
    let (signing_key, public_key) = credentials()?;
    let pin = load_server_pin(&address)?;
    connect_with_pin(&address, pin.as_deref(), &signing_key, &public_key).await
}

fn credentials() -> Result<(SigningKey, String)> {
    SIGNING_KEY
        .lock()
        .map_err(|_| "Failed to lock key store")?
        .clone()
        .ok_or_else(|| "Not authenticated. Please login first.".into())
}

/// 连接指定地址。没有指纹时只允许旧协议；提供指纹时要求精确匹配。
pub async fn connect_with_pin(
    address: &str,
    fingerprint: Option<&str>,
    signing_key: &SigningKey,
    public_key: &str,
) -> Result<EncryptedConnection> {
    let stored = load_server_pin(address)?;
    if stored
        .as_deref()
        .is_some_and(|actual| Some(actual) != fingerprint)
    {
        return Err("Server pin changed or cannot be downgraded".into());
    }
    // 不返回尚未持久化绑定的历史连接。首次信任调用 confirm_server_identity。
    if stored.as_deref() != fingerprint {
        return Err("Server fingerprint must be explicitly confirmed and saved".into());
    }
    authenticate_exact(
        &canonical_address(address)?,
        fingerprint,
        stored.as_deref(),
        signing_key,
        public_key,
    )
    .await
}

/// 为独立同步连接验证主会话使用的地址和固定指纹；不能退回旧协议。
pub async fn connect_verified(
    address: &str,
    fingerprint: &str,
    signing_key: &SigningKey,
    public_key: &str,
) -> Result<EncryptedConnection> {
    connect_with_pin(address, Some(fingerprint), signing_key, public_key).await
}

pub(crate) async fn authenticate_exact(
    address: &str,
    expected: Option<&str>,
    previous_pin: Option<&str>,
    signing_key: &SigningKey,
    public_key: &str,
) -> Result<EncryptedConnection> {
    tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        if let Some(expected) = expected {
            validate_fingerprint(expected)?;
        }
        if decode_base64::<32>(public_key).map_err(|e| e.to_string())?
            != signing_key.verifying_key().to_bytes()
        {
            return Err("Client public key does not match the signing key".into());
        }
        let (mut connection, proof) = negotiate(address).await?;
        match (expected, proof.as_ref()) {
            (Some(expected), Some((identity, _))) if expected == identity.fingerprint => (),
            (Some(_), Some(_)) => {
                return Err("Server fingerprint changed; verify and rebind explicitly".into());
            }
            (Some(_), None) => {
                return Err(
                    "Pinned server did not provide a valid historical protocol identity".into(),
                );
            }
            (None, Some(_)) => return Err("First-use server identity must be confirmed".into()),
            (None, None) => (),
        }
        connection.send(&Request::GetChallenge).await?;
        let mut buf = [0u8; 4096];
        let challenge = match connection.recv(&mut buf).await? {
            Response::Challenge(challenge) => {
                decode_base64::<32>(&challenge).map_err(|e| e.to_string())?
            }
            Response::Error(e) => return Err(format!("Server rejected challenge request: {e}")),
            _ => return Err("Unexpected challenge response".into()),
        };
        if load_server_pin(address)?.as_deref() != previous_pin {
            return Err("Server binding changed before authentication".into());
        }
        let request = match proof.as_ref() {
            Some((_, hash)) => Request::LoginHistory {
                public_key: public_key.into(),
                signature: BASE64_STANDARD.encode(
                    signing_key
                        .sign(&history_login_message(&challenge, hash))
                        .to_bytes(),
                ),
            },
            None => Request::Login {
                public_key: public_key.into(),
                signature: BASE64_STANDARD.encode(signing_key.sign(&challenge).to_bytes()),
            },
        };
        connection.send(&request).await?;
        match connection.recv(&mut buf).await? {
            Response::Ok => {
                if load_server_pin(address)?.as_deref() != previous_pin {
                    return Err("Server binding changed during authentication".into());
                }
                connection.server_identity = proof.map(|(identity, _)| identity);
                Ok(connection)
            }
            Response::Error(e) => Err(format!("Login failed: {e}")),
            _ => Err("Unexpected response during login".into()),
        }
    })
    .await
    .map_err(|_| "Server login timed out")?
}

async fn negotiate(
    address: &str,
) -> Result<(EncryptedConnection, Option<(ServerIdentity, [u8; 32])>)> {
    let mut stream = TcpStream::connect(address)
        .await
        .map_err(|e| format!("Failed to connect to server: {e}"))?;
    let (secret, client_public) = common::crypto::generate_ephemeral();
    let offered = [HISTORY_VERSION];
    let bytes = serde_json::to_vec(&Handshake::ClientHello {
        public_key: BASE64_STANDARD.encode(client_public.as_bytes()),
        history_versions: offered.to_vec(),
    })
    .map_err(|e| e.to_string())?;
    if bytes.len() > HANDSHAKE_MAX {
        return Err("Handshake message too large".into());
    }
    stream
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(&bytes).await.map_err(|e| e.to_string())?;
    let len = stream.read_u32().await.map_err(|e| e.to_string())? as usize;
    if !(1..=HANDSHAKE_MAX).contains(&len) {
        return Err("Invalid handshake message size".into());
    }
    let mut buf = [0u8; HANDSHAKE_MAX];
    stream
        .read_exact(&mut buf[..len])
        .await
        .map_err(|e| e.to_string())?;
    let hello = serde_json::from_slice::<Handshake>(&buf[..len]).map_err(|e| e.to_string())?;
    let Handshake::ServerHello {
        public_key,
        history,
    } = hello
    else {
        return Err("Expected ServerHello".into());
    };
    let server_public = decode_base64::<32>(&public_key).map_err(|e| e.to_string())?;
    let proof = history
        .map(|proof| proof.verify(client_public.as_bytes(), &server_public, &offered))
        .transpose()
        .map_err(|e| format!("Invalid signed server handshake: {e}"))?;
    let shared = secret.diffie_hellman(&PublicKey::from(server_public));
    if shared.to_bytes() == [0; 32] {
        return Err("Non-contributory server public key".into());
    }
    Ok((
        EncryptedConnection::new(stream, CryptoSession::new(shared.to_bytes(), false)),
        proof,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::server_auth::{ServerProof, fingerprint};
    use ed25519_dalek::{Signature, VerifyingKey};
    use tokio::net::TcpListener;

    async fn mock_server(
        listener: TcpListener,
        server_key: Option<SigningKey>,
        expect_client_auth: bool,
    ) {
        let (mut stream, _) = listener.accept().await.unwrap();
        let len = stream.read_u32().await.unwrap() as usize;
        assert!(len <= HANDSHAKE_MAX);
        let mut bytes = vec![0u8; len];
        stream.read_exact(&mut bytes).await.unwrap();
        let Handshake::ClientHello {
            public_key,
            history_versions,
        } = serde_json::from_slice(&bytes).unwrap()
        else {
            panic!("缺少客户端问候")
        };
        let client_key = decode_base64::<32>(&public_key).unwrap();
        let (secret, ephemeral) = common::crypto::generate_ephemeral();
        let proof = server_key.map(|key| {
            ServerProof::sign(
                &key,
                [7; 16],
                &client_key,
                ephemeral.as_bytes(),
                &history_versions,
                HISTORY_VERSION,
            )
            .unwrap()
        });
        let (history, digest) = match proof {
            Some((proof, digest)) => (Some(proof), Some(digest)),
            None => (None, None),
        };
        let reply = serde_json::to_vec(&Handshake::ServerHello {
            public_key: BASE64_STANDARD.encode(ephemeral.as_bytes()),
            history,
        })
        .unwrap();
        stream.write_u32(reply.len() as u32).await.unwrap();
        stream.write_all(&reply).await.unwrap();
        if !expect_client_auth {
            let mut byte = [0];
            assert_eq!(
                stream.read(&mut byte).await.unwrap(),
                0,
                "不受信的服务端不能收到客户端凭证"
            );
            return;
        }
        let mut crypto = CryptoSession::new(
            secret
                .diffie_hellman(&PublicKey::from(client_key))
                .to_bytes(),
            true,
        );
        let mut buf = [0; 4096];
        use common::transport::{FrameResult, recv_frame, send_frame};
        assert!(matches!(
            recv_frame::<Request>(&mut stream, &mut crypto, &mut buf)
                .await
                .unwrap(),
            FrameResult::Message(Request::GetChallenge)
        ));
        let challenge = [9; 32];
        send_frame(
            &mut stream,
            &mut crypto,
            &Response::Challenge(BASE64_STANDARD.encode(challenge)),
        )
        .await
        .unwrap();
        let request = recv_frame::<Request>(&mut stream, &mut crypto, &mut buf)
            .await
            .unwrap();
        let (public_key, signature, message) = match (digest, request) {
            (
                Some(digest),
                FrameResult::Message(Request::LoginHistory {
                    public_key,
                    signature,
                }),
            ) => (
                public_key,
                signature,
                history_login_message(&challenge, &digest),
            ),
            (
                None,
                FrameResult::Message(Request::Login {
                    public_key,
                    signature,
                }),
            ) => (public_key, signature, challenge.to_vec()),
            _ => panic!("登录类型与握手不匹配"),
        };
        let key = VerifyingKey::from_bytes(&decode_base64(&public_key).unwrap()).unwrap();
        key.verify_strict(
            &message,
            &Signature::from_bytes(&decode_base64(&signature).unwrap()),
        )
        .unwrap();
        send_frame(&mut stream, &mut crypto, &Response::Ok)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn verifies_fingerprint_before_sending_client_credentials() {
        let client = SigningKey::from_bytes(&[4; 32]);
        let public = BASE64_STANDARD.encode(client.verifying_key().as_bytes());
        let server = SigningKey::from_bytes(&[5; 32]);
        let actual = fingerprint(server.verifying_key().as_bytes());
        let wrong = "0".repeat(64);
        for (key, pin, succeeds) in [
            (Some(server.clone()), Some(wrong.as_str()), false),
            (Some(server.clone()), None, false),
            (None, Some(actual.as_str()), false),
            (Some(server), Some(actual.as_str()), true),
            (None, None, true),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap().to_string();
            let task = tokio::spawn(mock_server(listener, key, succeeds));
            let result = authenticate_exact(&address, pin, None, &client, &public).await;
            assert_eq!(result.is_ok(), succeeds, "{:?}", result.err());
            task.await.unwrap();
        }
    }
}
