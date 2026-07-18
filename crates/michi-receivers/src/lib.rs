pub mod client;
pub mod models;
pub mod session_manager;

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
pub use models::*;
pub use session_manager::ReceiverSessionManager;
