use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::models::{ReceiverPresence, ReceiverQualification, ReceiverRegistryEntry};
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
                presence_source,
                verified,
            } => {
                let registry_arc = self.receiver_manager.registry().await;
                let mut reg = registry_arc.write().await;
                if let Some(entry) = reg.get_mut(&michi_id) {
                    debug!(michi_id = %michi_id, new_endpoint = %new, "ReceiverDiscoveryBridge: updating endpoint");
                    entry.base_url = new.to_string();
                    entry.presence = match presence_source {
                        michi_connect::scent_store::ScentPresenceSource::WhiskerSigned
                            if verified =>
                        {
                            ReceiverPresence::VerifiedOnline
                        }
                        _ => ReceiverPresence::ProvisionalMdns,
                    };
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
        // Verified signed records or verified mDNS provisional candidates
        if !record.verified
            && record.presence_source
                != michi_connect::scent_store::ScentPresenceSource::MdnsProvisional
        {
            return;
        }
        let is_stream_service =
            record.service == "michi-stream-standard" || record.service == "michi-stream-hifi";
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

        let target_presence = match record.presence_source {
            michi_connect::scent_store::ScentPresenceSource::WhiskerSigned if record.verified => {
                ReceiverPresence::VerifiedOnline
            }
            _ => ReceiverPresence::ProvisionalMdns,
        };

        let registry_arc = self.receiver_manager.registry().await;
        let mut reg = registry_arc.write().await;

        if let Some(entry) = reg.get_mut(&record.michi_id) {
            entry.base_url = base_url_str;
            entry.presence = target_presence;
            entry.last_seen = Some(chrono::Utc::now());
            entry.name = record.name.clone();
            entry.device_type = if record.service.contains("hifi") {
                "hifi".to_string()
            } else {
                "standard".to_string()
            };
            if !entry.authenticated && !entry.revoked {
                let mgr = self.receiver_manager.clone();
                let mid = record.michi_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = mgr.authenticate_receiver(&mid).await {
                        debug!("Auto-auth for updated receiver {} deferred: {}", mid, e);
                    } else {
                        info!("Auto-auth completed successfully for receiver {}", mid);
                    }
                });
            }
        } else {
            // Check if there is an entry by device_id or legacy ID
            let mut found_legacy = false;
            for entry in reg.receivers.values_mut() {
                if entry.receiver_id == record.device_id || entry.receiver_id == record.michi_id {
                    entry.michi_id = Some(record.michi_id.clone());
                    entry.base_url = base_url_str.clone();
                    entry.presence = target_presence;
                    entry.last_seen = Some(chrono::Utc::now());
                    entry.name = record.name.clone();
                    if record.michi_home_id.is_some() {
                        entry.michi_home_id = record.michi_home_id.clone();
                    }
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
                    presence: target_presence,
                    last_seen: Some(chrono::Utc::now()),
                    capabilities: Vec::new(),
                    capabilities_verified_at: None,
                    capabilities_stale: true,
                    authority_supported: false,
                    owner_michi_id: None,
                    owner_name: None,
                    active_session_id: None,
                    max_sample_rate: 0,
                    max_bit_depth: 0,
                    supported_transports: Vec::new(),
                    supported_codecs: Vec::new(),
                    supported_sample_rates: Vec::new(),
                    supported_bit_depths: Vec::new(),
                    supported_channels: Vec::new(),
                    maximum_safe_volume: None,
                    qualification: ReceiverQualification::NeedsCapabilityRefresh,
                    michi_home_id: record.michi_home_id.clone(),
                    server_membership: None,
                    authenticated: false,
                    revoked: false,
                };
                info!(
                    michi_id = %record.michi_id,
                    name = %record.name,
                    presence = ?target_presence,
                    "ReceiverDiscoveryBridge: projected new receiver"
                );
                reg.add(entry);

                let mgr = self.receiver_manager.clone();
                let mid = record.michi_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = mgr.authenticate_receiver(&mid).await {
                        debug!("Auto-auth for receiver {} deferred: {}", mid, e);
                    } else {
                        info!("Auto-auth completed successfully for receiver {}", mid);
                    }
                });
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
            presence_source: michi_connect::scent_store::ScentPresenceSource::WhiskerSigned,
            endpoints: vec!["192.168.1.100:8080".parse().unwrap()],
            base_url: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
            last_signed_seen: Some(Instant::now()),
            last_mdns_seen: None,
            server_info_verified_at: Some(Instant::now()),
            online: true,
            michi_home_id: None,
            membership_fingerprint: None,
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
    async fn test_provisional_mdns_stream_projected_to_registry() {
        let scent = Arc::new(ScentStore::new());
        let mgr = ReceiverSessionManager::new_with_identity(test_identity());
        let bridge = ReceiverDiscoveryBridge::new(scent.clone(), mgr.clone());

        let record = ScentRecord {
            michi_id: "stream-mdns-id".to_string(),
            device_id: "stream-mdns".to_string(),
            name: "Bedroom Stream".to_string(),
            service: "michi-stream-standard".to_string(),
            roles: vec!["audio_receiver".to_string()],
            verified: true,
            presence_source: michi_connect::scent_store::ScentPresenceSource::MdnsProvisional,
            endpoints: vec!["192.168.1.105:8080".parse().unwrap()],
            base_url: Some(Url::parse("http://192.168.1.105:8080/").unwrap()),
            last_signed_seen: None,
            last_mdns_seen: Some(Instant::now()),
            server_info_verified_at: Some(Instant::now()),
            online: true,
            michi_home_id: None,
            membership_fingerprint: None,
        };

        bridge
            .handle_event(ScentEvent::Discovered(record.clone()))
            .await;

        let reg = mgr.registry().await.read().await.clone();
        let entry = reg.get("stream-mdns-id").expect("must project to registry");
        assert_eq!(entry.name, "Bedroom Stream");
        assert_eq!(entry.presence, ReceiverPresence::ProvisionalMdns);
        assert!(!entry.paired);
        assert_eq!(entry.base_url, "http://192.168.1.105:8080/");

        // Now signed announce arrives: upgrades presence to VerifiedOnline
        let mut upgraded = record.clone();
        upgraded.presence_source = michi_connect::scent_store::ScentPresenceSource::WhiskerSigned;
        upgraded.last_signed_seen = Some(Instant::now());

        bridge.handle_event(ScentEvent::Updated(upgraded)).await;

        let reg_after = mgr.registry().await.read().await.clone();
        let entry_after = reg_after.get("stream-mdns-id").unwrap();
        assert_eq!(entry_after.presence, ReceiverPresence::VerifiedOnline);
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
            presence_source: michi_connect::scent_store::ScentPresenceSource::WhiskerSigned,
            endpoints: vec![],
            base_url: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
            last_signed_seen: Some(Instant::now()),
            last_mdns_seen: None,
            server_info_verified_at: None,
            online: true,
            michi_home_id: None,
            membership_fingerprint: None,
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
            presence_source: michi_connect::scent_store::ScentPresenceSource::WhiskerSigned,
            endpoints: vec![],
            base_url: Some(Url::parse("http://192.168.1.101:8080/").unwrap()),
            last_signed_seen: Some(Instant::now()),
            last_mdns_seen: None,
            server_info_verified_at: None,
            online: true,
            michi_home_id: None,
            membership_fingerprint: None,
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
            presence_source: michi_connect::scent_store::ScentPresenceSource::WhiskerSigned,
            endpoints: vec![],
            base_url: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
            last_signed_seen: Some(Instant::now()),
            last_mdns_seen: None,
            server_info_verified_at: Some(Instant::now()),
            online: true,
            michi_home_id: None,
            membership_fingerprint: None,
        };

        bridge.handle_event(ScentEvent::Discovered(record)).await;

        // Endpoint changes due to DHCP
        let new_url = Url::parse("http://192.168.1.200:8080/").unwrap();
        bridge
            .handle_event(ScentEvent::EndpointChanged {
                michi_id: "stream-dhcp".to_string(),
                old: Some(Url::parse("http://192.168.1.100:8080/").unwrap()),
                new: new_url.clone(),
                presence_source: michi_connect::scent_store::ScentPresenceSource::WhiskerSigned,
                verified: true,
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
