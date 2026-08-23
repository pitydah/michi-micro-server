use crate::models::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// HTTP client for interacting with a Michi Music Stream receiver (canonical Michi Link v1-lite).
pub struct ReceiverClient {
    pub base_url: String,
    client: reqwest::Client,
    pub token: Option<String>,
    pub active_session_id: Option<String>,
    pub active_session_token: Option<String>,
    pub heartbeat_sequence: Arc<AtomicU64>,
    pub identity: Option<Arc<michi_identity::IdentityManager>>,
}

impl ReceiverClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token: None,
            active_session_id: None,
            active_session_token: None,
            heartbeat_sequence: Arc::new(AtomicU64::new(0)),
            identity: None,
        }
    }

    pub fn with_identity(base_url: &str, identity: Arc<michi_identity::IdentityManager>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token: None,
            active_session_id: None,
            active_session_token: None,
            heartbeat_sequence: Arc::new(AtomicU64::new(0)),
            identity: Some(identity),
        }
    }

    pub fn set_identity(&mut self, identity: Arc<michi_identity::IdentityManager>) {
        self.identity = Some(identity);
    }

    /// GET /api/v1/server/info (canonical v1-lite)
    pub async fn get_info(&self) -> Result<ReceiverInfo, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/server/info", self.base_url))
            .send()
            .await
            .map_err(|e| format!("info request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("info request failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("info parse failed: {e}"))
    }

    /// POST /api/v1/pair/start (canonical) with Ed25519 challenge signature over RAW nonce bytes
    pub async fn pair_start(&mut self, _initiator_id: &str) -> Result<PairStartResponse, String> {
        let (michi_id, public_key, challenge_nonce, challenge_signature) =
            if let Some(ref id) = self.identity {
                let nonce_raw: [u8; 32] = rand::random();
                let challenge_nonce = URL_SAFE_NO_PAD.encode(nonce_raw);
                let (sig_b64, pk_b64) = id.sign_base64url(&nonce_raw);
                (
                    id.michi_id().to_base64url(),
                    pk_b64,
                    challenge_nonce,
                    sig_b64,
                )
            } else {
                return Err("IdentityManager not configured on ReceiverClient".to_string());
            };

        let payload = serde_json::json!({
            "device_name": "Michi Micro Server",
            "device_type": "server",
            "roles": ["music_server", "playback_host"],
            "auth_strategy": "RECEIVER_BUTTON",
            "michi_id": michi_id,
            "public_key": public_key,
            "challenge_nonce": challenge_nonce,
            "challenge_signature": challenge_signature,
        });

        let resp = self
            .client
            .post(format!("{}/api/v1/pair/start", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("pair_start request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("pair_start failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("pair_start parse failed: {e}"))
    }

    /// POST /api/v1/pair/confirm (canonical) with 6-digit PIN verification
    pub async fn pair_confirm(
        &mut self,
        session_id: &str,
        _initiator_id: &str,
        pin: &str,
    ) -> Result<PairConfirmResponse, ReceiverProtocolError> {
        let (michi_id, public_key) = if let Some(ref id) = self.identity {
            (id.michi_id().to_base64url(), id.public_key_base64url())
        } else {
            return Err(ReceiverProtocolError {
                http_status: 500,
                code: "INTERNAL_ERROR".into(),
                message: "IdentityManager not configured on ReceiverClient".into(),
                details: serde_json::Value::Null,
            });
        };

        let payload = serde_json::json!({
            "session_id": session_id,
            "pin": pin,
            "michi_id": michi_id,
            "public_key": public_key,
        });

        let resp = self
            .client
            .post(format!("{}/api/v1/pair/confirm", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("pair_confirm request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PAIRING_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pair confirm rejected")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "PAIRING_FAILED".into(),
                message: format!("pair_confirm failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }
        let result: PairConfirmResponse = resp
            .json()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 500,
                code: "DECODE_ERROR".into(),
                message: format!("pair_confirm parse failed: {e}"),
                details: serde_json::Value::Null,
            })?;
        if let Some(ref t) = result.token {
            self.token = Some(t.clone());
        }
        Ok(result)
    }

    fn apply_session_headers(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref stok) = self.active_session_token {
            req = req.header("X-Michi-Session", stok);
        }
        if let Some(ref t) = self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        req
    }

    /// POST /api/v1/receiver-lite/heartbeat (canonical)
    pub async fn heartbeat(&self) -> Result<HeartbeatResponse, String> {
        let session_id = self.active_session_id.as_ref().ok_or_else(|| {
            "NoActiveSession: cannot heartbeat without active session".to_string()
        })?;

        let seq = self.heartbeat_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let sent_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let payload = serde_json::json!({
            "session_id": session_id,
            "sequence": seq,
            "sent_at_ms": sent_at_ms,
        });

        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver-lite/heartbeat", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("heartbeat request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("heartbeat failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("heartbeat parse failed: {e}"))
    }

    /// POST /api/v1/receiver-lite/session (canonical HTTP 201)
    #[allow(clippy::too_many_arguments)]
    pub async fn session_start(
        &mut self,
        _session_id_hint: &str,
        codec: &str,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
        _stream_port_hint: u16,
        buffer_ms: u64,
        volume: u32,
    ) -> Result<NegotiatedReceiverSession, String> {
        if volume > 100 {
            return Err(format!("volume {volume} exceeds maximum of 100"));
        }
        let ssrc: u32 = rand::random::<u32>().max(1);

        let payload = serde_json::json!({
            "transport": "rtp_udp",
            "codec": codec,
            "sample_rate": sample_rate,
            "bit_depth": bit_depth,
            "channels": channels,
            "packet_ms": 10,
            "buffer_ms": buffer_ms,
            "payload_type": 97,
            "ssrc": ssrc,
            "volume": volume,
        });

        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver-lite/session", self.base_url));
        if let Some(ref t) = self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("session_start request failed: {e}"))?;

        let status = resp.status();
        if status != reqwest::StatusCode::CREATED {
            return Err(format!(
                "session_start failed: expected HTTP 201 Created, got {status}"
            ));
        }
        let raw_resp: SessionStartResponse = resp
            .json()
            .await
            .map_err(|e| format!("session_start parse failed: {e}"))?;

        let negotiated = raw_resp.validate_strict()?;

        self.active_session_id = Some(negotiated.session_id.clone());
        self.active_session_token = Some(negotiated.session_token.clone());
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        Ok(negotiated)
    }

    /// PATCH /api/v1/receiver-lite/session (canonical)
    pub async fn set_volume(&self, volume: u32) -> Result<VolumeResponse, String> {
        if self.active_session_id.is_none() {
            return Err("NoActiveSession: cannot set volume without active session".to_string());
        }
        if volume > 100 {
            return Err(format!("volume {volume} exceeds maximum of 100"));
        }
        let payload = serde_json::json!({
            "volume": volume,
        });

        let mut req = self
            .client
            .patch(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("set_volume request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("set_volume failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("set_volume parse failed: {e}"))
    }

    /// DELETE /api/v1/receiver-lite/session (canonical HTTP 204 or 200)
    pub async fn session_stop(&mut self) -> Result<SessionStopResponse, String> {
        if self.active_session_id.is_none() {
            return Err(
                "NoActiveSession: cannot stop session when no session is active".to_string(),
            );
        }
        let mut req = self
            .client
            .delete(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req
            .send()
            .await
            .map_err(|e| format!("session_stop request failed: {e}"))?;

        let status = resp.status();
        if status != reqwest::StatusCode::NO_CONTENT
            && status != reqwest::StatusCode::OK
            && !status.is_success()
        {
            return Err(format!("session_stop failed with status {status}"));
        }

        self.active_session_id = None;
        self.active_session_token = None;
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        Ok(SessionStopResponse {
            status: Some("session_stopped".to_string()),
            session_id: None,
            error: None,
        })
    }

    /// GET /api/v1/receiver-lite/session (canonical)
    pub async fn get_playback_state(&self) -> Result<ReceiverPlaybackState, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/receiver-lite/session", self.base_url))
            .send()
            .await
            .map_err(|e| format!("get_playback_state failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("get_playback_state failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("get_playback_state parse failed: {e}"))
    }

    // Fault injection helpers
    pub async fn fault_latency(&self, latency_ms: u64) -> Result<(), String> {
        let payload = serde_json::json!({ "latency_ms": latency_ms });
        self.client
            .post(format!("{}/api/v1/receiver/fault/latency", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn fault_offline(&self, offline: bool) -> Result<(), String> {
        let payload = serde_json::json!({ "offline": offline });
        self.client
            .post(format!("{}/api/v1/receiver/fault/offline", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn fault_network_drop(&self, drop_count: u32) -> Result<(), String> {
        let payload = serde_json::json!({ "drop_count": drop_count });
        self.client
            .post(format!(
                "{}/api/v1/receiver/fault/network_drop",
                self.base_url
            ))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn fault_reset(&self) -> Result<(), String> {
        self.client
            .post(format!("{}/api/v1/receiver/fault/reset", self.base_url))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
