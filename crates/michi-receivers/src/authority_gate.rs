use crate::authority_client::ReceiverAuthorityClient;
use crate::authority_models::{AuthorityError, AuthorityGrant, AuthorityStamp};
use crate::models::ReceiverRegistryEntry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
use url::Url;

/// Intercepts and coordinates Perch authority before starting a streaming session.
#[derive(Clone)]
pub struct AuthorityGate {
    client: Arc<ReceiverAuthorityClient>,
    active_grants: Arc<RwLock<HashMap<String, AuthorityGrant>>>,
    claimant_michi_id: String,
    claimant_name: String,
    claimant_service: String,
}

impl AuthorityGate {
    pub fn new(claimant_michi_id: String, claimant_name: String, claimant_service: String) -> Self {
        Self {
            client: Arc::new(ReceiverAuthorityClient::new()),
            active_grants: Arc::new(RwLock::new(HashMap::new())),
            claimant_michi_id,
            claimant_name,
            claimant_service,
        }
    }

    pub fn client(&self) -> Arc<ReceiverAuthorityClient> {
        self.client.clone()
    }

    /// Claim authority on a receiver if supported. Returns `Some(grant)` if authority was
    /// claimed, or `None` if the receiver operates in legacy receiver-v1-lite mode.
    pub async fn ensure_claim(
        &self,
        receiver: &ReceiverRegistryEntry,
    ) -> Result<Option<AuthorityGrant>, AuthorityError> {
        let endpoint = Url::parse(&receiver.base_url)
            .map_err(|e| AuthorityError::Protocol(format!("invalid base_url: {e}")))?;

        // If explicitly known not to support authority, return None immediately
        if !receiver.supports_authority_v1() && receiver.capabilities_verified_at.is_some() {
            return Ok(None);
        }

        // Feature probe: check /authority/info
        match self.client.info(&endpoint, receiver.token.as_deref()).await {
            Ok(_info) => {
                info!(receiver_id = %receiver.receiver_id, "AuthorityGate: receiver supports authority-v1, claiming Perch");
                let grant = self
                    .client
                    .claim(
                        &endpoint,
                        receiver.token.as_deref(),
                        &self.claimant_michi_id,
                        &self.claimant_name,
                        &self.claimant_service,
                    )
                    .await?;

                self.active_grants
                    .write()
                    .await
                    .insert(receiver.receiver_id.clone(), grant.clone());

                Ok(Some(grant))
            }
            Err(AuthorityError::Unsupported) => {
                info!(receiver_id = %receiver.receiver_id, "AuthorityGate: receiver does not support authority-v1; using legacy mode");
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Perform explicit takeover (Pounce) of an occupied receiver.
    pub async fn explicit_takeover(
        &self,
        receiver: &ReceiverRegistryEntry,
    ) -> Result<AuthorityGrant, AuthorityError> {
        let endpoint = Url::parse(&receiver.base_url)
            .map_err(|e| AuthorityError::Protocol(format!("invalid base_url: {e}")))?;

        let grant = self
            .client
            .takeover(
                &endpoint,
                receiver.token.as_deref(),
                &self.claimant_michi_id,
                &self.claimant_name,
                &self.claimant_service,
                true,
            )
            .await?;

        self.active_grants
            .write()
            .await
            .insert(receiver.receiver_id.clone(), grant.clone());

        Ok(grant)
    }

    /// Get current active grant in RAM for a receiver.
    pub async fn get_grant(&self, receiver_id: &str) -> Option<AuthorityGrant> {
        self.active_grants.read().await.get(receiver_id).cloned()
    }

    /// Store a grant in RAM (e.g. upon PawPass commit).
    pub async fn store_grant(&self, receiver_id: &str, grant: AuthorityGrant) {
        self.active_grants
            .write()
            .await
            .insert(receiver_id.to_string(), grant);
    }

    /// Release authority on a receiver.
    pub async fn release_grant(
        &self,
        receiver: &ReceiverRegistryEntry,
    ) -> Result<(), AuthorityError> {
        let grant_opt = self
            .active_grants
            .write()
            .await
            .remove(&receiver.receiver_id);
        if let Some(grant) = grant_opt {
            let endpoint = Url::parse(&receiver.base_url)
                .map_err(|e| AuthorityError::Protocol(format!("invalid base_url: {e}")))?;
            let stamp = AuthorityStamp {
                authority_instance_id: grant.authority_instance_id,
                lease_epoch: grant.lease_epoch,
            };
            let _ = self
                .client
                .release(&endpoint, receiver.token.as_deref(), &stamp)
                .await;
        }
        Ok(())
    }
}
