use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

/// Canonical expiration timeout for presence (90 seconds).
pub const SCENT_EXPIRY_TIMEOUT: Duration = Duration::from_secs(90);

/// Channel capacity for broadcast Scent events.
const SCENT_EVENT_CHANNEL_CAPACITY: usize = 256;

/// Provenance of the active presence information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScentPresenceSource {
    /// Peer announced via signed Whisker UDP multicast packet.
    WhiskerSigned,
    /// Peer discovered via mDNS and verified via GET /api/v1/server/info.
    MdnsProvisional,
}

/// Verified server info returned from GET /api/v1/server/info.
#[derive(Debug, Clone)]
pub struct VerifiedServerInfo {
    pub michi_id: String,
    pub device_id: String,
    pub name: String,
    pub service: String,
    pub roles: Vec<String>,
}

/// Dynamic presence record for a verified or discovered peer.
#[derive(Debug, Clone)]
pub struct ScentRecord {
    pub michi_id: String,
    pub device_id: String,
    pub name: String,
    pub service: String,
    pub roles: Vec<String>,
    pub verified: bool,
    pub presence_source: ScentPresenceSource,
    pub endpoints: Vec<SocketAddr>,
    pub base_url: Option<Url>,
    pub last_signed_seen: Option<Instant>,
    pub last_mdns_seen: Option<Instant>,
    pub server_info_verified_at: Option<Instant>,
    pub online: bool,
}

/// Authoritative presence level derived strictly from current freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectivePresence {
    VerifiedOnline,
    ProvisionalMdns,
    Offline,
}

impl ScentRecord {
    pub fn is_signed_fresh(&self, now: Instant) -> bool {
        self.last_signed_seen
            .is_some_and(|t| now.duration_since(t) < SCENT_EXPIRY_TIMEOUT)
    }

    pub fn is_mdns_fresh(&self, now: Instant) -> bool {
        self.last_mdns_seen
            .is_some_and(|t| now.duration_since(t) < SCENT_EXPIRY_TIMEOUT)
    }

    pub fn effective_presence(&self, now: Instant) -> EffectivePresence {
        if self.is_signed_fresh(now) {
            EffectivePresence::VerifiedOnline
        } else if self.is_mdns_fresh(now) {
            EffectivePresence::ProvisionalMdns
        } else {
            EffectivePresence::Offline
        }
    }
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
        presence_source: ScentPresenceSource,
        verified: bool,
    },
    Offline {
        michi_id: String,
    },
}

#[derive(Debug, Clone)]
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
                    record.last_signed_seen = Some(now);
                    record.presence_source = ScentPresenceSource::WhiskerSigned;
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
                        presence_source: ScentPresenceSource::WhiskerSigned,
                        endpoints,
                        base_url: None,
                        last_signed_seen: Some(now),
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

    /// Record a verified mDNS candidate peer. If unknown, creates a provisional entry;
    /// if already known, updates base_url and refreshes mDNS presence.
    pub fn observe_mdns_candidate(
        &self,
        info: VerifiedServerInfo,
        base_url: Url,
        source: Option<SocketAddr>,
        now: Instant,
    ) {
        let mut to_send = Vec::new();

        {
            let mut store = self.records.write().unwrap();
            let entry = store.entry(info.michi_id.clone());

            match entry {
                std::collections::hash_map::Entry::Occupied(mut occ) => {
                    let record = occ.get_mut();
                    let was_offline = !record.online;
                    record.last_mdns_seen = Some(now);
                    record.server_info_verified_at = Some(now);
                    record.online = true;

                    if record.is_signed_fresh(now) {
                        record.presence_source = ScentPresenceSource::WhiskerSigned;
                        record.verified = true;
                    } else {
                        // Signed Whisker is stale: mDNS cannot elevate or retain verified trust
                        record.presence_source = ScentPresenceSource::MdnsProvisional;
                        record.verified = false;
                        record.device_id = info.device_id;
                        record.name = info.name;
                        record.service = info.service;
                        record.roles = info.roles;
                    }

                    if let Some(src) = source {
                        if !record.endpoints.contains(&src) {
                            record.endpoints.push(src);
                        }
                    }

                    let old_url = record.base_url.clone();
                    let url_changed = old_url.as_ref() != Some(&base_url);
                    record.base_url = Some(base_url.clone());

                    if was_offline {
                        info!(michi_id = %info.michi_id, "Scent: provisional mDNS peer transitioned to online");
                        to_send.push(ScentEvent::Discovered(record.clone()));
                    } else if url_changed {
                        info!(
                            michi_id = %info.michi_id,
                            old = ?old_url.as_ref().map(|u| u.as_str()),
                            new = %base_url.as_str(),
                            "Scent: peer base_url updated via mDNS candidate"
                        );
                        to_send.push(ScentEvent::EndpointChanged {
                            michi_id: info.michi_id.clone(),
                            old: old_url,
                            new: base_url,
                            presence_source: record.presence_source,
                            verified: record.verified,
                        });
                    } else {
                        debug!(michi_id = %info.michi_id, "Scent: peer refreshed mDNS presence");
                        to_send.push(ScentEvent::Updated(record.clone()));
                    }
                }
                std::collections::hash_map::Entry::Vacant(vac) => {
                    let mut endpoints = Vec::new();
                    if let Some(src) = source {
                        endpoints.push(src);
                    }
                    let record = ScentRecord {
                        michi_id: info.michi_id.clone(),
                        device_id: info.device_id,
                        name: info.name,
                        service: info.service,
                        roles: info.roles,
                        verified: false,
                        presence_source: ScentPresenceSource::MdnsProvisional,
                        endpoints,
                        base_url: Some(base_url),
                        last_signed_seen: None,
                        last_mdns_seen: Some(now),
                        server_info_verified_at: Some(now),
                        online: true,
                    };
                    info!(michi_id = %info.michi_id, "Scent: discovered new provisional mDNS peer");
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
    pub fn update_base_url(
        &self,
        michi_id: &str,
        new_url: Url,
        source_endpoint: Option<SocketAddr>,
        now: Instant,
    ) {
        let mut to_send = Vec::new();

        {
            let mut store = self.records.write().unwrap();
            if let Some(record) = store.get_mut(michi_id) {
                record.last_mdns_seen = Some(now);
                if !record.is_signed_fresh(now) {
                    record.presence_source = ScentPresenceSource::MdnsProvisional;
                    record.verified = false;
                }
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
                        presence_source: record.presence_source,
                        verified: record.verified,
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

    /// Check expirations against given instant, marking expired records offline
    /// or downgrading stale signed records to provisional mDNS when mDNS is still fresh.
    pub fn check_expirations(&self, now: Instant) -> Vec<ScentEvent> {
        let mut events = Vec::new();
        let mut store = self.records.write().unwrap();

        for (michi_id, record) in store.iter_mut() {
            if !record.online {
                continue;
            }
            match record.effective_presence(now) {
                EffectivePresence::VerifiedOnline => {
                    // Still fresh signed Whisker presence
                }
                EffectivePresence::ProvisionalMdns => {
                    // Signed Whisker expired, but mDNS presence is still fresh: downgrade
                    if record.presence_source != ScentPresenceSource::MdnsProvisional
                        || record.verified
                    {
                        record.presence_source = ScentPresenceSource::MdnsProvisional;
                        record.verified = false;
                        record.online = true;
                        warn!(
                            michi_id = %michi_id,
                            "Scent: signed Whisker presence expired; downgraded to provisional mDNS"
                        );
                        events.push(ScentEvent::Updated(record.clone()));
                    }
                }
                EffectivePresence::Offline => {
                    record.online = false;
                    record.verified = false;
                    warn!(
                        michi_id = %michi_id,
                        source = ?record.presence_source,
                        "Scent: peer expired after timeout without refresh; marked offline"
                    );
                    events.push(ScentEvent::Offline {
                        michi_id: michi_id.clone(),
                    });
                }
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

    /// List all currently active scent records (both WhiskerSigned and MdnsProvisional).
    pub fn list_active(&self) -> Vec<ScentRecord> {
        let store = self.records.read().unwrap();
        store.values().filter(|r| r.online).cloned().collect()
    }

    /// List only cryptographically verified signed scent records.
    pub fn list_signed_verified(&self) -> Vec<ScentRecord> {
        let store = self.records.read().unwrap();
        store
            .values()
            .filter(|r| {
                r.online && r.verified && r.presence_source == ScentPresenceSource::WhiskerSigned
            })
            .cloned()
            .collect()
    }

    /// List active online scent records (equivalent to list_active).
    pub fn list_online(&self) -> Vec<ScentRecord> {
        self.list_active()
    }

    /// Spawn periodic background sweeper task that checks expirations every 5 seconds.
    pub fn spawn_expiry_sweeper(
        &self,
        cancel_token: CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
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

    #[test]
    fn test_observe_mdns_candidate_and_upgrade_to_signed() {
        let store = ScentStore::new();
        let mut rx = store.subscribe();
        let t0 = Instant::now();

        let info = VerifiedServerInfo {
            michi_id: "michi-mdns-1".into(),
            device_id: "dev-mdns-1".into(),
            name: "Kitchen Stream".into(),
            service: "michi-stream-standard".into(),
            roles: vec!["audio_receiver".into()],
        };
        let url: Url = "http://192.168.1.150:8080/".parse().unwrap();
        let ep = "192.168.1.150:8080".parse().unwrap();

        store.observe_mdns_candidate(info, url.clone(), Some(ep), t0);

        let rec = store.get("michi-mdns-1").expect("must exist");
        assert!(rec.online);
        assert!(!rec.verified);
        assert_eq!(store.list_signed_verified().len(), 0);
        assert_eq!(store.list_active().len(), 1);
        assert_eq!(rec.presence_source, ScentPresenceSource::MdnsProvisional);
        assert_eq!(rec.base_url, Some(url.clone()));
        assert_eq!(rec.endpoints, vec![ep]);

        let ev = rx.try_recv().expect("should receive Discovered");
        match ev {
            ScentEvent::Discovered(r) => {
                assert_eq!(r.michi_id, "michi-mdns-1");
                assert_eq!(r.presence_source, ScentPresenceSource::MdnsProvisional);
            }
            _ => panic!("unexpected event"),
        }

        // Now signed announce arrives: upgrades presence_source to WhiskerSigned
        let t1 = t0 + Duration::from_secs(5);
        store.observe_signed(
            "michi-mdns-1".into(),
            "dev-mdns-1".into(),
            "Kitchen Stream".into(),
            "michi-stream-standard".into(),
            vec!["audio_receiver".into()],
            Some("192.168.1.150:53318".parse().unwrap()),
            t1,
        );

        let upgraded = store.get("michi-mdns-1").expect("must exist");
        assert!(upgraded.verified);
        assert_eq!(store.list_signed_verified().len(), 1);
        assert_eq!(store.list_active().len(), 1);
        assert_eq!(upgraded.presence_source, ScentPresenceSource::WhiskerSigned);
        assert_eq!(upgraded.base_url, Some(url)); // base_url preserved!
        assert_eq!(upgraded.endpoints.len(), 2);
    }

    #[test]
    fn test_mdns_provisional_expiration() {
        let store = ScentStore::new();
        let mut rx = store.subscribe();
        let t0 = Instant::now();

        let info = VerifiedServerInfo {
            michi_id: "michi-mdns-exp".into(),
            device_id: "dev-exp".into(),
            name: "Patio Stream".into(),
            service: "michi-stream-standard".into(),
            roles: vec!["audio_receiver".into()],
        };
        let url: Url = "http://192.168.1.160:8080/".parse().unwrap();
        store.observe_mdns_candidate(info, url, None, t0);
        let _ = rx.try_recv();

        // 89s later: still online
        let events = store.check_expirations(t0 + Duration::from_secs(89));
        assert!(events.is_empty());
        assert!(store.get("michi-mdns-exp").unwrap().online);

        // 91s later: expired
        let events = store.check_expirations(t0 + Duration::from_secs(91));
        assert_eq!(events.len(), 1);
        assert!(!store.get("michi-mdns-exp").unwrap().online);

        let ev = rx.try_recv().expect("should receive Offline event");
        match ev {
            ScentEvent::Offline { michi_id } => assert_eq!(michi_id, "michi-mdns-exp"),
            _ => panic!("unexpected event"),
        }
    }

    #[test]
    fn test_trust_matrix_presence_lifecycle() {
        let store = ScentStore::new();
        let mut rx = store.subscribe();
        let t0 = Instant::now();

        // 1. Signed Whisker record has presence_source = WhiskerSigned, verified = true
        store.observe_signed(
            "michi-stream-1".into(),
            "dev-1".into(),
            "Stream 1".into(),
            "michi-stream-standard".into(),
            vec!["audio_receiver".into()],
            Some("192.168.1.50:53318".parse().unwrap()),
            t0,
        );
        let _ = rx.try_recv();
        let rec = store.get("michi-stream-1").unwrap();
        assert_eq!(rec.presence_source, ScentPresenceSource::WhiskerSigned);
        assert!(rec.verified);
        assert!(rec.online);
        assert_eq!(
            rec.effective_presence(t0),
            EffectivePresence::VerifiedOnline
        );

        // 2. mDNS candidate arrives 10s later -> updates base_url but retains WhiskerSigned & verified=true
        let t1 = t0 + Duration::from_secs(10);
        let info = VerifiedServerInfo {
            michi_id: "michi-stream-1".into(),
            device_id: "dev-1".into(),
            name: "Stream 1".into(),
            service: "michi-stream-standard".into(),
            roles: vec!["audio_receiver".into()],
        };
        let url: Url = "http://192.168.1.50:8080/".parse().unwrap();
        store.observe_mdns_candidate(
            info,
            url.clone(),
            Some("192.168.1.50:8080".parse().unwrap()),
            t1,
        );
        let rec = store.get("michi-stream-1").unwrap();
        assert_eq!(rec.presence_source, ScentPresenceSource::WhiskerSigned);
        assert!(rec.verified);
        assert_eq!(rec.base_url, Some(url));
        assert_eq!(
            rec.effective_presence(t1),
            EffectivePresence::VerifiedOnline
        );

        // Drain event
        let _ = rx.try_recv();

        // 3. At t0 + 95s: Whisker is stale (>90s), but mDNS was seen at t1 (t0+10s), so mDNS is 85s old (<90s).
        // Effective presence is ProvisionalMdns.
        let t2 = t0 + Duration::from_secs(95);
        let rec = store.get("michi-stream-1").unwrap();
        assert_eq!(
            rec.effective_presence(t2),
            EffectivePresence::ProvisionalMdns
        );

        let events = store.check_expirations(t2);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScentEvent::Updated(r) => {
                assert_eq!(r.michi_id, "michi-stream-1");
                assert_eq!(r.presence_source, ScentPresenceSource::MdnsProvisional);
                assert!(!r.verified);
                assert!(r.online);
            }
            _ => panic!("expected Updated event with downgrade to MdnsProvisional"),
        }
        let rec = store.get("michi-stream-1").unwrap();
        assert_eq!(rec.presence_source, ScentPresenceSource::MdnsProvisional);
        assert!(!rec.verified);
        assert!(rec.online);

        // Drain event
        let _ = rx.try_recv();

        // 4. At t0 + 105s: mDNS is now 95s old (>90s). Both are stale -> Offline.
        let t3 = t0 + Duration::from_secs(105);
        let rec = store.get("michi-stream-1").unwrap();
        assert_eq!(rec.effective_presence(t3), EffectivePresence::Offline);

        let events = store.check_expirations(t3);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ScentEvent::Offline { michi_id } => {
                assert_eq!(michi_id, "michi-stream-1");
            }
            _ => panic!("expected Offline event"),
        }
        let rec = store.get("michi-stream-1").unwrap();
        assert!(!rec.online);
        assert!(!rec.verified);
    }
}
