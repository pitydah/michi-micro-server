use thiserror::Error;

#[derive(Debug, Error)]
pub enum LinkError {
    #[error("invalid device token")]
    InvalidToken,
    #[error("device token expired")]
    TokenExpired,
    #[error("insufficient permissions: {0}")]
    InsufficientPermissions(String),
    #[error("pairing code not found or expired")]
    PairingCodeInvalid,
    #[error("pairing already confirmed")]
    PairingAlreadyConfirmed,
    #[error("device already paired")]
    DeviceAlreadyPaired,
    #[error("device not found")]
    DeviceNotFound,
    #[error("device revoked")]
    DeviceRevoked,
    #[error("session not found")]
    SessionNotFound,
    #[error("import session not found")]
    ImportSessionNotFound,
    #[error("import session expired")]
    ImportSessionExpired,
    #[error("duplicate track hash: {0}")]
    DuplicateTrack(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<michi_db::DbError> for LinkError {
    fn from(e: michi_db::DbError) -> Self {
        LinkError::Database(e.to_string())
    }
}

/// The 20 canonical error codes defined in Michi Link v1 `error.schema.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MichiLinkErrorCode {
    InvalidRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    InternalError,
    NotImplemented,
    PairingExpired,
    PairingAttemptsExceeded,
    PairingKeyMismatch,
    IdentityCorrupted,
    SignatureInvalid,
    ReplayDetected,
    IdempotencyKeyReuse,
    TrackNotFound,
    ImportSessionExpired,
    PairingNotFound,
    PairingAlreadyConsumed,
    PairingPinMismatch,
}

impl MichiLinkErrorCode {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::InvalidRequest => "INVALID_REQUEST",
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::RateLimited => "RATE_LIMITED",
            Self::InternalError => "INTERNAL_ERROR",
            Self::NotImplemented => "NOT_IMPLEMENTED",
            Self::PairingExpired => "PAIRING_EXPIRED",
            Self::PairingAttemptsExceeded => "PAIRING_ATTEMPTS_EXCEEDED",
            Self::PairingKeyMismatch => "PAIRING_KEY_MISMATCH",
            Self::IdentityCorrupted => "IDENTITY_CORRUPTED",
            Self::SignatureInvalid => "SIGNATURE_INVALID",
            Self::ReplayDetected => "REPLAY_DETECTED",
            Self::IdempotencyKeyReuse => "IDEMPOTENCY_KEY_REUSE",
            Self::TrackNotFound => "TRACK_NOT_FOUND",
            Self::ImportSessionExpired => "IMPORT_SESSION_EXPIRED",
            Self::PairingNotFound => "PAIRING_NOT_FOUND",
            Self::PairingAlreadyConsumed => "PAIRING_ALREADY_CONSUMED",
            Self::PairingPinMismatch => "PAIRING_PIN_MISMATCH",
        }
    }
}

impl std::fmt::Display for MichiLinkErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
