use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use rand::RngCore;
use tracing::{info, warn};

#[derive(Debug, Clone)]
pub struct ReceiverCredentialStore {
    key: [u8; 32],
}

impl ReceiverCredentialStore {
    pub fn new(key: [u8; 32]) -> Self {
        Self { key }
    }

    pub fn random() -> Self {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        Self { key }
    }

    pub fn load_or_create_key(key_path: &Path) -> Result<Self, String> {
        if key_path.exists() {
            let mut file = File::open(key_path)
                .map_err(|e| format!("failed to open receiver key file: {e}"))?;
            let mut key = [0u8; 32];
            file.read_exact(&mut key)
                .map_err(|e| format!("failed to read 32-byte receiver key: {e}"))?;
            Ok(Self::new(key))
        } else {
            if let Some(parent) = key_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let mut key = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut key);

            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(key_path)
                    .map_err(|e| format!("failed to create receiver key file with 0600: {e}"))?;
                file.write_all(&key)
                    .map_err(|e| format!("failed to write receiver key: {e}"))?;
                file.flush().map_err(|e| e.to_string())?;
            }

            #[cfg(not(unix))]
            {
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(key_path)
                    .map_err(|e| format!("failed to create receiver key file: {e}"))?;
                file.write_all(&key)
                    .map_err(|e| format!("failed to write receiver key: {e}"))?;
                file.flush().map_err(|e| e.to_string())?;
            }

            info!(
                "generated persistent receiver credential encryption key at {:?}",
                key_path
            );
            Ok(Self::new(key))
        }
    }

    pub fn encrypt_token(
        &self,
        receiver_id: &str,
        plaintext_token: &str,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        let cipher_key = Key::from_slice(&self.key);
        let cipher = ChaCha20Poly1305::new(cipher_key);

        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let payload = Payload {
            msg: plaintext_token.as_bytes(),
            aad: receiver_id.as_bytes(),
        };

        let ciphertext = cipher
            .encrypt(nonce, payload)
            .map_err(|e| format!("encryption failure: {e:?}"))?;

        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    pub fn decrypt_token(
        &self,
        receiver_id: &str,
        ciphertext: &[u8],
        nonce_bytes: &[u8],
    ) -> Result<String, String> {
        if nonce_bytes.len() != 12 {
            return Err("invalid nonce length: expected 12 bytes".to_string());
        }

        let cipher_key = Key::from_slice(&self.key);
        let cipher = ChaCha20Poly1305::new(cipher_key);
        let nonce = Nonce::from_slice(nonce_bytes);

        let payload = Payload {
            msg: ciphertext,
            aad: receiver_id.as_bytes(),
        };

        let decrypted = cipher.decrypt(nonce, payload).map_err(|_| {
            warn!(
                "credential decryption authentication failed for receiver '{}'",
                receiver_id
            );
            "credential decryption failed (MAC mismatch or wrong key)".to_string()
        })?;

        String::from_utf8(decrypted).map_err(|e| format!("invalid UTF-8 in decrypted token: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let store = ReceiverCredentialStore::new([42u8; 32]);
        let receiver_id = "stream-living-room-01";
        let token = "secret_bearer_token_xyz_12345";

        let (ct, nonce) = store.encrypt_token(receiver_id, token).unwrap();
        let pt = store.decrypt_token(receiver_id, &ct, &nonce).unwrap();
        assert_eq!(pt, token);

        // AAD mismatch fails closed
        let fail_aad = store.decrypt_token("other-receiver", &ct, &nonce);
        assert!(fail_aad.is_err());

        // Ciphertext corruption fails closed
        let mut corrupted = ct.clone();
        corrupted[0] ^= 0xFF;
        let fail_ct = store.decrypt_token(receiver_id, &corrupted, &nonce);
        assert!(fail_ct.is_err());
    }
}
