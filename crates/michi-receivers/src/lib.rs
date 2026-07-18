pub mod client;
pub mod models;
pub mod session_manager;

use async_trait::async_trait;
use models::{PlayRequest, PlaybackPosition, ReceiverCapabilities};

#[async_trait]
pub trait ReceiverAdapter: Send + Sync {
    async fn capabilities(&self) -> ReceiverCapabilities;
    async fn play(&self, request: PlayRequest) -> Result<(), String>;
    async fn pause(&self) -> Result<(), String>;
    async fn stop(&self) -> Result<(), String>;
    async fn set_volume(&self, volume: u8) -> Result<(), String>;
    async fn position(&self) -> Result<PlaybackPosition, String>;
}

pub use client::ReceiverClient;
pub use models::*;
pub use session_manager::ReceiverSessionManager;
