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
}

impl MichiConnect {
    pub fn new(identity: MichiIdentity, port: u16, host: Option<String>) -> Self {
        let host = host.unwrap_or_else(|| "localhost".to_string());
        let server_url = format!("http://{}:{}", host, port);
        Self {
            identity,
            server_url: Arc::new(RwLock::new(server_url)),
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
    }

    pub async fn server_url(&self) -> String {
        self.server_url.read().await.clone()
    }
}

pub async fn announce_mdns() -> Result<(), String> {
    // Placeholder: full mDNS implementation requires mdns-sd crate
    info!("connect: mDNS announce placeholder (real impl needs mdns-sd)");
    Ok(())
}
