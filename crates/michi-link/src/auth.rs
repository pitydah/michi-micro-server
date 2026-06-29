use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::device_registry::hash_token;
use crate::errors::LinkError;

#[derive(Debug, Clone)]
pub struct TokenStore {
    tokens: Arc<RwLock<HashMap<String, TokenEntry>>>,
}

#[derive(Debug, Clone)]
pub struct TokenEntry {
    pub device_id: uuid::Uuid,
    pub token_hash: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

impl TokenStore {
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn store(&self, device_token: &str, device_id: uuid::Uuid) {
        let hash = hash_token(device_token);
        let entry = TokenEntry {
            device_id,
            token_hash: hash,
            expires_at: chrono::Utc::now() + chrono::Duration::days(90),
        };
        self.tokens.write().await.insert(device_token.to_string(), entry);
    }

    pub async fn validate(&self, token: &str) -> Result<uuid::Uuid, LinkError> {
        let tokens = self.tokens.read().await;
        match tokens.get(token) {
            Some(entry) if entry.expires_at > chrono::Utc::now() => Ok(entry.device_id),
            Some(_) => Err(LinkError::TokenExpired),
            None => Err(LinkError::InvalidToken),
        }
    }

    pub async fn revoke(&self, token: &str) {
        self.tokens.write().await.remove(token);
    }

    pub async fn cleanup(&self) {
        let mut tokens = self.tokens.write().await;
        tokens.retain(|_, entry| entry.expires_at > chrono::Utc::now());
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
