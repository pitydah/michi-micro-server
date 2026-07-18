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
            .map_err(|e| format!("info request failed: {}", e))?;
        resp.json()
            .await
            .map_err(|e| format!("info parse failed: {}", e))
    }

    /// POST /api/v1/receiver/pair/start
    pub async fn pair_start(&self, initiator_id: &str) -> Result<PairStartResponse, String> {
        let resp = self
            .client
            .post(format!("{}/api/v1/receiver/pair/start", self.base_url))
            .json(&serde_json::json!({"initiator_id": initiator_id}))
            .send()
            .await
            .map_err(|e| format!("pair_start request failed: {}", e))?;
        resp.json()
            .await
            .map_err(|e| format!("pair_start parse failed: {}", e))
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
            .map_err(|e| format!("pair_confirm request failed: {}", e))?;
        let result: PairConfirmResponse = resp
            .json()
            .await
            .map_err(|e| format!("pair_confirm parse failed: {}", e))?;
        if let Some(ref t) = result.token {
            self.token = Some(t.clone());
        }
        Ok(result)
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {}", t))
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
            .map_err(|e| format!("heartbeat request failed: {}", e))?;
        resp.json()
            .await
            .map_err(|e| format!("heartbeat parse failed: {}", e))
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
            .map_err(|e| format!("session_start request failed: {}", e))?;
        resp.json()
            .await
            .map_err(|e| format!("session_start parse failed: {}", e))
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
            .map_err(|e| format!("session_stop request failed: {}", e))?;
        resp.json()
            .await
            .map_err(|e| format!("session_stop parse failed: {}", e))
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
            .map_err(|e| format!("volume request failed: {}", e))?;
        resp.json()
            .await
            .map_err(|e| format!("volume parse failed: {}", e))
    }
}
