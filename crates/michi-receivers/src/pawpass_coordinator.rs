use crate::authority_models::{AuthorityError, AuthorityStamp};
use crate::session_manager::ReceiverSessionManager;
use michi_identity::IdentityManager;
use michi_sync::playback_transfer::{ContentRefV1, TailSyncV1};
use reqwest::Client;
use std::sync::Arc;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum PawPassError {
    #[error("Receiver not found: {0}")]
    ReceiverNotFound(String),
    #[error("Receiver not paired: {0}")]
    ReceiverNotPaired(String),
    #[error("No active authority grant for receiver: {0}")]
    NoActiveAuthority(String),
    #[error("Target error: {0}")]
    Target(String),
    #[error("Target proof invalid: {0}")]
    InvalidTargetProof(String),
    #[error("Authority error: {0}")]
    Authority(#[from] AuthorityError),
    #[error("Http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Protocol error: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TransferReceipt {
    pub transfer_id: String,
    pub target_michi_id: String,
    pub receiver_michi_id: String,
    pub status: String,
}

pub struct PawPassCoordinator {
    identity: Arc<IdentityManager>,
    receiver_manager: ReceiverSessionManager,
    http: Client,
}

impl PawPassCoordinator {
    pub fn new(identity: Arc<IdentityManager>, receiver_manager: ReceiverSessionManager) -> Self {
        Self {
            identity,
            receiver_manager,
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Orchestrates PawPass playback transfer from Micro (source) to target peer.
    #[allow(clippy::too_many_arguments)]
    pub async fn transfer_to(
        &self,
        target_michi_id: &str,
        receiver_id: &str,
        target_base_url: &str,
        current_track: ContentRefV1,
        queue: Vec<ContentRefV1>,
        queue_index: usize,
        position_ms: u64,
        is_playing: bool,
    ) -> Result<TransferReceipt, PawPassError> {
        let entry = {
            let reg_arc = self.receiver_manager.registry().await;
            let reg = reg_arc.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| PawPassError::ReceiverNotFound(receiver_id.to_string()))?;

        if !entry.paired {
            return Err(PawPassError::ReceiverNotPaired(receiver_id.to_string()));
        }

        let receiver_michi_id = entry
            .michi_id
            .clone()
            .unwrap_or_else(|| receiver_id.to_string());
        let transfer_id = format!("pawpass-{}", uuid::Uuid::new_v4());
        let nonce = format!("{}", rand::random::<u64>());

        let tailsync = TailSyncV1 {
            transfer_id: transfer_id.clone(),
            source_michi_id: self.identity.michi_id().to_string(),
            target_michi_id: target_michi_id.to_string(),
            receiver_michi_id: receiver_michi_id.clone(),
            status: if is_playing { "playing" } else { "paused" }.to_string(),
            position_ms,
            current_track,
            queue,
            queue_index,
            repeat_mode: "off".to_string(),
            shuffle: false,
            timestamp_ms: chrono::Utc::now().timestamp_millis(),
            source_revision: 1,
        };

        // 1. POST target /playback-transfer/prepare
        let prepare_url = format!(
            "{}/api/v1/playback-transfer/prepare",
            target_base_url.trim_end_matches('/')
        );
        let prepare_body = serde_json::json!({
            "transfer_id": transfer_id,
            "source_michi_id": self.identity.michi_id(),
            "target_michi_id": target_michi_id,
            "receiver_michi_id": receiver_michi_id,
            "tailsync": tailsync,
            "nonce": nonce,
        });

        let prepare_resp = self
            .http
            .post(&prepare_url)
            .json(&prepare_body)
            .send()
            .await?;

        if !prepare_resp.status().is_success() {
            let err_txt = prepare_resp.text().await.unwrap_or_default();
            return Err(PawPassError::Target(format!("prepare failed: {err_txt}")));
        }

        let prepare_data: serde_json::Value = prepare_resp.json().await?;
        let ready_proof = prepare_data
            .get("target_ready_proof")
            .and_then(|v| v.as_str())
            .ok_or_else(|| PawPassError::Protocol("missing target_ready_proof".into()))?;

        // 2. Fetch current active authority grant
        let current_grant = self
            .receiver_manager
            .authority_gate()
            .get_grant(receiver_id)
            .await
            .ok_or_else(|| PawPassError::NoActiveAuthority(receiver_id.to_string()))?;

        let stamp = AuthorityStamp {
            authority_instance_id: current_grant.authority_instance_id,
            lease_epoch: current_grant.lease_epoch,
        };

        // 3. Perform handoff with Stream authority
        let receiver_url = Url::parse(&entry.base_url)
            .map_err(|e| PawPassError::Protocol(format!("invalid receiver url: {e}")))?;

        let target_grant = self
            .receiver_manager
            .authority_gate()
            .client()
            .handoff(
                &receiver_url,
                entry.token.as_deref(),
                target_michi_id,
                ready_proof,
                &stamp,
            )
            .await?;

        // 4. POST target /playback-transfer/commit
        let commit_url = format!(
            "{}/api/v1/playback-transfer/commit",
            target_base_url.trim_end_matches('/')
        );
        let commit_body = serde_json::json!({
            "transfer_id": transfer_id,
            "grant": target_grant,
        });

        let commit_resp = self
            .http
            .post(&commit_url)
            .json(&commit_body)
            .send()
            .await?;

        if !commit_resp.status().is_success() {
            let err_txt = commit_resp.text().await.unwrap_or_default();
            // Ownership moved to target on Stream, stop local session immediately
            let _ = self.receiver_manager.stop_session(receiver_id).await;
            return Err(PawPassError::Target(format!("commit failed: {err_txt}")));
        }

        // 5. Success: stop local session
        let _ = self.receiver_manager.stop_session(receiver_id).await;

        Ok(TransferReceipt {
            transfer_id,
            target_michi_id: target_michi_id.to_string(),
            receiver_michi_id,
            status: "committed".to_string(),
        })
    }
}
