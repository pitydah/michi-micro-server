use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Identifies an authority instance and lease generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityStamp {
    pub authority_instance_id: String,
    pub lease_epoch: u64,
}

/// Short-lived cryptographic grant allowing session creation on a receiver.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityGrant {
    pub authority_instance_id: String,
    pub lease_epoch: u64,
    pub grant_token: String,
    pub activation_expires_at: DateTime<Utc>,
}

impl std::fmt::Debug for AuthorityGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorityGrant")
            .field("authority_instance_id", &self.authority_instance_id)
            .field("lease_epoch", &self.lease_epoch)
            .field("grant_token", &"[REDACTED]")
            .field("activation_expires_at", &self.activation_expires_at)
            .finish()
    }
}

/// Remote Stream authority state advertised on /api/v1/authority/state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityState {
    pub profile: String,
    pub state: String,
    pub receiver_michi_id: String,
    pub authority_instance_id: String,
    pub lease_epoch: u64,
    #[serde(default)]
    pub owner_michi_id: Option<String>,
    #[serde(default)]
    pub owner_service: Option<String>,
    #[serde(default)]
    pub owner_name: Option<String>,
    #[serde(default)]
    pub active_session_id: Option<String>,
    #[serde(default)]
    pub lease_expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub revision: u64,
}

/// Capability announcement from /api/v1/authority/info.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityInfo {
    pub profile: String,
    pub version: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default = "default_activation_ttl")]
    pub default_activation_ttl_secs: u64,
}

fn default_activation_ttl() -> u64 {
    15
}

/// Error taxonomy for Perch authority interactions.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum AuthorityError {
    #[error("Receiver occupied by {owner_name:?} ({owner_michi_id:?})")]
    Occupied {
        owner_michi_id: Option<String>,
        owner_name: Option<String>,
    },
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    #[error("Epoch mismatch: expected {expected}, actual {actual}")]
    EpochMismatch { expected: u64, actual: u64 },
    #[error("Activation grant expired")]
    GrantExpired,
    #[error("Receiver does not support authority-v1")]
    Unsupported,
    #[error("Explicit takeover flag required")]
    TakeoverRequiresExplicit,
    #[error("HTTP error: {0}")]
    Http(String),
    #[error("Protocol error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_redacts_token_in_debug() {
        let grant = AuthorityGrant {
            authority_instance_id: "inst-1".into(),
            lease_epoch: 42,
            grant_token: "super-secret-token-12345".into(),
            activation_expires_at: Utc::now(),
        };
        let formatted = format!("{grant:?}");
        assert!(!formatted.contains("super-secret-token-12345"));
        assert!(formatted.contains("[REDACTED]"));
    }
}
