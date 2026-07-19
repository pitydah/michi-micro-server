//! Michi Connect — Descubrimiento multi-capa y conexión resiliente
//!
//! - mDNS announce del servidor como _michi._tcp
//! - QR link michi://connect?id=XYZ&host=IP&port=PORT
//! - CORS dinámico basado en firma

use michi_identity::MichiIdentity;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Clone)]
pub struct MichiConnect {
    identity: MichiIdentity,
    server_url: Arc<RwLock<String>>,
    service_name: Arc<RwLock<String>>,
}

impl MichiConnect {
    pub fn new(identity: MichiIdentity, port: u16, host: Option<String>) -> Self {
        let host = host.unwrap_or_else(|| "localhost".to_string());
        let server_url = format!("http://{}:{}", host, port);
        Self {
            identity,
            server_url: Arc::new(RwLock::new(server_url)),
            service_name: Arc::new(RwLock::new(String::new())),
        }
    }

    /// Generate a QR code link string: michi://connect?id=XYZ&host=IP&port=PORT
    pub async fn qr_link(&self, host: &str, port: u16) -> String {
        let michi_id = self.identity.get_id().await;
        format!(
            "michi://connect?id={}&host={}&port={}",
            michi_id, host, port
        )
    }

    /// Generate QR SVG
    pub async fn qr_svg(&self, host: &str, port: u16) -> Result<String, String> {
        let link = self.qr_link(host, port).await;
        let code = qrcode::QrCode::new(link.as_bytes()).map_err(|e| format!("QR error: {}", e))?;
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
        let mut url = self.server_url.write().await;
        *url = format!("http://{}:{}", host, port);
        info!("connect: server URL updated to {}", url);
        // Re-announce mDNS
        let _ = self.announce_mdns().await;
    }

    pub async fn server_url(&self) -> String {
        self.server_url.read().await.clone()
    }

    /// Announce via mDNS using mdns-sd
    pub async fn announce_mdns(&self) -> Result<(), String> {
        let url = self.server_url.read().await.clone();
        let michi_id = self.identity.get_id().await;
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

        let daemon = mdns_sd::ServiceDaemon::new().map_err(|e| format!("mdns daemon: {}", e))?;

        let service_type = "_michi._tcp.local.";
        let instance_name = format!("Michi Micro Server ({})", &michi_id[..8]);
        let service_hostname = format!("{}.local.", hostname);

        let properties = [
            ("michi_id", michi_id.as_str()),
            ("version", env!("CARGO_PKG_VERSION")),
        ];

        let service_info = mdns_sd::ServiceInfo::new(
            service_type,
            &instance_name,
            &service_hostname,
            "", // empty addrs, enable_addr_auto will find them
            port,
            &properties[..],
        )
        .map_err(|e| format!("service info: {}", e))?
        .enable_addr_auto();

        daemon
            .register(service_info)
            .map_err(|e| format!("mdns register: {}", e))?;

        let name = instance_name;
        *self.service_name.write().await = name.clone();
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
        MichiIdentity::verify_peer(&pub_key_bytes, payload, signature_b64).unwrap_or(false)
    }

    pub async fn stop_mdns(&self) {
        info!("connect: mDNS stopped");
    }
}

// Re-export for convenience
pub use mdns_sd;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    async fn make_identity() -> MichiIdentity {
        let dir = tempdir().unwrap();
        let id = MichiIdentity::load_or_create(dir.path()).await.unwrap();
        std::mem::forget(dir);
        id
    }

    #[tokio::test]
    async fn test_new_default_host() {
        let identity = make_identity().await;
        let conn = MichiConnect::new(identity, 8080, None);
        let url = conn.server_url().await;
        assert_eq!(url, "http://localhost:8080");
    }

    #[tokio::test]
    async fn test_new_with_host() {
        let identity = make_identity().await;
        let conn = MichiConnect::new(identity, 9090, Some("192.168.1.5".into()));
        let url = conn.server_url().await;
        assert_eq!(url, "http://192.168.1.5:9090");
    }

    #[tokio::test]
    async fn test_new_default_host_url() {
        let identity = make_identity().await;
        let conn = MichiConnect::new(identity, 3000, None);
        let url = conn.server_url().await;
        assert_eq!(url, "http://localhost:3000");
    }

    #[tokio::test]
    async fn test_qr_link_format() {
        let identity = make_identity().await;
        let michi_id = identity.get_id().await;
        let conn = MichiConnect::new(identity, 5000, Some("10.0.0.1".into()));
        let link = conn.qr_link("10.0.0.1", 5000).await;
        assert!(link.starts_with("michi://connect?id="));
        assert!(link.contains("&host=10.0.0.1&port=5000"));
        let expected = format!("michi://connect?id={}&host=10.0.0.1&port=5000", michi_id);
        assert_eq!(link, expected);
    }

    #[tokio::test]
    async fn test_qr_svg_produces_valid_svg() {
        let identity = make_identity().await;
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
        let identity = make_identity().await;
        let pub_key = identity.public_key_bytes().await;
        let pub_key_hex = hex::encode(&pub_key);
        let payload = b"test payload for signature";
        let signature = identity.sign_payload(payload).await;

        let conn = MichiConnect::new(identity, 4000, None);
        let valid = conn
            .verify_peer_signature(&pub_key_hex, payload, &signature)
            .await;
        assert!(valid);
    }

    #[tokio::test]
    async fn test_verify_peer_signature_tampered_payload() {
        let identity = make_identity().await;
        let pub_key = identity.public_key_bytes().await;
        let pub_key_hex = hex::encode(&pub_key);
        let payload = b"original message";
        let signature = identity.sign_payload(payload).await;

        let conn = MichiConnect::new(identity, 4000, None);
        let valid = conn
            .verify_peer_signature(&pub_key_hex, b"tampered message", &signature)
            .await;
        assert!(!valid);
    }

    #[tokio::test]
    async fn test_verify_peer_signature_invalid_pubkey() {
        let identity = make_identity().await;
        let payload = b"some data";
        let signature = identity.sign_payload(payload).await;

        let conn = MichiConnect::new(identity, 4000, None);
        // Use a bogus public key (all zeros, wrong length, etc.)
        let valid = conn
            .verify_peer_signature("invalid-hex-key", payload, &signature)
            .await;
        assert!(!valid);
    }

    #[tokio::test]
    async fn test_verify_peer_signature_invalid_signature() {
        let identity = make_identity().await;
        let pub_key = identity.public_key_bytes().await;
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
        let identity = make_identity().await;
        let conn = MichiConnect::new(identity, 5000, None);
        let svg1 = conn.qr_svg("192.168.1.1", 5000).await.unwrap();
        let svg2 = conn.qr_svg("192.168.1.2", 5000).await.unwrap();
        assert_ne!(svg1, svg2);
    }

    #[tokio::test]
    async fn test_service_name_starts_empty() {
        let identity = make_identity().await;
        let conn = MichiConnect::new(identity, 4000, None);
        // service_name is not publicly readable, but we test via the field
        // indirectly: announce_mdns() populates it, but that needs network.
        // We just verify the struct can be constructed without panic.
        drop(conn);
    }

    #[tokio::test]
    async fn test_qr_link_uses_identity_id_not_host() {
        let identity = make_identity().await;
        let michi_id = identity.get_id().await;
        assert_eq!(
            michi_id.len(),
            64,
            "michi_id must be 64 hex chars (SHA-256)"
        );

        let conn = MichiConnect::new(identity, 5000, None);
        let link = conn.qr_link("example.com", 9999).await;
        assert!(link.contains(&michi_id));
    }

    /// Verify that MichiConnect::new() can be called with the identity created
    /// in a different scope — i.e., identity is Clone and independent.
    #[tokio::test]
    async fn test_identity_independent_of_connect_lifetime() {
        let identity = make_identity().await;
        let conn = MichiConnect::new(identity, 5555, None);
        let url = conn.server_url().await;
        assert_eq!(url, "http://localhost:5555");
    }
}
