use common::keys::encrypt_private_key;
use ed25519_dalek::SigningKey;
use std::fs::File;
use std::io::{self, Write};

fn main() -> anyhow::Result<()> {
    println!("Generating Ed25519 Keypair...");
    let signing_key_bytes: [u8; 32] = rand::random();
    let signing_key = SigningKey::from_bytes(&signing_key_bytes);

    print!("Enter passphrase to encrypt private key: ");
    io::stdout().flush()?;
    let mut passphrase = String::new();
    io::stdin().read_line(&mut passphrase)?;
    let passphrase = passphrase.trim();

    let key_file = encrypt_private_key(&signing_key, passphrase)
        .map_err(|e| anyhow::anyhow!("Encryption failure: {}", e))?;

    println!("Public Key: {}", key_file.pub_key);

    // 保存到用户主目录
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Failed to locate home directory"))?;
    let key_path = home_dir.join("id_ed25519.json");

    let file = File::create(&key_path)?;
    serde_json::to_writer_pretty(file, &key_file)?;

    println!("\nKeypair saved to '{}'.", key_path.display());
    println!("Add the Public Key to your server's config.toml 'authorized_keys' list.");

    Ok(())
}
