use crate::models::*;

/// HTTP client for interacting with a Michi Music Stream receiver (simulator-compatible).
pub struct ReceiverClient {
    pub base_url: String,
    client: reqwest::Client,
    pub token: Option<String>,
}

impl ReceiverClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token: None,
        }
    }

    /// GET /api/v1/receiver/info
    pub async fn get_info(&self) -> Result<ReceiverInfo, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/receiver/info", self.base_url))
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

    /// POST /api/v1/receiver/pair/start
    pub async fn pair_start(&self, initiator_id: &str) -> Result<PairStartResponse, String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/receiver/pair/start", self.base_url))
            .json(&serde_json::json!({"initiator_id": initiator_id}))
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

    /// POST /api/v1/receiver/pair/confirm
    pub async fn pair_confirm(
        &mut self,
        nonce: &str,
        initiator_id: &str,
        token: &str,
    ) -> Result<PairConfirmResponse, String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/receiver/pair/confirm", self.base_url))
            .json(&serde_json::json!({
                "nonce": nonce,
                "initiator_id": initiator_id,
                "token": token,
            }))
            .send()
            .await
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

    /// POST /api/v1/receiver/heartbeat
    pub async fn heartbeat(&self) -> Result<HeartbeatResponse, String> {
        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver/heartbeat", self.base_url));
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("heartbeat request failed: {e}"))?;
        resp.json()
            .await
            .map_err(|e| format!("heartbeat parse failed: {e}"))
    }

    /// POST /api/v1/receiver/session/start
    #[allow(clippy::too_many_arguments)]
    pub async fn session_start(
        &self,
        session_id: &str,
        codec: &str,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
        stream_port: u16,
        buffer_ms: u64,
        volume: u32,
    ) -> Result<SessionStartResponse, String> {
        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver/session/start", self.base_url));
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
        let resp = req
            .json(&serde_json::json!({
                "session_id": session_id,
                "codec": codec,
                "sample_rate": sample_rate,
                "bit_depth": bit_depth,
                "channels": channels,
                "stream_port": stream_port,
                "buffer_ms": buffer_ms,
                "volume": volume,
            }))
            .send()
            .await
            .map_err(|e| format!("session_start request failed: {e}"))?;
        resp.json()
            .await
            .map_err(|e| format!("session_start parse failed: {e}"))
    }

    /// POST /api/v1/receiver/session/stop
    pub async fn session_stop(&self) -> Result<SessionStopResponse, String> {
        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver/session/stop", self.base_url));
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("session_stop request failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("session_stop failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("session_stop parse failed: {e}"))
    }

    /// POST /api/v1/receiver/volume
    pub async fn set_volume(&self, volume: u32) -> Result<VolumeResponse, String> {
        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver/volume", self.base_url));
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
        let resp = req
            .json(&serde_json::json!({"volume": volume}))
            .send()
            .await
            .map_err(|e| format!("volume request failed: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("volume request failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("volume parse failed: {e}"))
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
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
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
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
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
        &self,
        session_id: &str,
        position_ms: u64,
        volume: u32,
        playing: bool,
    ) -> Result<SessionRecoverResponse, String> {
        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver/session/recover", self.base_url));
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", &h);
        }
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
        resp.json()
            .await
            .map_err(|e| format!("session_recover parse failed: {e}"))
    }

    /// POST /api/v1/receiver/disconnect
    pub async fn disconnect(&self) -> Result<(), String> {
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
