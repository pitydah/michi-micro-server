//! Michi Identity — Identidad persistente criptográfica estilo Apple
//!
//! Al primer inicio genera un par de claves Ed25519 y lo persiste en disco
//! cifrado con ChaCha20-Poly1305 (AEAD). La clave de cifrado se deriva de
//! blake3(hostname + salt) para evitar que una copia del archivo sea usable
//! directamente en otra máquina.

use base64::Engine;
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng as RandOsRng;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use zeroize::Zeroize;

const KEY_FILE_NAME: &str = "identity.msgpack";
const AEAD_CONTEXT: &[u8] = b"michi-identity-aead-v1";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12; // ChaCha20Poly1305 standard nonce

#[derive(Error, Debug)]
pub enum IdentityError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("Invalid key file: {0}")]
    InvalidKey(String),
    #[error("Signature error: {0}")]
    Signature(String),
    #[error("AEAD decryption failed")]
    DecryptionFailed,
    #[error("AEAD encryption failed")]
    EncryptionFailed,
    #[error("Base64 error: {0}")]
    Base64(#[from] base64::DecodeError),
}

/// Encrypted format on disk (MessagePack)
#[derive(serde::Serialize, serde::Deserialize)]
struct EncryptedKeyFile {
    version: u32,
    algorithm: String,
    salt: [u8; 16],
    nonce: [u8; NONCE_LEN],
    /// ChaCha20-Poly1305 encrypted ciphertext of the 64-byte keypair
    ciphertext: Vec<u8>,
}

#[derive(Clone)]
pub struct MichiIdentity {
    inner: Arc<RwLock<IdentityInner>>,
}

struct IdentityInner {
    signing_key: SigningKey,
    verifying_key: VerifyingKey,
    michi_id: String,
    #[allow(dead_code)]
    key_path: PathBuf,
}

impl Drop for IdentityInner {
    fn drop(&mut self) {
        // Zeroize secret key material on drop
        let mut sk_bytes = self.signing_key.to_bytes();
        sk_bytes.zeroize();
    }
}

impl MichiIdentity {
    pub async fn load_or_create(data_dir: &std::path::Path) -> Result<Self, IdentityError> {
        tokio::fs::create_dir_all(data_dir).await?;
        let key_path = data_dir.join(KEY_FILE_NAME);

        if key_path.exists() {
            let data = tokio::fs::read(&key_path).await?;
            let file: EncryptedKeyFile = rmp_serde::from_slice(&data)
                .map_err(|e| IdentityError::InvalidKey(format!("msgpack: {}", e)))?;

            let wrap_key = Self::derive_wrap_key(&file.salt);
            let cipher = ChaCha20Poly1305::new(Key::from_slice(&wrap_key));
            let nonce = Nonce::from_slice(&file.nonce);

            let plaintext = cipher
                .decrypt(nonce, file.ciphertext.as_ref())
                .map_err(|_| IdentityError::DecryptionFailed)?;

            let arr: [u8; 64] = plaintext
                .as_slice()
                .try_into()
                .map_err(|_| IdentityError::InvalidKey("key length invalid".into()))?;
            let signing_key = SigningKey::from_keypair_bytes(&arr)
                .map_err(|e| IdentityError::InvalidKey(format!("dalek: {}", e)))?;
            let verifying_key = signing_key.verifying_key();
            let michi_id = hash_public_key(&verifying_key);

            // Zeroize plaintext after use
            let mut pt = plaintext;
            pt.zeroize();

            tracing::info!("identity loaded: michi_id={}", &michi_id[..12]);

            return Ok(Self {
                inner: Arc::new(RwLock::new(IdentityInner {
                    signing_key,
                    verifying_key,
                    michi_id,
                    key_path,
                })),
            });
        }

        // Generate new key
        let mut csprng = RandOsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let michi_id = hash_public_key(&verifying_key);

        let keypair_bytes = signing_key.to_keypair_bytes();
        let salt: [u8; 16] = rand::random();
        let nonce: [u8; NONCE_LEN] = rand::random();
        let wrap_key = Self::derive_wrap_key(&salt);
        let cipher = ChaCha20Poly1305::new(Key::from_slice(&wrap_key));
        let nonce_aead = Nonce::from_slice(&nonce);

        let ciphertext = cipher
            .encrypt(nonce_aead, keypair_bytes.as_ref())
            .map_err(|_| IdentityError::EncryptionFailed)?;

        // Zeroize keypair bytes after encryption
        let mut kb = keypair_bytes;
        kb.zeroize();

        let file = EncryptedKeyFile {
            version: 1,
            algorithm: "ed25519+chacha20-poly1305".into(),
            salt,
            nonce,
            ciphertext,
        };

        let encoded = rmp_serde::to_vec(&file)
            .map_err(|e| IdentityError::InvalidKey(format!("msgpack encode: {}", e)))?;
        tokio::fs::write(&key_path, &encoded).await?;

        // Set file permissions to 600 (owner only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600))?;
        }

        tracing::info!("identity created: michi_id={}", &michi_id[..12]);

        Ok(Self {
            inner: Arc::new(RwLock::new(IdentityInner {
                signing_key,
                verifying_key,
                michi_id,
                key_path,
            })),
        })
    }

    fn derive_wrap_key(salt: &[u8; 16]) -> [u8; KEY_LEN] {
        let hostname = gethostname::gethostname().to_string_lossy().to_string();
        let mut hasher = blake3::Hasher::new();
        hasher.update(AEAD_CONTEXT);
        hasher.update(hostname.as_bytes());
        hasher.update(salt);
        *hasher.finalize().as_bytes()
    }

    pub async fn get_id(&self) -> String {
        self.inner.read().await.michi_id.clone()
    }

    pub async fn public_key_bytes(&self) -> Vec<u8> {
        self.inner.read().await.verifying_key.to_bytes().to_vec()
    }

    pub async fn sign_payload(&self, payload: &[u8]) -> String {
        let inner = self.inner.read().await;
        let signature: Signature = inner.signing_key.sign(payload);
        BASE64.encode(signature.to_bytes())
    }

    pub fn verify_peer(
        peer_public_key: &[u8],
        payload: &[u8],
        signature_b64: &str,
    ) -> Result<bool, IdentityError> {
        let arr: [u8; 32] = peer_public_key
            .try_into()
            .map_err(|_| IdentityError::Signature("invalid public key length".into()))?;
        let verifying_key = VerifyingKey::from_bytes(&arr)
            .map_err(|e| IdentityError::Signature(format!("invalid key: {}", e)))?;

        let sig_bytes = BASE64
            .decode(signature_b64.as_bytes())
            .map_err(|e| IdentityError::Signature(format!("base64: {}", e)))?;
        let sig_arr: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| IdentityError::Signature("invalid signature length".into()))?;
        let signature = Signature::from_bytes(&sig_arr);

        match verifying_key.verify(payload, &signature) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    pub async fn signed_packet(&self) -> serde_json::Value {
        let inner = self.inner.read().await;
        let payload = format!(
            "{}:{}",
            inner.michi_id,
            hex::encode(inner.verifying_key.to_bytes())
        );
        let signature = BASE64.encode(inner.signing_key.sign(payload.as_bytes()).to_bytes());
        serde_json::json!({
            "michi_id": inner.michi_id,
            "public_key": hex::encode(inner.verifying_key.to_bytes()),
            "signature": signature,
            "version": env!("CARGO_PKG_VERSION"),
        })
    }
}

fn hash_public_key(key: &VerifyingKey) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.to_bytes());
    hex::encode(hasher.finalize())
}

const BASE64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_create_and_load_identity() {
        let dir = tempdir().unwrap();
        let identity = MichiIdentity::load_or_create(dir.path()).await.unwrap();
        let id1 = identity.get_id().await;
        assert_eq!(id1.len(), 64);

        let identity2 = MichiIdentity::load_or_create(dir.path()).await.unwrap();
        let id2 = identity2.get_id().await;
        assert_eq!(id1, id2, "identity must persist across loads");
    }

    #[tokio::test]
    async fn test_sign_and_verify() {
        let dir = tempdir().unwrap();
        let identity = MichiIdentity::load_or_create(dir.path()).await.unwrap();
        let payload = b"hello michi";
        let signature = identity.sign_payload(payload).await;

        let pub_key = identity.public_key_bytes().await;
        let valid = MichiIdentity::verify_peer(&pub_key, payload, &signature).unwrap();
        assert!(valid);

        let invalid = MichiIdentity::verify_peer(&pub_key, b"tampered", &signature).unwrap();
        assert!(!invalid);
    }
}
