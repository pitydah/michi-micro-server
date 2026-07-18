//! Michi Identity — Identidad persistente criptográfica estilo Apple
//!
//! Al primer inicio genera un par de claves Ed25519 y lo persiste en disco.
//! El `michi-id` es el hash público derivado de la clave pública.
//! Nunca depende de la IP para la identidad.

use base64::Engine;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

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
}

const KEY_FILE_NAME: &str = "identity.key";

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

impl MichiIdentity {
    /// Load or create identity from `data_dir/identity.key`
    pub async fn load_or_create(data_dir: &std::path::Path) -> Result<Self, IdentityError> {
        tokio::fs::create_dir_all(data_dir).await?;
        let key_path = data_dir.join(KEY_FILE_NAME);

        if key_path.exists() {
            let bytes = tokio::fs::read(&key_path).await?;
            let key_str = String::from_utf8(bytes)
                .map_err(|_| IdentityError::InvalidKey("not valid UTF-8".into()))?;
            let decoded = BASE64
                .decode(key_str.trim().as_bytes())
                .map_err(|e| IdentityError::InvalidKey(format!("base64: {}", e)))?;
            let arr: [u8; 64] = decoded
                .as_slice()
                .try_into()
                .map_err(|_| IdentityError::InvalidKey("key length invalid".into()))?;
            let signing_key = SigningKey::from_keypair_bytes(&arr)
                .map_err(|e| IdentityError::InvalidKey(format!("dalek: {}", e)))?;
            let verifying_key = signing_key.verifying_key();
            let michi_id = hash_public_key(&verifying_key);

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
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        let michi_id = hash_public_key(&verifying_key);

        let encoded = BASE64.encode(signing_key.to_keypair_bytes());
        tokio::fs::write(&key_path, encoded.as_bytes()).await?;
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

    /// Get the public michi-id (SHA-256 of public key, hex encoded)
    pub async fn get_id(&self) -> String {
        self.inner.read().await.michi_id.clone()
    }

    /// Get the verifying key bytes (for QR / pairing)
    pub async fn public_key_bytes(&self) -> Vec<u8> {
        self.inner.read().await.verifying_key.to_bytes().to_vec()
    }

    /// Sign a payload (returns base64-encoded signature)
    pub async fn sign_payload(&self, payload: &[u8]) -> String {
        let inner = self.inner.read().await;
        let signature: Signature = inner.signing_key.sign(payload);
        BASE64.encode(signature.to_bytes())
    }

    /// Verify a peer's signature against their public key
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

    /// Sign and return a signed identity packet: { michi_id, public_key, signature }
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
        assert_eq!(id1.len(), 64, "michi_id should be 64 hex chars");

        // Load again — should match
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
        assert!(valid, "signature must verify");

        let invalid = MichiIdentity::verify_peer(&pub_key, b"tampered", &signature).unwrap();
        assert!(!invalid, "tampered payload must not verify");
    }

    #[tokio::test]
    async fn test_signed_packet() {
        let dir = tempdir().unwrap();
        let identity = MichiIdentity::load_or_create(dir.path()).await.unwrap();
        let packet = identity.signed_packet().await;
        assert!(packet["michi_id"].as_str().unwrap().len() == 64);
        assert!(!packet["public_key"].as_str().unwrap().is_empty());
        assert!(!packet["signature"].as_str().unwrap().is_empty());
    }
}
