use crate::scent_store::{ScentStore, VerifiedServerInfo};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use michi_identity::IdentityManager;
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

    /// Query the peer's GET /api/v1/server/info and verify that the returned michi_id,
    /// Ed25519 public key, service type, and audio_receiver role match before trusting this base_url.
    pub async fn verify_identity_endpoint(
        &self,
        base_url: &Url,
        expected_michi_id: &str,
    ) -> Result<Option<VerifiedServerInfo>, String> {
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

        if returned_michi_id != expected_michi_id {
            warn!(
                expected = %expected_michi_id,
                actual = %returned_michi_id,
                url = %base_url,
                "MdnsResolver: server/info returned mismatching michi_id! Rejecting endpoint."
            );
            return Ok(None);
        }

        let public_key = body
            .get("public_key")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if public_key.is_empty() {
            warn!(
                url = %base_url,
                "MdnsResolver: server/info missing public_key! Rejecting endpoint."
            );
            return Ok(None);
        }

        let derived_michi_id = match IdentityManager::derive_michi_id(public_key) {
            Ok(id) => id.to_base64url(),
            Err(e) => {
                warn!(
                    err = %e,
                    url = %base_url,
                    "MdnsResolver: failed to derive michi_id from public_key! Rejecting endpoint."
                );
                return Ok(None);
            }
        };

        if derived_michi_id != expected_michi_id {
            warn!(
                expected = %expected_michi_id,
                derived = %derived_michi_id,
                url = %base_url,
                "MdnsResolver: derived michi_id does not match expected_michi_id! Rejecting endpoint."
            );
            return Ok(None);
        }

        let service = body
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if service != "michi-stream-standard" && service != "michi-stream-hifi" {
            debug!(
                service = %service,
                url = %base_url,
                "MdnsResolver: service is not an exact canonical match (michi-stream-standard or michi-stream-hifi); ignoring"
            );
            return Ok(None);
        }

        let api_version = body
            .get("api_version")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        if api_version != "v1-lite" {
            warn!(
                api_version = %api_version,
                url = %base_url,
                "MdnsResolver: server/info api_version is not v1-lite; rejecting endpoint."
            );
            return Ok(None);
        }

        let roles: Vec<String> = body
            .get("roles")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        if !roles.iter().any(|r| r == "audio_receiver") {
            debug!(
                roles = ?roles,
                url = %base_url,
                "MdnsResolver: receiver does not declare audio_receiver role; ignoring"
            );
            return Ok(None);
        }

        let server_id = match body
            .get("server_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            Some(s) => s.to_string(),
            None => {
                warn!(
                    url = %base_url,
                    "MdnsResolver: server/info missing non-empty server_id; rejecting endpoint."
                );
                return Ok(None);
            }
        };

        let name = body
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("Michi Stream")
            .to_string();

        Ok(Some(VerifiedServerInfo {
            michi_id: expected_michi_id.to_string(),
            device_id: server_id,
            name,
            service: service.to_string(),
            roles,
        }))
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
                Ok(Some(server_info)) => {
                    info!(
                        michi_id = %michi_id,
                        url = %candidate_url,
                        "MdnsResolver: verified identity endpoint; updating Scent presence"
                    );
                    let now = Instant::now();
                    self.scent.observe_mdns_candidate(
                        server_info,
                        candidate_url,
                        Some(socket_addr),
                        now,
                    );
                    break;
                }
                Ok(None) => {
                    // Identity mismatch or non-stream receiver, skip this endpoint
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
