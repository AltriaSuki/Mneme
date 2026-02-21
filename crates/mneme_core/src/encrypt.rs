//! B-12: Memory encryption at rest.
//!
//! Provides ChaCha20-Poly1305 AEAD encryption for memory body text.
//! Key is held by the runtime process (env var or generated file).

use chacha20poly1305::{
    aead::{Aead, KeyInit, OsRng},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;

/// Encrypts/decrypts memory body text using ChaCha20-Poly1305.
#[derive(Clone)]
pub struct MemoryEncryptor {
    cipher: ChaCha20Poly1305,
}

impl MemoryEncryptor {
    /// Create from a 32-byte key.
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new(key.into()),
        }
    }

    /// Create from a base64-encoded key string.
    pub fn from_base64(key_b64: &str) -> anyhow::Result<Self> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD.decode(key_b64)?;
        if bytes.len() != 32 {
            anyhow::bail!("Encryption key must be 32 bytes, got {}", bytes.len());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self::new(&key))
    }

    /// Generate a new random key and return it as base64.
    pub fn generate_key_b64() -> String {
        use base64::Engine;
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        base64::engine::general_purpose::STANDARD.encode(key)
    }

    /// Encrypt plaintext. Returns base64(nonce || ciphertext).
    pub fn encrypt(&self, plaintext: &str) -> anyhow::Result<String> {
        use base64::Engine;
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        let mut combined = Vec::with_capacity(12 + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        Ok(base64::engine::general_purpose::STANDARD.encode(combined))
    }

    /// Decrypt base64(nonce || ciphertext) back to plaintext.
    pub fn decrypt(&self, encoded: &str) -> anyhow::Result<String> {
        use base64::Engine;
        let combined = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        if combined.len() < 13 {
            anyhow::bail!("Ciphertext too short");
        }

        let nonce = Nonce::from_slice(&combined[..12]);
        let ciphertext = &combined[12..];

        let plaintext = self
            .cipher
            .decrypt(nonce, ciphertext)
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        String::from_utf8(plaintext).map_err(Into::into)
    }
}

impl std::fmt::Debug for MemoryEncryptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryEncryptor").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip() {
        let key_b64 = MemoryEncryptor::generate_key_b64();
        let enc = MemoryEncryptor::from_base64(&key_b64).unwrap();
        let plaintext = "这是一段记忆内容";
        let encrypted = enc.encrypt(plaintext).unwrap();
        assert_ne!(encrypted, plaintext);
        let decrypted = enc.decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_different_nonces() {
        let key_b64 = MemoryEncryptor::generate_key_b64();
        let enc = MemoryEncryptor::from_base64(&key_b64).unwrap();
        let a = enc.encrypt("hello").unwrap();
        let b = enc.encrypt("hello").unwrap();
        // Same plaintext should produce different ciphertext (random nonce)
        assert_ne!(a, b);
    }

    #[test]
    fn test_wrong_key_fails() {
        let enc1 = MemoryEncryptor::from_base64(&MemoryEncryptor::generate_key_b64()).unwrap();
        let enc2 = MemoryEncryptor::from_base64(&MemoryEncryptor::generate_key_b64()).unwrap();
        let encrypted = enc1.encrypt("secret").unwrap();
        assert!(enc2.decrypt(&encrypted).is_err());
    }
}
