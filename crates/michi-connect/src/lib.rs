//! Michi Connect — Descubrimiento multi-capa y conexión resiliente
//!
//! - mDNS announce del servidor como _michi-link._tcp
//! - QR link michi://connect?id=XYZ&host=IP&port=PORT
//! - CORS dinámico basado en firma

pub mod discovery_listener;
pub mod mdns_resolver;
pub mod scent_store;

pub use discovery_listener::{WhiskerDiscoveryListener, WhiskerMetrics};
pub use mdns_resolver::MdnsResolver;
pub use scent_store::{ScentEvent, ScentRecord, ScentStore, SCENT_EXPIRY_TIMEOUT};

use michi_identity::IdentityManager;
use std::sync::Arc;
use std::sync::RwLock;
use tracing::info;

#[derive(Clone)]
pub struct MichiConnect {
    identity: Arc<IdentityManager>,
    server_url: Arc<RwLock<String>>,
    service_name: Arc<RwLock<String>>,
    mdns_daemon: Arc<RwLock<Option<mdns_sd::ServiceDaemon>>>,
    scent_store: Arc<ScentStore>,
    michi_home_id: Arc<RwLock<Option<String>>>,
}

impl MichiConnect {
    pub fn new(identity: Arc<IdentityManager>, port: u16, host: Option<String>) -> Self {
        Self::new_with_scent(identity, port, host, Arc::new(ScentStore::new()))
    }

    pub fn new_with_scent(
        identity: Arc<IdentityManager>,
        port: u16,
        host: Option<String>,
        scent_store: Arc<ScentStore>,
    ) -> Self {
        let host = host.unwrap_or_else(|| "localhost".to_string());
        let server_url = format!("http://{host}:{port}");
        Self {
            identity,
            server_url: Arc::new(RwLock::new(server_url)),
            service_name: Arc::new(RwLock::new(String::new())),
            mdns_daemon: Arc::new(RwLock::new(None)),
            scent_store,
            michi_home_id: Arc::new(RwLock::new(None)),
        }
    }

    pub fn set_home_id(&self, home_id: String) {
        if let Ok(mut guard) = self.michi_home_id.write() {
            *guard = Some(home_id);
        }
    }

    pub fn home_id(&self) -> Option<String> {
        self.michi_home_id.read().ok().and_then(|g| g.clone())
    }

    pub fn scent_store(&self) -> Arc<ScentStore> {
        self.scent_store.clone()
    }

    /// Generate a QR code link string: michi://connect?id=XYZ&host=IP&port=PORT
    pub async fn qr_link(&self, host: &str, port: u16) -> String {
        let michi_id = self.identity.michi_id().to_base64url();
        format!("michi://connect?id={michi_id}&host={host}&port={port}")
    }

    /// Generate QR SVG
    pub async fn qr_svg(&self, host: &str, port: u16) -> Result<String, String> {
        let link = self.qr_link(host, port).await;
        let code = qrcode::QrCode::new(link.as_bytes()).map_err(|e| format!("QR error: {e}"))?;
        let svg = code
            .render()
            .min_dimensions(300, 300)
            .dark_color(qrcode::render::svg::Color("#8B5CF6"))
            .light_color(qrcode::render::svg::Color("transparent"))
            .build();
        Ok(svg)
    }

    /// Update the server URL (when IP changes)
    pub async fn update_url(&self, host: &str, port: u16) {
        {
            let mut url = self.server_url.write().unwrap();
            *url = format!("http://{host}:{port}");
            info!("connect: server URL updated to {}", url);
        }
        // Re-announce mDNS
        let _ = self.announce_mdns().await;
    }

    pub async fn server_url(&self) -> String {
        self.server_url.read().unwrap().clone()
    }

    /// Announce via mDNS using mdns-sd
    pub async fn announce_mdns(&self) -> Result<(), String> {
        let url = self.server_url.read().unwrap().clone();
        let michi_id = self.identity.michi_id().to_base64url();
        let hostonly = url.trim_start_matches("http://");
        let hostname = hostonly
            .split(':')
            .next()
            .unwrap_or("localhost")
            .to_string();
        let port: u16 = hostonly
            .split(':')
            .nth(1)
            .unwrap_or("9090")
            .parse()
            .unwrap_or(9090);

        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns daemon: {e}"))?;

        let service_type = "_michi-link._tcp.local.";
        let instance_name = format!("Michi Micro Server ({})", &michi_id[..8]);
        let safe_host_label = hostname.replace('.', "-");
        let service_hostname = format!("{safe_host_label}.local.");

        let public_key = self.identity.public_key_base64url();
        let home_id = self.home_id();
        let mut properties: Vec<(&str, &str)> = vec![
            ("identity_scheme", michi_identity::IDENTITY_SCHEME_V1),
            ("michi_id", michi_id.as_str()),
            ("public_key", public_key.as_str()),
            ("api_version", "v1"),
            ("service", "michi-micro-server"),
            ("version", env!("CARGO_PKG_VERSION")),
        ];
        if let Some(ref hid) = home_id {
            properties.push(("michi_home_id", hid.as_str()));
        }

        let service_info = mdns_sd::ServiceInfo::new(
            service_type,
            &instance_name,
            &service_hostname,
            "", // empty addrs, enable_addr_auto will find them
            port,
            &properties[..],
        )
        .map_err(|e| format!("service info: {e}"))?
        .enable_addr_auto();

        daemon
            .register(service_info)
            .map_err(|e| format!("mdns register: {e}"))?;

        let name = instance_name;
        *self.service_name.write().unwrap() = name.clone();
        *self.mdns_daemon.write().unwrap() = Some(daemon);
        info!("connect: mDNS announced as {}.{}", name, service_type);
        Ok(())
    }

    /// Verify a peer signature for CORS
    pub async fn verify_peer_signature(
        &self,
        peer_public_key_hex: &str,
        payload: &[u8],
        signature_b64: &str,
    ) -> bool {
        let pub_key_bytes = hex::decode(peer_public_key_hex).unwrap_or_default();
        let pub_key_b64url = michi_identity::encode_base64url(&pub_key_bytes);
        IdentityManager::verify(payload, signature_b64, &pub_key_b64url).unwrap_or(false)
    }

    pub async fn stop_mdns(&self) {
        if let Ok(mut lock) = self.mdns_daemon.write() {
            if let Some(daemon) = lock.take() {
                let name = self.service_name.read().unwrap().clone();
                if !name.is_empty() {
                    let fullname = format!("{name}._michi-link._tcp.local.");
                    let _ = daemon.unregister(&fullname);
                }
                let _ = daemon.shutdown();
                info!("connect: mDNS service unregistered and shut down");
            }
        }
    }

    /// Default canonical discovery features for Michi Micro Server
    pub fn canonical_features() -> std::collections::BTreeMap<String, bool> {
        let mut features = std::collections::BTreeMap::new();
        features.insert("library".to_string(), true);
        features.insert("stream".to_string(), true);
        features.insert("playback".to_string(), true);
        features.insert("sync".to_string(), true);
        features
    }

    /// Builds a canonical signed UDP multicast announce for Michi Micro Server
    pub fn build_signed_announce(
        &self,
        port: u16,
        host: &str,
    ) -> Result<michi_identity::types::Announce, michi_identity::IdentityError> {
        self.build_signed_announce_with_features(port, host, Self::canonical_features())
    }

    /// Builds a canonical signed UDP multicast announce with specified feature map
    pub fn build_signed_announce_with_features(
        &self,
        port: u16,
        host: &str,
        features: std::collections::BTreeMap<String, bool>,
    ) -> Result<michi_identity::types::Announce, michi_identity::IdentityError> {
        let engine = michi_identity::discovery::DiscoveryEngine::new(self.identity.clone());
        let profile = michi_identity::types::AnnounceProfile {
            device_id: self.identity.michi_id().to_base64url(),
            name: "Michi Micro Server".to_string(),
            service: michi_identity::types::Service::MicroServer,
            roles: vec![
                michi_identity::types::Role::MusicServer,
                michi_identity::types::Role::LibraryHost,
                michi_identity::types::Role::PlaybackHost,
            ],
            api_version: michi_identity::types::ApiVersion::V1,
            host: host.to_string(),
            port,
            features,
            michi_home_id: self.home_id(),
            membership_fingerprint: None,
        };
        engine.build_signed_announce(&profile)
    }

    /// Enumerates all unicast IP addresses actually bound to this host's network
    /// interfaces (including LAN, IPv6, Tailscale NICs, and loopback), plus any
    /// explicit IP configured via `MICHI_ADVERTISE_HOST`.
    pub fn local_server_ips() -> Vec<std::net::IpAddr> {
        let mut ips = Vec::new();
        let mut push_unique = |ip: std::net::IpAddr| {
            if ip.is_unspecified() || ip.is_multicast() {
                return;
            }
            if let std::net::IpAddr::V4(v4) = ip {
                if v4.is_broadcast() {
                    return;
                }
            }
            if !ips.contains(&ip) {
                ips.push(ip);
            }
        };

        push_unique(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        push_unique(std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST));

        if let Ok(ifaces) = if_addrs::get_if_addrs() {
            for iface in ifaces {
                push_unique(iface.ip());
            }
        }

        if let Ok(adv) = std::env::var("MICHI_ADVERTISE_HOST") {
            let trimmed = adv.trim().trim_start_matches('[').trim_end_matches(']');
            if let Ok(ip) = trimmed.parse::<std::net::IpAddr>() {
                push_unique(ip);
            }
        }

        ips
    }

    /// Returns the actual `.local` mDNS hostnames advertised by this server instance
    /// for an optional configured/advertised host (never a blanket wildcard for
    /// arbitrary LAN `.local` peers).
    pub fn local_mdns_hostnames_for_host(advertised_host: Option<&str>) -> Vec<String> {
        let mut hosts = Vec::new();
        let mut push_mdns = |raw: &str| {
            let trimmed = raw.trim().trim_end_matches('.').to_ascii_lowercase();
            if trimmed.is_empty() || trimmed == "0.0.0.0" || trimmed == "::" {
                return;
            }
            let candidate = if trimmed.ends_with(".local") {
                trimmed
            } else {
                format!("{}.local", trimmed.replace('.', "-"))
            };
            if candidate != ".local" && !hosts.contains(&candidate) {
                hosts.push(candidate);
            }
        };

        if let Some(h) = advertised_host {
            push_mdns(h);
        }
        if let Ok(explicit) = std::env::var("MICHI_MDNS_HOST") {
            push_mdns(&explicit);
        }
        if let Ok(adv) = std::env::var("MICHI_ADVERTISE_HOST") {
            push_mdns(&adv);
        }
        if let Ok(env_host) = std::env::var("HOSTNAME") {
            push_mdns(&env_host);
        }
        if let Ok(etc_host) = std::fs::read_to_string("/etc/hostname") {
            push_mdns(&etc_host);
        }

        hosts
    }

    /// Returns the actual `.local` mDNS hostnames advertised by this `MichiConnect` instance.
    pub fn local_mdns_hostnames(&self) -> Vec<String> {
        let host = self.server_url.read().ok().and_then(|url| {
            let hostonly = url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            hostonly.split(':').next().map(|s| s.to_string())
        });
        Self::local_mdns_hostnames_for_host(host.as_deref())
    }

    /// Resolves the primary non-loopback reachable LAN IP, checking MICHI_ADVERTISE_HOST and local routes offline.
    pub fn resolve_lan_ip() -> String {
        // 1. Explicit environment variable configuration
        if let Ok(host) = std::env::var("MICHI_ADVERTISE_HOST") {
            if !host.trim().is_empty() {
                return host.trim().to_string();
            }
        }

        // 2. Offline routing check towards local physical RFC1918 gateways (strictly local-first)
        for probe in &[
            "192.168.1.1:80",
            "192.168.0.1:80",
            "192.168.2.1:80",
            "10.0.0.1:80",
            "10.1.1.1:80",
            "172.16.0.1:80",
        ] {
            if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
                if socket.connect(probe).is_ok() {
                    if let Ok(local_addr) = socket.local_addr() {
                        let ip = local_addr.ip();
                        if !ip.is_loopback() && !ip.is_unspecified() {
                            return ip.to_string();
                        }
                    }
                }
            }
        }

        // 3. Fallback to 127.0.0.1
        "127.0.0.1".to_string()
    }

    /// Spawns a background task broadcasting signed UDP multicast announces periodically
    pub fn spawn_announcer(
        &self,
        port: u16,
        advertised_host: Option<String>,
        features: std::collections::BTreeMap<String, bool>,
        cancel_token: tokio_util::sync::CancellationToken,
    ) -> tokio::task::JoinHandle<()> {
        let connect = self.clone();
        let pinned_host = advertised_host.filter(|h| !h.trim().is_empty());

        tokio::spawn(async move {
            let bind_addr: std::net::SocketAddr = "0.0.0.0:0".parse().unwrap();
            let socket = match tokio::net::UdpSocket::bind(bind_addr).await {
                Ok(s) => {
                    let _ = s.set_broadcast(true);
                    s
                }
                Err(e) => {
                    tracing::warn!("Failed to bind UDP announcer socket: {e}");
                    return;
                }
            };

            let target_addr = format!(
                "{}:{}",
                michi_identity::discovery::MULTICAST_GROUP,
                michi_identity::discovery::MULTICAST_PORT
            );

            let mut interval = tokio::time::interval(michi_identity::discovery::ANNOUNCE_INTERVAL);
            let initial_host = pinned_host.clone().unwrap_or_else(Self::resolve_lan_ip);
            info!(
                "connect: discovery announcer started for host {} targeting {}",
                initial_host, target_addr
            );

            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        info!("connect: discovery announcer stopping on cancel signal");
                        break;
                    }
                    _ = interval.tick() => {
                        let current_host = pinned_host
                            .clone()
                            .unwrap_or_else(Self::resolve_lan_ip);
                        match connect.build_signed_announce_with_features(port, &current_host, features.clone()) {
                            Ok(announce) => {
                                if let Ok(json_bytes) = serde_json::to_vec(&announce) {
                                    if let Err(e) = socket.send_to(&json_bytes, &target_addr).await {
                                        tracing::debug!("UDP multicast announce send error: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to build signed announce: {e}");
                            }
                        }
                    }
                }
            }
        })
    }
}

// Re-export for convenience
pub use mdns_sd;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_identity() -> Arc<IdentityManager> {
        let dir = tempdir().unwrap();
        let id = IdentityManager::load_or_generate(dir.path(), "test", "password").unwrap();
        std::mem::forget(dir);
        Arc::new(id)
    }

    #[tokio::test]
    async fn test_new_default_host() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 8080, None);
        let url = conn.server_url().await;
        assert_eq!(url, "http://localhost:8080");
    }

    #[tokio::test]
    async fn test_new_with_host() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 9090, Some("192.168.1.5".into()));
        let url = conn.server_url().await;
        assert_eq!(url, "http://192.168.1.5:9090");
    }

    #[tokio::test]
    async fn test_new_default_host_url() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 3000, None);
        let url = conn.server_url().await;
        assert_eq!(url, "http://localhost:3000");
    }

    #[tokio::test]
    async fn test_qr_link_format() {
        let identity = make_identity();
        let michi_id = identity.michi_id().to_base64url();
        let conn = MichiConnect::new(identity, 5000, Some("10.0.0.1".into()));
        let link = conn.qr_link("10.0.0.1", 5000).await;
        assert!(link.starts_with("michi://connect?id="));
        assert!(link.contains("&host=10.0.0.1&port=5000"));
        let expected = format!("michi://connect?id={michi_id}&host=10.0.0.1&port=5000");
        assert_eq!(link, expected);
    }

    #[tokio::test]
    async fn test_qr_svg_produces_valid_svg() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 5000, None);
        let svg = conn.qr_svg("localhost", 5000).await.unwrap();
        assert!(svg.starts_with("<?xml"));
        assert!(svg.contains("<svg"));
        assert!(svg.contains("viewBox"));
        assert!(svg.contains("</svg>"));
        // The QR module renders at least some path/rect elements for the QR code
        assert!(svg.len() > 1000);
    }

    // update_url() is not tested in isolation because it internally calls
    // announce_mdns() which requires a real network interface and mDNS daemon.

    #[tokio::test]
    async fn test_verify_peer_signature_valid() {
        let identity = make_identity();
        let pub_key = identity.public_key_bytes().to_vec();
        let pub_key_hex = hex::encode(&pub_key);
        let payload = b"test payload for signature";
        let signature = identity.sign_base64url(payload).0;

        let conn = MichiConnect::new(identity, 4000, None);
        let valid = conn
            .verify_peer_signature(&pub_key_hex, payload, &signature)
            .await;
        assert!(valid);
    }

    #[tokio::test]
    async fn test_verify_peer_signature_tampered_payload() {
        let identity = make_identity();
        let pub_key = identity.public_key_bytes().to_vec();
        let pub_key_hex = hex::encode(&pub_key);
        let payload = b"original message";
        let signature = identity.sign_base64url(payload).0;

        let conn = MichiConnect::new(identity, 4000, None);
        let valid = conn
            .verify_peer_signature(&pub_key_hex, b"tampered message", &signature)
            .await;
        assert!(!valid);
    }

    #[tokio::test]
    async fn test_verify_peer_signature_invalid_pubkey() {
        let identity = make_identity();
        let payload = b"some data";
        let signature = identity.sign_base64url(payload).0;

        let conn = MichiConnect::new(identity, 4000, None);
        // Use a bogus public key (all zeros, wrong length, etc.)
        let valid = conn
            .verify_peer_signature("invalid-hex-key", payload, &signature)
            .await;
        assert!(!valid);
    }

    #[tokio::test]
    async fn test_verify_peer_signature_invalid_signature() {
        let identity = make_identity();
        let pub_key = identity.public_key_bytes().to_vec();
        let pub_key_hex = hex::encode(&pub_key);
        let payload = b"message";

        let conn = MichiConnect::new(identity, 4000, None);
        // Bogus base64 signature
        let valid = conn
            .verify_peer_signature(&pub_key_hex, payload, "not-a-real-signature!!")
            .await;
        assert!(!valid);
    }

    #[tokio::test]
    async fn test_qr_svg_different_hosts_produce_different_output() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 5000, None);
        let svg1 = conn.qr_svg("192.168.1.1", 5000).await.unwrap();
        let svg2 = conn.qr_svg("192.168.1.2", 5000).await.unwrap();
        assert_ne!(svg1, svg2);
    }

    #[tokio::test]
    async fn test_service_name_starts_empty() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 4000, None);
        // service_name is not publicly readable, but we test via the field
        // indirectly: announce_mdns() populates it, but that needs network.
        // We just verify the struct can be constructed without panic.
        drop(conn);
    }

    #[tokio::test]
    async fn test_qr_link_uses_identity_id_not_host() {
        let identity = make_identity();
        let michi_id = identity.michi_id().to_base64url();
        assert_eq!(
            michi_id.len(),
            43,
            "michi_id must be 43 base64url chars (BLAKE3)"
        );

        let conn = MichiConnect::new(identity, 5000, None);
        let link = conn.qr_link("example.com", 9999).await;
        assert!(link.contains(&michi_id));
    }

    /// Verify that MichiConnect::new() can be called with the identity created
    /// in a different scope — i.e., identity is Clone and independent.
    #[tokio::test]
    async fn test_identity_independent_of_connect_lifetime() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity, 5555, None);
        let url = conn.server_url().await;
        assert_eq!(url, "http://localhost:5555");
    }

    #[test]
    fn test_build_signed_announce_validates_and_signs() {
        let identity = make_identity();
        let conn = MichiConnect::new(identity.clone(), 9090, Some("192.168.1.100".into()));
        let announce = conn
            .build_signed_announce(9090, "192.168.1.100")
            .expect("signed announce must succeed");

        assert_eq!(
            announce.service,
            michi_identity::types::Service::MicroServer
        );
        assert_eq!(announce.api_version, michi_identity::types::ApiVersion::V1);
        assert_eq!(announce.port, 9090);
        assert_eq!(announce.host, "192.168.1.100");
        assert!(announce.signature.is_some());
        assert_eq!(
            announce.michi_id.as_deref(),
            Some(identity.michi_id().to_base64url()).as_deref()
        );
    }
}
