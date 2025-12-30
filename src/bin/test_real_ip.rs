use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::prelude::*;
use common::{Request, Response};
use ed25519_dalek::{Signer, SigningKey};
use hmac::Hmac;
use pbkdf2::pbkdf2;
use serde::Deserialize;
use sha2::Sha256;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[derive(Deserialize)]
struct KeyFile {
    pub_key: String,
    enc_priv_key: String,
    salt: String,
    nonce: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: cargo run --bin test_real_ip -- <address:port>");
        println!("Example: cargo run --bin test_real_ip -- 127.0.0.1:8088");
        return Ok(());
    }

    let addr = &args[1];

    // Load Key
    let file = File::open("client_key.json")
        .expect("Failed to open client_key.json. Run 'cargo run --bin keygen' first.");
    let key_file: KeyFile = serde_json::from_reader(file)?;

    print!("Enter passphrase for key {}: ", key_file.pub_key);
    io::stdout().flush()?;
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase)?;
    let passphrase = passphrase.trim();

    // Decrypt Key
    let salt = BASE64_STANDARD.decode(&key_file.salt)?;
    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, 100_000, &mut key)
        .expect("HMAC can be initialized with any key length");

    let cipher = Aes256Gcm::new(&key.into());
    let nonce_bytes = BASE64_STANDARD.decode(&key_file.nonce)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let enc_bytes = BASE64_STANDARD.decode(&key_file.enc_priv_key)?;

    let priv_key_bytes = cipher
        .decrypt(nonce, enc_bytes.as_ref())
        .map_err(|_| anyhow::anyhow!("Invalid passphrase or corrupted key file"))?;

    let signing_key = SigningKey::from_bytes(priv_key_bytes.as_slice().try_into().unwrap());

    println!("Connecting to {}...", addr);

    let mut stream = TcpStream::connect(addr).await?;

    // 1. Get Challenge
    send_request(&mut stream, Request::GetChallenge).await?;
    let resp = read_response(&mut stream).await?;

    let challenge = match resp {
        Response::Challenge(c) => c,
        _ => {
            eprintln!("Expected Challenge, got {:?}", resp);
            return Ok(());
        }
    };

    // 2. Sign Challenge
    let challenge_bytes = BASE64_STANDARD.decode(&challenge)?;
    let signature = signing_key.sign(&challenge_bytes);
    let signature_str = BASE64_STANDARD.encode(signature.to_bytes());

    // 3. Login
    send_request(
        &mut stream,
        Request::Login {
            public_key: key_file.pub_key.clone(),
            signature: signature_str,
        },
    )
    .await?;

    let resp = read_response(&mut stream).await?;
    match resp {
        Response::Ok => println!("Authentication Successful!"),
        Response::Error(e) => {
            eprintln!("Authentication Failed: {}", e);
            return Ok(());
        }
        _ => {
            eprintln!("Unexpected response during login: {:?}", resp);
            return Ok(());
        }
    }

    // 4. Get Status
    send_request(&mut stream, Request::GetStatus { interval_ms: None }).await?;
    let resp = read_response(&mut stream).await?;

    match resp {
        Response::Status(status) => {
            println!("\n=== Server Status ===");
            println!("CPU: {:.1}%", status.cpu_usage);
            println!("Mem: {} / {}", status.memory_usage, status.total_memory);
            println!("Uptime: {}s", status.uptime);
        }
        _ => println!("Received unexpected response: {:?}", resp),
    }

    Ok(())
}

async fn send_request(stream: &mut TcpStream, req: Request) -> anyhow::Result<()> {
    let req_bytes = serde_json::to_vec(&req)?;
    stream.write_u32(req_bytes.len() as u32).await?;
    stream.write_all(&req_bytes).await?;
    Ok(())
}

async fn read_response(stream: &mut TcpStream) -> anyhow::Result<Response> {
    let len = stream.read_u32().await? as usize;
    let mut buf = vec![0; len];
    stream.read_exact(&mut buf).await?;
    let resp: Response = serde_json::from_slice(&buf)?;
    Ok(resp)
}
