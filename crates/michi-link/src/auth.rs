use sha2::{Digest, Sha256};
use sqlx::Row;
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

/// Thread-safe token store.
///
/// Tokens are stored in memory hashed with SHA-256.
/// A companion `load_from_db()` function can be called at startup
/// to re-populate from the database after a restart.
#[derive(Debug, Clone)]
pub struct TokenStore {
    by_hash: Arc<RwLock<HashMap<String, TokenEntry>>>,
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
            expires_at: chrono::Utc::now() + chrono::Duration::days(7),
        };
        self.by_hash.write().await.insert(hash, entry);
    }

    /// Insert a pre-hashed token entry directly (used for DB restoration).
    pub async fn store_hash(&self, hash: String, token_type: TokenType, device_id: uuid::Uuid) {
        let entry = TokenEntry {
            device_id,
            token_type,
            expires_at: chrono::Utc::now() + chrono::Duration::days(7),
        };
        self.by_hash.write().await.insert(hash, entry);
    }

    pub async fn validate(
        &self,
        token: &str,
        expected_type: TokenType,
    ) -> Result<uuid::Uuid, LinkError> {
        let hash = hash_token(token);
        let store = self.by_hash.read().await;
        match store.get(&hash) {
            Some(entry) if entry.token_type != expected_type => Err(LinkError::InvalidToken),
            Some(entry) if entry.expires_at > chrono::Utc::now() => Ok(entry.device_id),
            Some(_) => Err(LinkError::TokenExpired),
            None => Err(LinkError::InvalidToken),
        }
    }

    /// Check if a token hash exists in the store (for DB restoration).
    pub async fn has_hash(&self, hash: &str) -> bool {
        self.by_hash.read().await.contains_key(hash)
    }

    pub async fn revoke(&self, token: &str) {
        let hash = hash_token(token);
        self.by_hash.write().await.remove(&hash);
    }

    pub async fn revoke_by_hash(&self, hash: &str) {
        self.by_hash.write().await.remove(hash);
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

/// Load all non-revoked device token hashes from DB into the TokenStore.
/// This ensures tokens survive server restarts.
pub async fn load_tokens_from_db(
    store: &TokenStore,
    pool: &sqlx::SqlitePool,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query("SELECT device_id, token_hash FROM link_devices WHERE revoked = 0")
        .fetch_all(pool)
        .await?;

    let mut count = 0;
    for row in rows {
        let device_id: uuid::Uuid = row.get(0);
        let token_hash: String = row.get(1);
        if !store.has_hash(&token_hash).await {
            store
                .store_hash(token_hash, TokenType::Device, device_id)
                .await;
            count += 1;
        }
    }
    Ok(count)
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
