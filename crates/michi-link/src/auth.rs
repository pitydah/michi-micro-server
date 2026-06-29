use sha2::{Sha256, Digest};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::errors::LinkError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Device,
    Refresh,
}

#[derive(Debug, Clone)]
pub struct TokenStore {
    by_hash: Arc<RwLock<HashMap<String, TokenEntry>>>,
}

#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub device_id: uuid::Uuid,
    pub token_type: TokenType,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

pub fn hash_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            by_hash: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn store(&self, token: &str, token_type: TokenType, device_id: uuid::Uuid) {
        let hash = hash_token(token);
        let entry = TokenEntry {
            device_id,
            token_type,
            expires_at: chrono::Utc::now() + chrono::Duration::days(90),
        };
        self.by_hash.write().await.insert(hash, entry);
    }

    pub async fn validate(&self, token: &str, expected_type: TokenType) -> Result<uuid::Uuid, LinkError> {
        let hash = hash_token(token);
        let store = self.by_hash.read().await;
        match store.get(&hash) {
            Some(entry) if entry.token_type != expected_type => Err(LinkError::InvalidToken),
            Some(entry) if entry.expires_at > chrono::Utc::now() => Ok(entry.device_id),
            Some(_) => Err(LinkError::TokenExpired),
            None => Err(LinkError::InvalidToken),
        }
    }

    pub async fn revoke(&self, token: &str) {
        let hash = hash_token(token);
        self.by_hash.write().await.remove(&hash);
    }

    pub async fn revoke_all_by_device(&self, device_id: uuid::Uuid) {
        let mut store = self.by_hash.write().await;
        store.retain(|_, entry| entry.device_id != device_id);
    }

    pub async fn cleanup(&self) {
        let mut store = self.by_hash.write().await;
        store.retain(|_, entry| entry.expires_at > chrono::Utc::now());
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

pub fn spawn_token_cleanup(store: TokenStore) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            store.cleanup().await;
        }
    });
}
