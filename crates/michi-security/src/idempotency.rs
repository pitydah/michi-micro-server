//! Idempotency store for Michi Link API.
//!
//! Stores responses for POST/PUT/DELETE requests keyed by `Idempotency-Key` header.
//! Entries expire after 1 hour.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const IDEMPOTENCY_TTL: Duration = Duration::from_secs(3600); // 1 hora

#[derive(Debug)]
struct IdempotencyEntry {
    response: serde_json::Value,
    method: String,
    path: String,
    created_at: Instant,
}

/// Thread-safe idempotency store.
#[derive(Debug, Clone)]
pub struct IdempotencyStore {
    store: Arc<Mutex<HashMap<String, IdempotencyEntry>>>,
}

impl IdempotencyStore {
    /// Create a new empty store.
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get a cached response by key.
    /// Returns None if key not found or expired.
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        let mut store = self.store.lock().unwrap();
        // Clean expired entries
        store.retain(|_, entry| entry.created_at.elapsed() < IDEMPOTENCY_TTL);

        store.get(key).map(|entry| entry.response.clone())
    }

    /// Check if a key exists (without returning the response).
    pub fn exists(&self, key: &str) -> bool {
        let store = self.store.lock().unwrap();
        store.contains_key(key)
    }

    /// Store a response by key.
    pub fn set(&self, key: &str, response: &serde_json::Value, method: &str, path: &str) {
        let mut store = self.store.lock().unwrap();
        store.insert(
            key.to_string(),
            IdempotencyEntry {
                response: response.clone(),
                method: method.to_string(),
                path: path.to_string(),
                created_at: Instant::now(),
            },
        );
    }

    /// Check if a key was used with a different method/path (key reuse conflict).
    pub fn check_reuse(&self, key: &str, method: &str, path: &str) -> Option<(String, String)> {
        let store = self.store.lock().unwrap();
        store.get(key).and_then(|entry| {
            if entry.method != method || entry.path != path {
                Some((entry.method.clone(), entry.path.clone()))
            } else {
                None
            }
        })
    }

    /// Remove all expired entries and return count.
    pub fn clean_expired(&self) -> usize {
        let mut store = self.store.lock().unwrap();
        let before = store.len();
        store.retain(|_, entry| entry.created_at.elapsed() < IDEMPOTENCY_TTL);
        before - store.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idempotency_basic() {
        let store = IdempotencyStore::new();
        let key = "test-key";
        let response = serde_json::json!({"status": "ok"});

        assert!(store.get(key).is_none());
        store.set(key, &response, "POST", "/api/v1/test");
        assert!(store.get(key).is_some());
    }

    #[test]
    fn test_idempotency_reuse_detection() {
        let store = IdempotencyStore::new();
        let key = "key-1";
        let response = serde_json::json!({"status": "ok"});

        store.set(key, &response, "POST", "/api/v1/upload");
        let conflict = store.check_reuse(key, "POST", "/api/v1/other");
        assert!(conflict.is_some());

        let no_conflict = store.check_reuse(key, "POST", "/api/v1/upload");
        assert!(no_conflict.is_none());
    }

    #[test]
    fn test_clean_expired() {
        let store = IdempotencyStore::new();
        store.set("k1", &serde_json::json!({}), "POST", "/v1/a");
        store.set("k2", &serde_json::json!({}), "POST", "/v1/b");
        assert_eq!(store.clean_expired(), 0); // none expired yet
    }
}
