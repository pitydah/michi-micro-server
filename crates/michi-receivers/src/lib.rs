pub mod client;
pub mod credentials;
pub mod models;
pub mod session_manager;
pub mod transport;

use async_trait::async_trait;

#[async_trait]
pub trait ReceiverAdapter: Send + Sync {
    async fn capabilities(&self) -> models::ReceiverCapabilities;
    async fn play(&self, request: models::PlayRequest) -> Result<(), String>;
    async fn pause(&self) -> Result<(), String>;
    async fn stop(&self) -> Result<(), String>;
    async fn set_volume(&self, volume: u8) -> Result<(), String>;
    async fn position(&self) -> Result<models::PlaybackPosition, String>;
}

pub use client::ReceiverClient;
pub use credentials::ReceiverCredentialStore;
pub use models::*;
pub use session_manager::ReceiverSessionManager;
