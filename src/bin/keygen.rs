use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::prelude::*;
use ed25519_dalek::SigningKey;
use hmac::Hmac;
use pbkdf2::pbkdf2;
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs::File;
use std::io::{self, Write};

#[derive(Serialize, Deserialize)]
struct KeyFile {
    pub_key: String,
    enc_priv_key: String,
    salt: String,
    nonce: String,
}

fn main() -> anyhow::Result<()> {
    println!("Generating Ed25519 Keypair...");
    let mut csprng = OsRng;
    let signing_key = SigningKey::generate(&mut csprng);
    let verifying_key = signing_key.verifying_key();

    let pub_key_b64 = BASE64_STANDARD.encode(verifying_key.to_bytes());
    println!("Public Key: {}", pub_key_b64);

    print!("Enter passphrase to encrypt private key: ");
    io::stdout().flush()?;
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase)?;
    let passphrase = passphrase.trim();

    // Derive key from passphrase
    let salt: [u8; 16] = rand::random();
    let mut key = [0u8; 32];
    pbkdf2::<Hmac<Sha256>>(passphrase.as_bytes(), &salt, 100_000, &mut key)
        .expect("HMAC can be initialized with any key length");

    // Encrypt private key
    let cipher = Aes256Gcm::new(&key.into());
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let priv_key_bytes = signing_key.to_bytes();

    let ciphertext = cipher
        .encrypt(nonce, priv_key_bytes.as_ref())
        .map_err(|e| anyhow::anyhow!("Encryption failure: {}", e))?;

    let key_file = KeyFile {
        pub_key: pub_key_b64.clone(),
        enc_priv_key: BASE64_STANDARD.encode(ciphertext),
        salt: BASE64_STANDARD.encode(salt),
        nonce: BASE64_STANDARD.encode(nonce_bytes),
    };

    // Save to user home directory
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Failed to locate home directory"))?;
    let key_path = home_dir.join("id_ed25519.json");

    let file = File::create(&key_path)?;
    serde_json::to_writer_pretty(file, &key_file)?;

    println!("\nKeypair saved to '{}'.", key_path.display());
    println!("Add the Public Key to your server's config.toml 'authorized_keys' list.");

    Ok(())
}
