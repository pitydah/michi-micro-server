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
        format!("michi://connect?id={}&host={}&port={}", michi_id, host, port)
    }

    /// Generate QR SVG
    pub async fn qr_svg(&self, host: &str, port: u16) -> Result<String, String> {
        let link = self.qr_link(host, port).await;
        let code = qrcode::QrCode::new(link.as_bytes())
            .map_err(|e| format!("QR error: {}", e))?;
        let svg = code.render()
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
        let hostname = hostonly.split(':').next().unwrap_or("localhost").to_string();
        let port: u16 = hostonly.split(':').nth(1).unwrap_or("9090").parse().unwrap_or(9090);

        let daemon = mdns_sd::ServiceDaemon::new()
            .map_err(|e| format!("mdns daemon: {}", e))?;

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
            "",  // empty addrs, enable_addr_auto will find them
            port,
            &properties[..],
        )
        .map_err(|e| format!("service info: {}", e))?
        .enable_addr_auto();

        daemon.register(service_info)
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
