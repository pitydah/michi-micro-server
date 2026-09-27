pub mod authority_client;
pub mod authority_gate;
pub mod authority_models;
pub mod client;
pub mod credentials;
pub mod discovery_bridge;
pub mod models;
pub mod pawpass_coordinator;
pub mod session_manager;
pub mod session_supervisor;
pub mod transport;

pub use discovery_bridge::*;

pub use authority_client::ReceiverAuthorityClient;
pub use authority_gate::AuthorityGate;
pub use authority_models::*;
pub use pawpass_coordinator::*;

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
