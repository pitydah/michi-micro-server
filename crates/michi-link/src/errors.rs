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
