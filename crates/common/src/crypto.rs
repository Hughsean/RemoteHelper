use aes_gcm::{
    Aes256Gcm,
    aead::consts::U12,
    aead::generic_array::GenericArray,
    aead::{Aead, KeyInit},
};
use anyhow::{Result, anyhow};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, X25519_BASEPOINT_BYTES, x25519};

type AeadNonce = GenericArray<u8, U12>;

pub struct EphemeralSecret([u8; 32]);

pub struct SharedSecret([u8; 32]);

impl EphemeralSecret {
    pub fn diffie_hellman(self, their_public: &PublicKey) -> SharedSecret {
        SharedSecret(x25519(self.0, their_public.to_bytes()))
    }
}

impl SharedSecret {
    pub fn to_bytes(&self) -> [u8; 32] {
        self.0
    }
}

impl Drop for EphemeralSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

impl Drop for SharedSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

pub struct CryptoSession {
    cipher: Aes256Gcm,
    // We use a simple counter for nonce.
    // AES-GCM nonce is 12 bytes (96 bits).
    // We'll use 4 bytes of fixed prefix (derived from handshake) + 8 bytes counter.
    // Or just 12 bytes counter if keys are unique.
    // Since we generate a fresh key every session, starting nonce at 0 is fine.
    // We need separate counters for read and write to avoid reuse if same key is used (which it is).
    // Actually, usually we derive two keys: client_write_key and server_write_key.
    // For simplicity, let's derive ONE key, but use different Nonce prefixes?
    // Or just use different keys.
    write_nonce: u64,
    read_nonce: u64,
    is_server: bool,
}

impl CryptoSession {
    pub fn new(shared_secret: [u8; 32], is_server: bool) -> Self {
        // Derive session key from shared secret
        // Simple: SHA256(secret) -> 32 bytes
        let key_bytes: [u8; 32] = Sha256::digest(shared_secret).into();
        let cipher = Aes256Gcm::new_from_slice(&key_bytes)
            .expect("SHA256 output length for AES-256 key must be 32 bytes");

        // Start nonce at 0. Each session has unique shared_secret from X25519 ECDH,
        // so nonce reuse across different connections is not a concern.
        // We use direction-aware nonce generation (MSB bit) to separate
        // server/client streams, preventing collisions within same session.
        Self {
            cipher,
            write_nonce: 0,
            read_nonce: 0,
            is_server,
        }
    }

    fn get_nonce(counter: u64, is_server_sender: bool) -> AeadNonce {
        let mut bytes = [0u8; 12];
        // High bit of first byte indicates sender direction to avoid collision
        // if we used the same key.
        // Server sends with MSB 1, Client with MSB 0.
        if is_server_sender {
            bytes[0] |= 0x80;
        }

        // Put counter in last 8 bytes (big endian)
        let counter_bytes = counter.to_be_bytes();
        bytes[4..12].copy_from_slice(&counter_bytes);

        GenericArray::clone_from_slice(&bytes)
    }

    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let nonce = Self::get_nonce(self.write_nonce, self.is_server);
        self.write_nonce += 1;

        let ciphertext = self
            .cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow!("Encryption failed: {}", e))?;
        Ok(ciphertext)
    }

    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        // When decrypting, the sender is the opposite of us
        let nonce = Self::get_nonce(self.read_nonce, !self.is_server);
        self.read_nonce += 1;

        let plaintext = self
            .cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| anyhow!("Decryption failed: {}", e))?;
        Ok(plaintext)
    }
}

pub fn generate_ephemeral() -> (EphemeralSecret, PublicKey) {
    let secret = EphemeralSecret(rand::random::<[u8; 32]>());
    let public = PublicKey::from(x25519(secret.0, X25519_BASEPOINT_BYTES));
    (secret, public)
}
