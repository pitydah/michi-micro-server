use crate::scent_store::ScentStore;
use michi_identity::discovery::{
    DiscoveryEngine, MAX_ANNOUNCE_BYTES, MULTICAST_GROUP, MULTICAST_PORT,
};
use michi_identity::types::{Announce, Role, Service, TrustLevel};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

/// Counters for diagnostic observability.
#[derive(Debug, Default)]
pub struct WhiskerMetrics {
    pub packets_received: AtomicU64,
    pub announces_verified: AtomicU64,
    pub signature_rejected: AtomicU64,
    pub timestamp_rejected: AtomicU64,
    pub replay_rejected: AtomicU64,
    pub non_stream_filtered: AtomicU64,
}

pub struct WhiskerDiscoveryListener {
    engine: Arc<DiscoveryEngine>,
    scent: Arc<ScentStore>,
    metrics: Arc<WhiskerMetrics>,
    multicast_group: Ipv4Addr,
    multicast_port: u16,
}

impl WhiskerDiscoveryListener {
    pub fn new(engine: Arc<DiscoveryEngine>, scent: Arc<ScentStore>) -> Self {
        Self {
            engine,
            scent,
            metrics: Arc::new(WhiskerMetrics::default()),
            multicast_group: MULTICAST_GROUP
                .parse()
                .expect("valid canonical multicast IP"),
            multicast_port: MULTICAST_PORT,
        }
    }

    pub fn with_custom_network(
        engine: Arc<DiscoveryEngine>,
        scent: Arc<ScentStore>,
        group: Ipv4Addr,
        port: u16,
    ) -> Self {
        Self {
            engine,
            scent,
            metrics: Arc::new(WhiskerMetrics::default()),
            multicast_group: group,
            multicast_port: port,
        }
    }

    pub fn metrics(&self) -> Arc<WhiskerMetrics> {
        self.metrics.clone()
    }

    /// Bind the multicast UDP socket with SO_REUSEADDR and SO_REUSEPORT.
    /// Checks the `MICHI_WHISKER_IFACE_IP` environment variable if set, otherwise defaults to UNSPECIFIED.
    pub fn bind_socket(&self) -> std::io::Result<UdpSocket> {
        let env_iface = std::env::var("MICHI_WHISKER_IFACE_IP")
            .ok()
            .and_then(|s| s.parse::<Ipv4Addr>().ok());
        self.bind_socket_on(env_iface)
    }

    /// Bind the multicast UDP socket on a specific network interface IP (e.g. physical LAN IP).
    pub fn bind_socket_on(&self, interface_ip: Option<Ipv4Addr>) -> std::io::Result<UdpSocket> {
        let domain = socket2::Domain::IPV4;
        let socket = socket2::Socket::new(domain, socket2::Type::DGRAM, None)?;

        socket.set_reuse_address(true)?;
        #[cfg(all(unix, not(target_os = "solaris"), not(target_os = "illumos")))]
        let _ = socket.set_reuse_port(true);

        socket.set_nonblocking(true)?;

        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, self.multicast_port);
        socket.bind(&bind_addr.into())?;

        let iface = interface_ip.unwrap_or(Ipv4Addr::UNSPECIFIED);
        socket.join_multicast_v4(&self.multicast_group, &iface)?;

        info!(
            group = %self.multicast_group,
            port = self.multicast_port,
            interface = %iface,
            "Whisker discovery socket bound and joined multicast group"
        );

        let std_socket: std::net::UdpSocket = socket.into();
        UdpSocket::from_std(std_socket)
    }

    /// Process a raw received datagram buffer. Returns true if accepted into Scent.
    pub fn handle_packet(&self, buf: &[u8], source: SocketAddr, now: Instant) -> bool {
        self.metrics
            .packets_received
            .fetch_add(1, Ordering::Relaxed);

        let announce: Announce = match serde_json::from_slice(buf) {
            Ok(ann) => ann,
            Err(e) => {
                debug!(err = %e, "Whisker: failed to parse announce JSON");
                self.metrics
                    .signature_rejected
                    .fetch_add(1, Ordering::Relaxed);
                return false;
            }
        };

        match self.engine.verify_announce(&announce, Some(source)) {
            Ok(TrustLevel::Verified(michi_id)) => {
                // Filter strictly for Music Stream services with audio_receiver role
                let is_stream = matches!(
                    announce.service,
                    Service::StreamStandard | Service::StreamHiFi
                );
                let has_receiver_role = announce.roles.contains(&Role::AudioReceiver);

                if !is_stream || !has_receiver_role {
                    debug!(
                        service = ?announce.service,
                        roles = ?announce.roles,
                        "Whisker: verified peer is not a Michi Music Stream receiver, filtered"
                    );
                    self.metrics
                        .non_stream_filtered
                        .fetch_add(1, Ordering::Relaxed);
                    return false;
                }

                self.metrics
                    .announces_verified
                    .fetch_add(1, Ordering::Relaxed);
                let service_str = match announce.service {
                    Service::StreamStandard => "michi-stream-standard",
                    Service::StreamHiFi => "michi-stream-hifi",
                    _ => "unknown",
                };
                let roles_str: Vec<String> = announce
                    .roles
                    .iter()
                    .map(|r| match r {
                        Role::AudioReceiver => "audio_receiver".to_string(),
                        _ => format!("{r:?}").to_lowercase(),
                    })
                    .collect();

                self.scent.observe_signed(
                    michi_id,
                    announce.device_id,
                    announce.name,
                    service_str.to_string(),
                    roles_str,
                    Some(source),
                    now,
                );
                true
            }
            Ok(TrustLevel::Untrusted(dev_id)) => {
                debug!(device_id = %dev_id, "Whisker: rejected untrusted/unsigned announce");
                self.metrics
                    .signature_rejected
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
            Ok(TrustLevel::Invalid) => {
                warn!("Whisker: rejected invalid announce signature or payload tampering");
                self.metrics
                    .signature_rejected
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(michi_identity::error::IdentityError::TimestampOutOfWindow) => {
                warn!("Whisker: rejected announce with stale/future timestamp");
                self.metrics
                    .timestamp_rejected
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(michi_identity::error::IdentityError::ReplayDetected) => {
                warn!("Whisker: rejected announce with replayed nonce");
                self.metrics.replay_rejected.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(e) => {
                warn!(err = %e, "Whisker: announce verification error");
                self.metrics
                    .signature_rejected
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Run the persistent UDP multicast receive loop until cancelled.
    pub async fn run(&self, socket: UdpSocket, cancel_token: CancellationToken) {
        let mut buf = [0u8; MAX_ANNOUNCE_BYTES];
        info!(
            group = %self.multicast_group,
            port = %self.multicast_port,
            "Whisker: persistent discovery listener started"
        );

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("Whisker: discovery listener cancelled");
                    break;
                }
                res = socket.recv_from(&mut buf) => {
                    match res {
                        Ok((len, source)) => {
                            let packet = &buf[..len];
                            self.handle_packet(packet, source, Instant::now());
                        }
                        Err(e) => {
                            warn!(err = %e, "Whisker: error receiving UDP datagram");
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use michi_identity::types::{AnnounceProfile, ApiVersion};
    use michi_identity::IdentityManager;

    fn make_test_identity() -> Arc<IdentityManager> {
        let dir = std::env::temp_dir().join(format!("whisker-test-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        Arc::new(
            IdentityManager::generate(&dir, "Stream Standard Test", "")
                .expect("test identity generation"),
        )
    }

    fn make_test_features(key: &str) -> std::collections::BTreeMap<String, bool> {
        let mut map = std::collections::BTreeMap::new();
        map.insert(key.to_string(), true);
        map
    }

    #[test]
    fn test_valid_signed_stream_enters_scent() {
        let stream_id = make_test_identity();
        let engine = Arc::new(DiscoveryEngine::new(stream_id.clone()));
        let scent = Arc::new(ScentStore::new());
        let listener = WhiskerDiscoveryListener::new(engine.clone(), scent.clone());

        let profile = AnnounceProfile {
            device_id: "stream-device-01".into(),
            name: "Living Room Speaker".into(),
            service: Service::StreamStandard,
            roles: vec![Role::AudioReceiver],
            api_version: ApiVersion::V1Lite,
            host: "192.168.1.50".into(),
            port: 8080,
            features: make_test_features("pcm_s16le"),
        };

        let announce = engine
            .build_signed_announce(&profile)
            .expect("signed announce");
        let bytes = serde_json::to_vec(&announce).unwrap();

        let source = "192.168.1.50:53318".parse().unwrap();
        let ok = listener.handle_packet(&bytes, source, Instant::now());
        assert!(ok);

        let michi_id = stream_id.michi_id().to_base64url();
        let record = scent.get(&michi_id).expect("must be in scent");
        assert!(record.verified);
        assert_eq!(record.name, "Living Room Speaker");
        assert_eq!(record.service, "michi-stream-standard");
        assert_eq!(record.roles, vec!["audio_receiver"]);
    }

    #[test]
    fn test_tampered_signature_rejected() {
        let stream_id = make_test_identity();
        let engine = Arc::new(DiscoveryEngine::new(stream_id));
        let scent = Arc::new(ScentStore::new());
        let listener = WhiskerDiscoveryListener::new(engine.clone(), scent.clone());

        let profile = AnnounceProfile {
            device_id: "stream-device-02".into(),
            name: "Living Room Speaker".into(),
            service: Service::StreamStandard,
            roles: vec![Role::AudioReceiver],
            api_version: ApiVersion::V1Lite,
            host: "192.168.1.51".into(),
            port: 8080,
            features: make_test_features("pcm_s16le"),
        };

        let mut announce = engine
            .build_signed_announce(&profile)
            .expect("signed announce");
        // Tamper with name after signing
        announce.name = "Tampered Speaker".into();
        let bytes = serde_json::to_vec(&announce).unwrap();

        let source = "192.168.1.51:53318".parse().unwrap();
        let ok = listener.handle_packet(&bytes, source, Instant::now());
        assert!(!ok);
        assert_eq!(
            listener
                .metrics()
                .signature_rejected
                .load(Ordering::Relaxed),
            1
        );
        assert!(scent.list().is_empty());
    }

    #[test]
    fn test_replayed_nonce_rejected() {
        let stream_id = make_test_identity();
        let engine = Arc::new(DiscoveryEngine::new(stream_id));
        let scent = Arc::new(ScentStore::new());
        let listener = WhiskerDiscoveryListener::new(engine.clone(), scent.clone());

        let profile = AnnounceProfile {
            device_id: "stream-device-03".into(),
            name: "Bedroom Speaker".into(),
            service: Service::StreamStandard,
            roles: vec![Role::AudioReceiver],
            api_version: ApiVersion::V1Lite,
            host: "192.168.1.52".into(),
            port: 8080,
            features: make_test_features("pcm_s16le"),
        };

        let announce = engine
            .build_signed_announce(&profile)
            .expect("signed announce");
        let bytes = serde_json::to_vec(&announce).unwrap();
        let source = "192.168.1.52:53318".parse().unwrap();

        // First packet accepted
        assert!(listener.handle_packet(&bytes, source, Instant::now()));

        // Exact same replay rejected
        assert!(!listener.handle_packet(&bytes, source, Instant::now()));
        assert_eq!(
            listener.metrics().replay_rejected.load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn test_non_stream_service_filtered() {
        let player_id = make_test_identity();
        let engine = Arc::new(DiscoveryEngine::new(player_id));
        let scent = Arc::new(ScentStore::new());
        let listener = WhiskerDiscoveryListener::new(engine.clone(), scent.clone());

        let profile = AnnounceProfile {
            device_id: "player-device-01".into(),
            name: "Desktop Player".into(),
            service: Service::MusicPlayer,
            roles: vec![Role::DesktopPlayer, Role::LibraryMaster, Role::SyncHost],
            api_version: ApiVersion::V1,
            host: "192.168.1.10".into(),
            port: 9090,
            features: make_test_features("flac"),
        };

        let announce = engine
            .build_signed_announce(&profile)
            .expect("signed announce");
        let bytes = serde_json::to_vec(&announce).unwrap();
        let source = "192.168.1.10:53318".parse().unwrap();

        // Valid signature for a MusicPlayer, but must be filtered from receiver scent
        let ok = listener.handle_packet(&bytes, source, Instant::now());
        assert!(!ok);
        assert_eq!(
            listener
                .metrics()
                .non_stream_filtered
                .load(Ordering::Relaxed),
            1
        );
        assert!(scent.list().is_empty());
    }
}
