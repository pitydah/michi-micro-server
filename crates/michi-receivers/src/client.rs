use crate::models::*;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
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
    signing_key: Option<SigningKey>,
    verifying_key: Option<VerifyingKey>,
    pub michi_id: Option<String>,
    pub public_key_b64: Option<String>,
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
            signing_key: None,
            verifying_key: None,
            michi_id: None,
            public_key_b64: None,
        }
    }

    /// Ensure Ed25519 cryptographic identity is initialized
    pub fn ensure_identity(&mut self) {
        if self.signing_key.is_none() {
            let mut csprng = OsRng;
            let sk = SigningKey::generate(&mut csprng);
            let vk = sk.verifying_key();
            let pk_bytes = vk.to_bytes();
            let pubkey_b64 = URL_SAFE_NO_PAD.encode(pk_bytes);
            let michi_id_b64 = URL_SAFE_NO_PAD.encode(blake3::hash(&pk_bytes).as_bytes());

            self.public_key_b64 = Some(pubkey_b64);
            self.michi_id = Some(michi_id_b64);
            self.verifying_key = Some(vk);
            self.signing_key = Some(sk);
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

    /// POST /api/v1/pair/start (canonical) with Ed25519 challenge signature
    pub async fn pair_start(&mut self, _initiator_id: &str) -> Result<PairStartResponse, String> {
        self.ensure_identity();
        let sk = self.signing_key.as_ref().unwrap();
        let michi_id = self.michi_id.as_ref().unwrap().clone();
        let public_key = self.public_key_b64.as_ref().unwrap().clone();

        let nonce_bytes: [u8; 32] = rand::random();
        let challenge_nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
        let signature = sk.sign(challenge_nonce.as_bytes());
        let challenge_signature = URL_SAFE_NO_PAD.encode(signature.to_bytes());

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
                    .json(&payload)
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

    /// POST /api/v1/pair/confirm (canonical) with 6-digit PIN verification
    pub async fn pair_confirm(
        &mut self,
        session_id_or_nonce: &str,
        _initiator_id: &str,
        pin: &str,
    ) -> Result<PairConfirmResponse, String> {
        self.ensure_identity();
        let michi_id = self.michi_id.as_ref().unwrap().clone();
        let public_key = self.public_key_b64.as_ref().unwrap().clone();

        let payload = serde_json::json!({
            "session_id": session_id_or_nonce,
            "nonce": session_id_or_nonce,
            "pin": pin,
            "michi_id": michi_id,
            "public_key": public_key,
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
                legacy_req = self.apply_session_headers(legacy_req);
                legacy_req.json(&payload).send().await
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
    ) -> Result<SessionStartResponse, String> {
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

        let resp = match req.json(&payload).send().await {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/session/start", self.base_url));
                if let Some(ref t) = self.token {
                    legacy_req = legacy_req.header("Authorization", format!("Bearer {t}"));
                }
                legacy_req.json(&payload).send().await
            }
            res => res,
        }
        .map_err(|e| format!("session_start request failed: {e}"))?;

        let status = resp.status();
        if status != reqwest::StatusCode::CREATED && status != reqwest::StatusCode::OK {
            return Err(format!("session_start failed with status {status}"));
        }
        let mut result: SessionStartResponse = resp
            .json()
            .await
            .map_err(|e| format!("session_start parse failed: {e}"))?;

        if let Some(ref sid) = result.session_id {
            self.active_session_id = Some(sid.clone());
        }
        if let Some(ref stok) = result.session_token {
            self.active_session_token = Some(stok.clone());
        }
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        // Derive stream_port from effective if present
        if result.stream_port.is_none() {
            if let Some(ref eff) = result.effective {
                if let Some(p) = eff.get("stream_port").and_then(|v| v.as_u64()) {
                    result.stream_port = Some(p as u16);
                }
            }
        }
        result.status = Some("session_started".to_string());
        Ok(result)
    }

    /// PATCH /api/v1/receiver-lite/session (canonical)
    pub async fn set_volume(&self, volume: u32) -> Result<VolumeResponse, String> {
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

        let resp = match req.json(&payload).send().await {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/volume", self.base_url));
                legacy_req = self.apply_session_headers(legacy_req);
                legacy_req.json(&payload).send().await
            }
            res => res,
        }
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
        let mut req = self
            .client
            .delete(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = match req.send().await {
            Ok(r) if r.status() == reqwest::StatusCode::NOT_FOUND => {
                let mut legacy_req = self
                    .client
                    .post(format!("{}/api/v1/receiver/session/stop", self.base_url));
                legacy_req = self.apply_session_headers(legacy_req);
                legacy_req.send().await
            }
            res => res,
        }
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
