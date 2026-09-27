use crate::scent_store::ScentStore;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

/// Canonical mDNS service type for Michi Link peers.
pub const MDNS_SERVICE_TYPE: &str = "_michi-link._tcp.local.";

pub struct MdnsResolver {
    scent: Arc<ScentStore>,
    http_client: reqwest::Client,
}

impl MdnsResolver {
    pub fn new(scent: Arc<ScentStore>) -> Self {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(3))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self { scent, http_client }
    }

    /// Construct a valid URL from an IP and port. Handles IPv6 brackets correctly.
    pub fn format_base_url(host: &str, port: u16) -> Result<Url, url::ParseError> {
        if host.contains(':') && !host.starts_with('[') {
            Url::parse(&format!("http://[{host}]:{port}/"))
        } else {
            Url::parse(&format!("http://{host}:{port}/"))
        }
    }

    /// Query the peer's GET /api/v1/server/info and verify that the returned michi_id
    /// exactly matches the expected michi_id before trusting this base_url.
    pub async fn verify_identity_endpoint(
        &self,
        base_url: &Url,
        expected_michi_id: &str,
    ) -> Result<bool, String> {
        let info_url = base_url
            .join("api/v1/server/info")
            .map_err(|e| format!("invalid URL join: {e}"))?;

        let resp = self
            .http_client
            .get(info_url)
            .send()
            .await
            .map_err(|e| format!("server/info request failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("server/info returned status {}", resp.status()));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse server/info json: {e}"))?;

        let returned_michi_id = body
            .get("michi_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if returned_michi_id.is_empty() {
            return Err("server/info did not return a michi_id".into());
        }

        if returned_michi_id == expected_michi_id {
            Ok(true)
        } else {
            warn!(
                expected = %expected_michi_id,
                actual = %returned_michi_id,
                url = %base_url,
                "MdnsResolver: server/info returned mismatching michi_id! Rejecting endpoint."
            );
            Ok(false)
        }
    }

    /// Process a resolved mDNS service info.
    pub async fn handle_resolved_service(&self, info: &mdns_sd::ServiceInfo) {
        let michi_id = match info.get_property_val_str("michi_id") {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => {
                debug!("MdnsResolver: discovered service without michi_id property, ignoring");
                return;
            }
        };

        let port = info.get_port();
        let addresses = info.get_addresses();

        if addresses.is_empty() {
            debug!(michi_id = %michi_id, "MdnsResolver: resolved service has no IP addresses");
            return;
        }

        for ip in addresses {
            let socket_addr = SocketAddr::new(*ip, port);
            let candidate_url = match Self::format_base_url(&ip.to_string(), port) {
                Ok(u) => u,
                Err(e) => {
                    warn!(err = %e, "MdnsResolver: failed to format candidate URL");
                    continue;
                }
            };

            debug!(
                michi_id = %michi_id,
                url = %candidate_url,
                "MdnsResolver: probing candidate endpoint via server/info"
            );

            match self
                .verify_identity_endpoint(&candidate_url, &michi_id)
                .await
            {
                Ok(true) => {
                    info!(
                        michi_id = %michi_id,
                        url = %candidate_url,
                        "MdnsResolver: verified identity endpoint; updating Scent base_url"
                    );
                    let now = Instant::now();
                    self.scent
                        .update_base_url(&michi_id, candidate_url, Some(socket_addr), now);
                    self.scent.mark_server_info_verified(&michi_id, now);
                    break;
                }
                Ok(false) => {
                    // Identity mismatch, skip this endpoint
                }
                Err(e) => {
                    debug!(
                        michi_id = %michi_id,
                        url = %candidate_url,
                        err = %e,
                        "MdnsResolver: candidate endpoint verification failed"
                    );
                }
            }
        }
    }

    /// Run the persistent mDNS resolver loop until cancelled.
    pub async fn run(&self, daemon: ServiceDaemon, cancel_token: CancellationToken) {
        let receiver = match daemon.browse(MDNS_SERVICE_TYPE) {
            Ok(rx) => rx,
            Err(e) => {
                warn!(err = %e, "MdnsResolver: failed to browse mDNS service type");
                return;
            }
        };

        info!("MdnsResolver: persistent mDNS service resolver started");

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("MdnsResolver: service resolver cancelled");
                    let _ = daemon.stop_browse(MDNS_SERVICE_TYPE);
                    break;
                }
                event_opt = receiver.recv_async() => {
                    match event_opt {
                        Ok(ServiceEvent::ServiceResolved(info)) => {
                            self.handle_resolved_service(&info).await;
                        }
                        Ok(ServiceEvent::ServiceRemoved(_service_type, fullname)) => {
                            debug!(fullname = %fullname, "MdnsResolver: service removed");
                        }
                        Ok(_) => {}
                        Err(e) => {
                            warn!(err = %e, "MdnsResolver: browse channel error");
                            break;
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

    #[test]
    fn test_format_base_url_ipv4() {
        let url = MdnsResolver::format_base_url("192.168.1.50", 8080).unwrap();
        assert_eq!(url.as_str(), "http://192.168.1.50:8080/");
    }

    #[test]
    fn test_format_base_url_ipv6() {
        let url = MdnsResolver::format_base_url("fe80::1", 8080).unwrap();
        assert_eq!(url.as_str(), "http://[fe80::1]:8080/");
    }
}
