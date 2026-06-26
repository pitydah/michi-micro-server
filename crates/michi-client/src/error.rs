use serde::Deserialize;

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("http {status}: {message}")]
    Http { status: u16, message: String },

    #[error("v1 error [{code}]: {message}")]
    V1 { code: String, message: String },

    #[error("invalid response: {0}")]
    InvalidResponse(String),

    #[error("api version mismatch: expected v1, got {0}")]
    ApiVersionMismatch(String),

    #[error("authentication required")]
    AuthRequired,

    #[error("timeout")]
    Timeout,
}

#[derive(Debug, Deserialize)]
pub struct V1ErrorBody {
    pub error: V1ErrorPayload,
}

#[derive(Debug, Deserialize)]
pub struct V1ErrorPayload {
    pub code: String,
    pub message: String,
}

impl V1ErrorBody {
    pub fn code(&self) -> &str {
        &self.error.code
    }

    pub fn message(&self) -> &str {
        &self.error.message
    }
}
