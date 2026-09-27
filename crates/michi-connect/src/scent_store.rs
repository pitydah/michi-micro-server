use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

/// Canonical expiration timeout for signed presence (90 seconds).
pub const SCENT_EXPIRY_TIMEOUT: Duration = Duration::from_secs(90);

/// Channel capacity for broadcast Scent events.
const SCENT_EVENT_CHANNEL_CAPACITY: usize = 256;

/// Dynamic presence record for a verified or discovered peer.
#[derive(Debug, Clone)]
pub struct ScentRecord {
    pub michi_id: String,
    pub device_id: String,
    pub name: String,
    pub service: String,
    pub roles: Vec<String>,
    pub verified: bool,
    pub endpoints: Vec<SocketAddr>,
    pub base_url: Option<Url>,
    pub last_signed_seen: Instant,
    pub last_mdns_seen: Option<Instant>,
    pub server_info_verified_at: Option<Instant>,
    pub online: bool,
}

/// Typed presence events emitted by the Scent store.
#[derive(Debug, Clone)]
pub enum ScentEvent {
    Discovered(ScentRecord),
    Updated(ScentRecord),
    EndpointChanged {
        michi_id: String,
        old: Option<Url>,
        new: Url,
    },
    Offline {
        michi_id: String,
    },
}

#[derive(Clone)]
pub struct ScentStore {
    records: Arc<RwLock<HashMap<String, ScentRecord>>>,
    event_tx: broadcast::Sender<ScentEvent>,
}

impl Default for ScentStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ScentStore {
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(SCENT_EVENT_CHANNEL_CAPACITY);
        Self {
            records: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
        }
    }

    /// Subscribe to typed presence events.
    pub fn subscribe(&self) -> broadcast::Receiver<ScentEvent> {
        self.event_tx.subscribe()
    }

    /// Record a verified signed announce from discovery.
    #[allow(clippy::too_many_arguments)]
    pub fn observe_signed(
        &self,
        michi_id: String,
        device_id: String,
        name: String,
        service: String,
        roles: Vec<String>,
        source: Option<SocketAddr>,
        now: Instant,
    ) {
        let mut to_send = Vec::new();

        {
            let mut store = self.records.write().unwrap();
            let entry = store.entry(michi_id.clone());

            match entry {
                std::collections::hash_map::Entry::Occupied(mut occ) => {
                    let record = occ.get_mut();
                    let was_offline = !record.online;
                    record.last_signed_seen = now;
                    record.online = true;
                    record.verified = true;
                    record.device_id = device_id;
                    record.name = name;
                    record.service = service;
                    record.roles = roles;

                    if let Some(src) = source {
                        if !record.endpoints.contains(&src) {
                            record.endpoints.push(src);
                        }
                    }

                    if was_offline {
                        info!(michi_id = %michi_id, "Scent: peer transitioned to online");
                        to_send.push(ScentEvent::Discovered(record.clone()));
                    } else {
                        debug!(michi_id = %michi_id, "Scent: peer refreshed signed presence");
                        to_send.push(ScentEvent::Updated(record.clone()));
                    }
                }
                std::collections::hash_map::Entry::Vacant(vac) => {
                    let mut endpoints = Vec::new();
                    if let Some(src) = source {
                        endpoints.push(src);
                    }
                    let record = ScentRecord {
                        michi_id: michi_id.clone(),
                        device_id,
                        name,
                        service,
                        roles,
                        verified: true,
                        endpoints,
                        base_url: None,
                        last_signed_seen: now,
                        last_mdns_seen: None,
                        server_info_verified_at: None,
                        online: true,
                    };
                    info!(michi_id = %michi_id, "Scent: discovered new signed peer");
                    vac.insert(record.clone());
                    to_send.push(ScentEvent::Discovered(record));
                }
            }
        }

        for ev in to_send {
            let _ = self.event_tx.send(ev);
        }
    }

    /// Update the base_url for a peer upon verified mDNS / server-info resolution.
    pub fn update_base_url(&self, michi_id: &str, new_url: Url, source_endpoint: Option<SocketAddr>, now: Instant) {
        let mut to_send = Vec::new();

        {
            let mut store = self.records.write().unwrap();
            if let Some(record) = store.get_mut(michi_id) {
                record.last_mdns_seen = Some(now);
                if let Some(src) = source_endpoint {
                    if !record.endpoints.contains(&src) {
                        record.endpoints.push(src);
                    }
                }

                let old_url = record.base_url.clone();
                if old_url.as_ref() != Some(&new_url) {
                    record.base_url = Some(new_url.clone());
                    info!(
                        michi_id = %michi_id,
                        old = ?old_url.as_ref().map(|u| u.as_str()),
                        new = %new_url.as_str(),
                        "Scent: peer base_url updated"
                    );
                    to_send.push(ScentEvent::EndpointChanged {
                        michi_id: michi_id.to_string(),
                        old: old_url,
                        new: new_url,
                    });
                }
            }
        }

        for ev in to_send {
            let _ = self.event_tx.send(ev);
        }
    }

    /// Mark server/info verification timestamp.
    pub fn mark_server_info_verified(&self, michi_id: &str, now: Instant) {
        let mut store = self.records.write().unwrap();
        if let Some(record) = store.get_mut(michi_id) {
            record.server_info_verified_at = Some(now);
        }
    }

    /// Check expirations against given instant, marking expired records offline.
    pub fn check_expirations(&self, now: Instant) -> Vec<ScentEvent> {
        let mut events = Vec::new();
        let mut store = self.records.write().unwrap();

        for (michi_id, record) in store.iter_mut() {
            if record.online && now.duration_since(record.last_signed_seen) >= SCENT_EXPIRY_TIMEOUT {
                record.online = false;
                warn!(michi_id = %michi_id, "Scent: peer expired after 90s without signed presence; marked offline");
                events.push(ScentEvent::Offline {
                    michi_id: michi_id.clone(),
                });
            }
        }

        for ev in &events {
            let _ = self.event_tx.send(ev.clone());
        }

        events
    }

    /// Get record for given michi_id.
    pub fn get(&self, michi_id: &str) -> Option<ScentRecord> {
        let store = self.records.read().unwrap();
        store.get(michi_id).cloned()
    }

    /// List all known scent records.
    pub fn list(&self) -> Vec<ScentRecord> {
        let store = self.records.read().unwrap();
        store.values().cloned().collect()
    }

    /// List online verified scent records.
    pub fn list_online(&self) -> Vec<ScentRecord> {
        let store = self.records.read().unwrap();
        store.values().filter(|r| r.online && r.verified).cloned().collect()
    }

    /// Spawn periodic background sweeper task that checks expirations every 5 seconds.
    pub fn spawn_expiry_sweeper(&self, cancel_token: CancellationToken) -> tokio::task::JoinHandle<()> {
        let store = self.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        debug!("Scent expiry sweeper cancelled");
                        break;
                    }
                    _ = interval.tick() => {
                        store.check_expirations(Instant::now());
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observe_and_update() {
        let store = ScentStore::new();
        let mut rx = store.subscribe();
        let now = Instant::now();

        store.observe_signed(
            "michi-id-1".into(),
            "dev-1".into(),
            "Stream Speaker".into(),
            "michi-stream-standard".into(),
            vec!["audio_receiver".into()],
            Some("192.168.1.100:53318".parse().unwrap()),
            now,
        );

        let rec = store.get("michi-id-1").expect("must exist");
        assert!(rec.online);
        assert!(rec.verified);
        assert_eq!(rec.name, "Stream Speaker");

        let ev = rx.try_recv().expect("should receive Discovered");
        match ev {
            ScentEvent::Discovered(r) => assert_eq!(r.michi_id, "michi-id-1"),
            _ => panic!("unexpected event"),
        }

        // Update URL
        let url: Url = "http://192.168.1.100:8080".parse().unwrap();
        store.update_base_url("michi-id-1", url.clone(), None, now);

        let updated = store.get("michi-id-1").unwrap();
        assert_eq!(updated.base_url, Some(url));

        let ev = rx.try_recv().expect("should receive EndpointChanged");
        match ev {
            ScentEvent::EndpointChanged { michi_id, new, .. } => {
                assert_eq!(michi_id, "michi-id-1");
                assert_eq!(new.as_str(), "http://192.168.1.100:8080/");
            }
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn test_expiration_marks_offline() {
        let store = ScentStore::new();
        let mut rx = store.subscribe();
        let t0 = Instant::now();

        store.observe_signed(
            "michi-id-2".into(),
            "dev-2".into(),
            "Living Room".into(),
            "michi-stream-hifi".into(),
            vec!["audio_receiver".into()],
            None,
            t0,
        );
        let _ = rx.try_recv();

        // 89s later: still online
        let events = store.check_expirations(t0 + Duration::from_secs(89));
        assert!(events.is_empty());
        assert!(store.get("michi-id-2").unwrap().online);

        // 91s later: expired
        let events = store.check_expirations(t0 + Duration::from_secs(91));
        assert_eq!(events.len(), 1);
        assert!(!store.get("michi-id-2").unwrap().online);

        let ev = rx.try_recv().expect("should receive Offline event");
        match ev {
            ScentEvent::Offline { michi_id } => assert_eq!(michi_id, "michi-id-2"),
            _ => panic!("unexpected event"),
        }
    }
}
