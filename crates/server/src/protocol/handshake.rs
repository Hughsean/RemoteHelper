use crate::identity::ServerIdentity;
use anyhow::{Result, anyhow, ensure};
use base64::prelude::*;
use common::{
    Handshake,
    crypto::CryptoSession,
    server_auth::{HANDSHAKE_MAX, HISTORY_VERSION, decode_base64},
};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, tcp::OwnedReadHalf, tcp::OwnedWriteHalf};
use tokio::sync::Mutex;
use x25519_dalek::PublicKey;

pub async fn perform_handshake(
    mut socket: TcpStream,
    identity: Option<&ServerIdentity>,
) -> Result<(
    OwnedReadHalf,
    Arc<Mutex<OwnedWriteHalf>>,
    Arc<Mutex<CryptoSession>>,
    Option<[u8; 32]>,
)> {
    let peer_addr = socket.peer_addr().ok();
    tracing::info!(?peer_addr, "正在处理连接");
    let mut buf = [0u8; HANDSHAKE_MAX];
    let len = socket.read_u32().await? as usize;
    ensure!(
        (1..=HANDSHAKE_MAX).contains(&len),
        "Invalid handshake message size"
    );
    socket.read_exact(&mut buf[..len]).await?;
    let hello = serde_json::from_slice::<Handshake>(&buf[..len])?;
    let (client_public, offered) = match hello {
        Handshake::ClientHello {
            public_key,
            history_versions,
        } => (decode_base64(&public_key)?, history_versions),
        _ => return Err(anyhow!("Expected ClientHello")),
    };

    let (secret, server_public) = common::crypto::generate_ephemeral();
    let shared_secret = secret.diffie_hellman(&PublicKey::from(client_public));
    ensure!(
        shared_secret.to_bytes() != [0; 32],
        "Non-contributory client public key"
    );

    let (history, transcript_hash) = match identity.filter(|_| offered.contains(&HISTORY_VERSION)) {
        Some(identity) => {
            let (proof, hash) =
                identity.proof(&client_public, server_public.as_bytes(), &offered)?;
            (Some(proof), Some(hash))
        }
        None => (None, None),
    };
    let response = serde_json::to_vec(&Handshake::ServerHello {
        public_key: BASE64_STANDARD.encode(server_public.as_bytes()),
        history,
    })?;
    ensure!(
        response.len() <= HANDSHAKE_MAX,
        "Handshake message too large"
    );
    socket.write_u32(response.len() as u32).await?;
    socket.write_all(&response).await?;

    let (read, write) = socket.into_split();
    let crypto = CryptoSession::new(shared_secret.to_bytes(), true);
    Ok((
        read,
        Arc::new(Mutex::new(write)),
        Arc::new(Mutex::new(crypto)),
        transcript_hash,
    ))
}
