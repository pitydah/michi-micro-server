use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::models::{ReceiverPresence, ReceiverRegistryEntry};
use crate::session_manager::ReceiverSessionManager;
use michi_connect::{ScentEvent, ScentRecord, ScentStore};

#[derive(Debug, thiserror::Error)]
pub enum ReceiverDiscoveryError {
    #[error("channel error: {0}")]
    Channel(String),
}

/// Bridges verified presence and endpoint changes from `ScentStore` into `ReceiverRegistry`.
pub struct ReceiverDiscoveryBridge {
    scent: Arc<ScentStore>,
    receiver_manager: ReceiverSessionManager,
}

impl ReceiverDiscoveryBridge {
    pub fn new(scent: Arc<ScentStore>, receiver_manager: ReceiverSessionManager) -> Self {
        Self {
            scent,
            receiver_manager,
        }
    }

    /// Reconcile a single Scent event into the ReceiverRegistry.
    pub async fn handle_event(&self, event: ScentEvent) {
        match event {
            ScentEvent::Discovered(record) | ScentEvent::Updated(record) => {
                self.reconcile_record(&record).await;
            }
            ScentEvent::EndpointChanged {
                michi_id,
                old: _,
                new,
            } => {
                let registry_arc = self.receiver_manager.registry().await;
                let mut reg = registry_arc.write().await;
                if let Some(entry) = reg.get_mut(&michi_id) {
                    debug!(michi_id = %michi_id, new_endpoint = %new, "ReceiverDiscoveryBridge: updating endpoint");
                    entry.base_url = new.to_string();
                    entry.presence = ReceiverPresence::VerifiedOnline;
                    entry.last_seen = Some(chrono::Utc::now());
                }
            }
            ScentEvent::Offline { michi_id } => {
                let registry_arc = self.receiver_manager.registry().await;
                let mut reg = registry_arc.write().await;
                if let Some(entry) = reg.get_mut(&michi_id) {
                    info!(michi_id = %michi_id, "ReceiverDiscoveryBridge: marking receiver offline (90s TTL expired)");
                    entry.presence = ReceiverPresence::Offline;
                    entry.capabilities_stale = true;
                }
            }
        }
    }

    async fn reconcile_record(&self, record: &ScentRecord) {
        // Only verified records from Music Stream services with audio_receiver role
        if !record.verified {
            return;
        }
        let is_stream_service = record.service == "michi-stream-standard"
            || record.service == "michi-stream-hifi"
            || record.service.starts_with("michi-stream");
        let has_audio_role = record.roles.iter().any(|r| r == "audio_receiver");
        if !is_stream_service || !has_audio_role {
            return;
        }

        // Base URL must be verified before marking VerifiedOnline endpoint-ready
        let base_url_str = match &record.base_url {
            Some(u) => u.to_string(),
            None => {
                // Keep Scent only, cannot direct traffic without verified base_url
                return;
            }
        };

        let registry_arc = self.receiver_manager.registry().await;
        let mut reg = registry_arc.write().await;

        if let Some(entry) = reg.get_mut(&record.michi_id) {
            entry.base_url = base_url_str;
            entry.presence = ReceiverPresence::VerifiedOnline;
            entry.last_seen = Some(chrono::Utc::now());
            entry.name = record.name.clone();
            entry.device_type = if record.service.contains("hifi") {
                "hifi".to_string()
            } else {
                "standard".to_string()
            };
        } else {
            // Check if there is an entry by device_id or legacy ID
            let mut found_legacy = false;
            for entry in reg.receivers.values_mut() {
                if entry.receiver_id == record.device_id || entry.receiver_id == record.michi_id {
                    entry.michi_id = Some(record.michi_id.clone());
                    entry.base_url = base_url_str.clone();
                    entry.presence = ReceiverPresence::VerifiedOnline;
                    entry.last_seen = Some(chrono::Utc::now());
                    entry.name = record.name.clone();
                    found_legacy = true;
                    break;
                }
            }

            if !found_legacy {
                // Create UNPAIRED projected receiver
                let entry = ReceiverRegistryEntry {
                    receiver_id: record.michi_id.clone(),
                    michi_id: Some(record.michi_id.clone()),
                    name: record.name.clone(),
                    device_type: if record.service.contains("hifi") {
                        "hifi".to_string()
                    } else {
                        "standard".to_string()
                    },
                    base_url: base_url_str,
                    paired: false,
                    token: None,
                    presence: ReceiverPresence::VerifiedOnline,
                    last_seen: Some(chrono::Utc::now()),
                    capabilities: Vec::new(),
                    capabilities_verified_at: None,
                    capabilities_stale: true,
                    authority_supported: false,
                    owner_michi_id: None,
                    owner_name: None,
                    active_session_id: None,
                    max_sample_rate: 48000,
                    max_bit_depth: 16,
                    supported_transports: vec!["rtp_udp".to_string()],
                    supported_codecs: vec!["pcm_s16le".to_string()],
                    supported_sample_rates: vec![48000],
                    supported_bit_depths: vec![16],
                    supported_channels: vec![2],
                    maximum_safe_volume: Some(100),
                };
                info!(michi_id = %record.michi_id, name = %record.name, "ReceiverDiscoveryBridge: projected new unpaired receiver");
                reg.add(entry);
            }
        }
    }

    /// Reconcile current snapshot of ScentStore.
    pub async fn reconcile_snapshot(&self) {
        for record in self.scent.list_online() {
            self.reconcile_record(&record).await;
        }
    }

    /// Spawn the discovery bridge worker loop with cancellation support.
    pub fn spawn(self, cancel: CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("ReceiverDiscoveryBridge: worker started");
            let mut rx = self.scent.subscribe();

            // Initial snapshot reconciliation
            self.reconcile_snapshot().await;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        info!("ReceiverDiscoveryBridge: cancelled, shutting down");
                        break;
                    }
                    event_res = rx.recv() => {
                        match event_res {
                            Ok(event) => {
                                self.handle_event(event).await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                                warn!("ReceiverDiscoveryBridge: lagged by {n} events, reconciling full snapshot");
                                self.reconcile_snapshot().await;
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                                info!("ReceiverDiscoveryBridge: scent event channel closed");
                                break;
                            }
                        }
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use url::Url;

    fn test_identity() -> Arc<michi_identity::IdentityManager> {
        let dir = std::env::temp_dir().join(format!("test-id-bridge-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        Arc::new(michi_identity::IdentityManager::generate(&dir, "Bridge Test", "").unwrap())
    }

    #[tokio::test]
    async fn test_verified_stream_projected_to_registry() {
        let scent = Arc::new(ScentStore::new());
        let mgr = ReceiverSessionManager::new_with_identity(test_identity());
        let bridge = ReceiverDiscoveryBridge::new(scent.clone(), mgr.clone());

        let record = ScentRecord {
            michi_id: "stream-01-michi-id".to_string(),
            device_id: "stream-01".to_string(),
            name: "Living Room Stream".to_string(),
            service: "michi-stream-standard".to_string(),
            roles: vec!["audio_receiver".to_string()],
            verified: true,
            endpoints: vec!["192.168.1.100:8080".parse().unwrap()],
            base_url: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
            last_signed_seen: Instant::now(),
            last_mdns_seen: None,
            server_info_verified_at: Some(Instant::now()),
            online: true,
        };

        bridge
            .handle_event(ScentEvent::Discovered(record.clone()))
            .await;

        let reg = mgr.registry().await.read().await.clone();
        let entry = reg
            .get("stream-01-michi-id")
            .expect("must project to registry");
        assert_eq!(entry.name, "Living Room Stream");
        assert_eq!(entry.presence, ReceiverPresence::VerifiedOnline);
        assert!(!entry.paired);
        assert_eq!(entry.base_url, "http://192.168.1.100:8080/");
    }

    #[tokio::test]
    async fn test_unverified_stream_ignored() {
        let scent = Arc::new(ScentStore::new());
        let mgr = ReceiverSessionManager::new_with_identity(test_identity());
        let bridge = ReceiverDiscoveryBridge::new(scent.clone(), mgr.clone());

        let record = ScentRecord {
            michi_id: "stream-unverified".to_string(),
            device_id: "stream-unverified".to_string(),
            name: "Unverified Stream".to_string(),
            service: "michi-stream-standard".to_string(),
            roles: vec!["audio_receiver".to_string()],
            verified: false, // NOT verified!
            endpoints: vec![],
            base_url: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
            last_signed_seen: Instant::now(),
            last_mdns_seen: None,
            server_info_verified_at: None,
            online: true,
        };

        bridge.handle_event(ScentEvent::Discovered(record)).await;

        let reg = mgr.registry().await.read().await.clone();
        assert!(reg.get("stream-unverified").is_none());
    }

    #[tokio::test]
    async fn test_non_stream_service_ignored() {
        let scent = Arc::new(ScentStore::new());
        let mgr = ReceiverSessionManager::new_with_identity(test_identity());
        let bridge = ReceiverDiscoveryBridge::new(scent.clone(), mgr.clone());

        let record = ScentRecord {
            michi_id: "mobile-peer".to_string(),
            device_id: "mobile-peer".to_string(),
            name: "Michi Mobile".to_string(),
            service: "michi-player-mobile".to_string(), // Not a stream!
            roles: vec!["player_control".to_string()],
            verified: true,
            endpoints: vec![],
            base_url: Some(Url::parse("http://192.168.1.101:8080/").unwrap()),
            last_signed_seen: Instant::now(),
            last_mdns_seen: None,
            server_info_verified_at: None,
            online: true,
        };

        bridge.handle_event(ScentEvent::Discovered(record)).await;

        let reg = mgr.registry().await.read().await.clone();
        assert!(reg.get("mobile-peer").is_none());
    }

    #[tokio::test]
    async fn test_endpoint_changed_and_offline_events() {
        let scent = Arc::new(ScentStore::new());
        let mgr = ReceiverSessionManager::new_with_identity(test_identity());
        let bridge = ReceiverDiscoveryBridge::new(scent.clone(), mgr.clone());

        let record = ScentRecord {
            michi_id: "stream-dhcp".to_string(),
            device_id: "stream-dhcp".to_string(),
            name: "DHCP Stream".to_string(),
            service: "michi-stream-standard".to_string(),
            roles: vec!["audio_receiver".to_string()],
            verified: true,
            endpoints: vec![],
            base_url: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
            last_signed_seen: Instant::now(),
            last_mdns_seen: None,
            server_info_verified_at: Some(Instant::now()),
            online: true,
        };

        bridge.handle_event(ScentEvent::Discovered(record)).await;

        // Endpoint changes due to DHCP
        let new_url = Url::parse("http://192.168.1.200:8080/").unwrap();
        bridge
            .handle_event(ScentEvent::EndpointChanged {
                michi_id: "stream-dhcp".to_string(),
                old: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
                new: new_url.clone(),
            })
            .await;

        let reg = mgr.registry().await.read().await.clone();
        let entry = reg.get("stream-dhcp").unwrap();
        assert_eq!(entry.base_url, "http://192.168.1.200:8080/");
        assert_eq!(entry.presence, ReceiverPresence::VerifiedOnline);

        // Offline event (90s TTL)
        bridge
            .handle_event(ScentEvent::Offline {
                michi_id: "stream-dhcp".to_string(),
            })
            .await;

        let reg = mgr.registry().await.read().await.clone();
        let entry_offline = reg.get("stream-dhcp").unwrap();
        assert_eq!(entry_offline.presence, ReceiverPresence::Offline);
        assert!(entry_offline.capabilities_stale);
    }
}
