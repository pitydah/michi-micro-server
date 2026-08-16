use crate::models::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use uuid::Uuid;

/// HTTP client for interacting with a Michi Music Stream receiver (canonical Michi Link v1-lite).
pub struct ReceiverClient {
    pub base_url: String,
    client: reqwest::Client,
    pub token: Option<String>,
    pub active_session_id: Option<String>,
    pub active_session_token: Option<String>,
    pub heartbeat_sequence: Arc<AtomicU64>,
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
        }
    }

    /// GET /api/v1/server/info (canonical) with fallback to /api/v1/receiver/info
    pub async fn get_info(&self) -> Result<ReceiverInfo, String> {
        let resp = match self
            .client
            .get(format!("{}/api/v1/server/info", self.base_url))
            .send()
            .await
        {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                self.client
                    .get(format!("{}/api/v1/receiver/info", self.base_url))
                    .send()
                    .await
            }
            res => res,
        }
        .map_err(|e| format!("info request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("info request failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("info parse failed: {e}"))
    }

    /// POST /api/v1/pair/start (canonical) with fallback to /api/v1/receiver/pair/start
    pub async fn pair_start(&self, initiator_id: &str) -> Result<PairStartResponse, String> {
        let challenge_nonce = Uuid::new_v4().to_string();
        let payload = serde_json::json!({
            "initiator_id": initiator_id,
            "device_name": "Michi Micro Server",
            "device_type": "server",
            "roles": ["music_server"],
            "auth_strategy": "RECEIVER_BUTTON",
            "challenge_nonce": challenge_nonce,
        });

        let resp = match self
            .client
            .post(format!("{}/api/v1/pair/start", self.base_url))
            .json(&payload)
            .send()
            .await
        {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                self.client
                    .post(format!("{}/api/v1/receiver/pair/start", self.base_url))
                    .json(&serde_json::json!({"initiator_id": initiator_id}))
                    .send()
                    .await
            }
            res => res,
        }
        .map_err(|e| format!("pair_start request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("pair_start failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("pair_start parse failed: {e}"))
    }

    /// POST /api/v1/pair/confirm (canonical) with fallback to /api/v1/receiver/pair/confirm
    pub async fn pair_confirm(
        &mut self,
        nonce_or_session_id: &str,
        initiator_id: &str,
        pin_or_token: &str,
    ) -> Result<PairConfirmResponse, String> {
        let payload = serde_json::json!({
            "session_id": nonce_or_session_id,
            "nonce": nonce_or_session_id,
            "pin": pin_or_token,
            "initiator_id": initiator_id,
            "token": pin_or_token,
        });

        let resp = match self
            .client
            .post(format!("{}/api/v1/pair/confirm", self.base_url))
            .json(&payload)
            .send()
            .await
        {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                self.client
                    .post(format!("{}/api/v1/receiver/pair/confirm", self.base_url))
                    .json(&payload)
                    .send()
                    .await
            }
            res => res,
        }
        .map_err(|e| format!("pair_confirm request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("pair_confirm failed with status {status}"));
        }
        let result: PairConfirmResponse = resp
            .json()
            .await
            .map_err(|e| format!("pair_confirm parse failed: {e}"))?;
        if let Some(ref t) = result.token {
            self.token = Some(t.clone());
        }
        Ok(result)
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
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

    /// POST /api/v1/receiver-lite/heartbeat (canonical) with fallback to /api/v1/receiver/heartbeat
    pub async fn heartbeat(&self) -> Result<HeartbeatResponse, String> {
        let seq = self.heartbeat_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let sent_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let session_id = self
            .active_session_id
            .clone()
            .unwrap_or_else(|| Uuid::nil().to_string());

        let payload = serde_json::json!({
            "session_id": session_id,
            "sequence": seq,
            "sent_at_ms": sent_at_ms,
        });

        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver-lite/heartbeat", self.base_url));
        req = self.apply_session_headers(req);

        let resp = match req.json(&payload).send().await {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/heartbeat", self.base_url));
                if let Some(h) = self.auth_header() {
                    legacy_req = legacy_req.header("Authorization", &h);
                }
                legacy_req.send().await
            }
            res => res,
        }
        .map_err(|e| format!("heartbeat request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("heartbeat failed with status {status}"));
        }

        resp.json()
            .await
            .map_err(|e| format!("heartbeat parse failed: {e}"))
    }

    /// POST /api/v1/receiver-lite/session (canonical) with fallback to /api/v1/receiver/session/start
    #[allow(clippy::too_many_arguments)]
    pub async fn session_start(
        &mut self,
        session_id: &str,
        codec: &str,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
        stream_port: u16,
        buffer_ms: u64,
        volume: u32,
    ) -> Result<SessionStartResponse, String> {
        let ssrc: u32 = {
            let raw = (Uuid::new_v4().as_u128() & 0xFFFFFFFF) as u32;
            if raw == 0 {
                305419896
            } else {
                raw
            }
        };

        let canonical_body = serde_json::json!({
            "session_id": session_id,
            "transport": "rtp_udp",
            "codec": codec,
            "sample_rate": sample_rate,
            "bit_depth": bit_depth,
            "channels": channels,
            "packet_ms": 10,
            "buffer_ms": buffer_ms,
            "payload_type": 97,
            "ssrc": ssrc,
            "stream_port": stream_port,
            "volume": volume,
        });

        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver-lite/session", self.base_url));
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }

        let resp = match req.json(&canonical_body).send().await {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/session/start", self.base_url));
                if let Some(h) = self.auth_header() {
                    legacy_req = legacy_req.header("Authorization", &h);
                }
                legacy_req.json(&canonical_body).send().await
            }
            res => res,
        }
        .map_err(|e| format!("session_start request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("session_start failed with status {status}"));
        }

        let mut start_resp: SessionStartResponse = resp
            .json()
            .await
            .map_err(|e| format!("session_start parse failed: {e}"))?;

        if start_resp.stream_port.is_none() {
            if let Some(ref eff) = start_resp.effective {
                if let Some(port) = eff.get("stream_port").and_then(|v| v.as_u64()) {
                    start_resp.stream_port = Some(port as u16);
                }
            }
        }
        if start_resp.status.is_none() && start_resp.session_id.is_some() {
            start_resp.status = Some("session_started".to_string());
        }

        // Store active session credentials and reset heartbeat sequence
        if let Some(ref sid) = start_resp.session_id {
            self.active_session_id = Some(sid.clone());
        } else {
            self.active_session_id = Some(session_id.to_string());
        }
        if let Some(ref stok) = start_resp.session_token {
            self.active_session_token = Some(stok.clone());
        }
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        Ok(start_resp)
    }

    /// DELETE /api/v1/receiver-lite/session (canonical) with fallback to /api/v1/receiver/session/stop
    pub async fn session_stop(&mut self) -> Result<SessionStopResponse, String> {
        let mut req = self
            .client
            .delete(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = match req.send().await {
            Ok(r)
                if r.status() == reqwest::StatusCode::NOT_FOUND
                    || r.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED =>
            {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/session/stop", self.base_url));
                if let Some(h) = self.auth_header() {
                    legacy_req = legacy_req.header("Authorization", &h);
                }
                legacy_req.send().await
            }
            res => res,
        }
        .map_err(|e| format!("session_stop request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("session_stop failed with status {status}"));
        }

        self.active_session_id = None;
        self.active_session_token = None;
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        resp.json().await.or_else(|_| {
            Ok(SessionStopResponse {
                status: Some("session_stopped".to_string()),
                session_id: None,
                error: None,
            })
        })
    }

    /// PATCH /api/v1/receiver-lite/session (canonical) with fallback to /api/v1/receiver/volume
    pub async fn set_volume(&self, volume: u32) -> Result<VolumeResponse, String> {
        if volume > 100 {
            return Err(format!("volume {volume} exceeds maximum allowed (100)"));
        }

        let mut req = self
            .client
            .patch(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = match req
            .json(&serde_json::json!({"volume": volume}))
            .send()
            .await
        {
            Ok(r)
                if r.status() == reqwest::StatusCode::NOT_FOUND
                    || r.status() == reqwest::StatusCode::METHOD_NOT_ALLOWED =>
            {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/volume", self.base_url));
                if let Some(h) = self.auth_header() {
                    legacy_req = legacy_req.header("Authorization", &h);
                }
                legacy_req
                    .json(&serde_json::json!({"volume": volume}))
                    .send()
                    .await
            }
            res => res,
        }
        .map_err(|e| format!("volume request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("volume request failed with status {status}"));
        }
        resp.json().await.or_else(|_| {
            Ok(VolumeResponse {
                status: Some("ok".to_string()),
                volume: Some(volume),
                error: None,
            })
        })
    }

    /// POST /api/v1/receiver/playback/control
    pub async fn playback_control(
        &self,
        command: &str,
        position_ms: Option<u64>,
    ) -> Result<PlaybackControlResponse, String> {
        let mut req = self.client.post(format!(
            "{}/api/v1/receiver/playback/control",
            self.base_url
        ));
        req = self.apply_session_headers(req);
        let mut payload = serde_json::json!({"command": command});
        if let Some(pos) = position_ms {
            payload["position_ms"] = serde_json::json!(pos);
        }
        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("playback_control request failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("playback_control failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("playback_control parse failed: {e}"))
    }

    /// GET /api/v1/receiver/playback/state
    pub async fn get_playback_state(&self) -> Result<ReceiverPlaybackState, String> {
        let mut req = self
            .client
            .get(format!("{}/api/v1/receiver/playback/state", self.base_url));
        req = self.apply_session_headers(req);
        let resp = req
            .send()
            .await
            .map_err(|e| format!("get_playback_state request failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("get_playback_state failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("get_playback_state parse failed: {e}"))
    }

    /// POST /api/v1/receiver/session/recover
    pub async fn session_recover(
        &mut self,
        session_id: &str,
        position_ms: u64,
        volume: u32,
        playing: bool,
    ) -> Result<SessionRecoverResponse, String> {
        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver/session/recover", self.base_url));
        req = self.apply_session_headers(req);
        let resp = req
            .json(&serde_json::json!({
                "session_id": session_id,
                "position_ms": position_ms,
                "volume": volume,
                "playing": playing,
            }))
            .send()
            .await
            .map_err(|e| format!("session_recover request failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("session_recover failed with status {status}"));
        }
        self.active_session_id = Some(session_id.to_string());
        self.heartbeat_sequence.store(0, Ordering::SeqCst);
        resp.json()
            .await
            .map_err(|e| format!("session_recover parse failed: {e}"))
    }

    /// POST /api/v1/receiver/disconnect
    pub async fn disconnect(&mut self) -> Result<(), String> {
        let req = self
            .client
            .post(format!("{}/api/v1/receiver/disconnect", self.base_url));
        let resp = req
            .send()
            .await
            .map_err(|e| format!("disconnect failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("disconnect failed with status {status}"));
        }
        self.active_session_id = None;
        self.active_session_token = None;
        self.heartbeat_sequence.store(0, Ordering::SeqCst);
        Ok(())
    }

    // --- Fault Injection Admin Methods ---

    pub async fn fault_latency(&self, latency_ms: u64) -> Result<(), String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/receiver/fault/latency", self.base_url))
            .json(&serde_json::json!({"latency_ms": latency_ms}))
            .send()
            .await
            .map_err(|e| format!("fault_latency failed: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            Err(format!("fault_latency returned {status}"))
        }
    }

    pub async fn fault_offline(&self, offline: bool) -> Result<(), String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/receiver/fault/offline", self.base_url))
            .json(&serde_json::json!({"offline": offline}))
            .send()
            .await
            .map_err(|e| format!("fault_offline failed: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            Err(format!("fault_offline returned {status}"))
        }
    }

    pub async fn fault_network_drop(&self, drop_count: u32) -> Result<(), String> {
        let resp = self
            .client
            .post(format!(
                "{}/api/v1/receiver/fault/network_drop",
                self.base_url
            ))
            .json(&serde_json::json!({"drop_count": drop_count}))
            .send()
            .await
            .map_err(|e| format!("fault_network_drop failed: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            Err(format!("fault_network_drop returned {status}"))
        }
    }

    pub async fn fault_reset(&self) -> Result<(), String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/receiver/fault/reset", self.base_url))
            .send()
            .await
            .map_err(|e| format!("fault_reset failed: {e}"))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            Err(format!("fault_reset returned {status}"))
        }
    }
}
