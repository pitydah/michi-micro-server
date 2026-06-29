use uuid::Uuid;

use crate::models::*;

/// HTTP client for interacting with a Michi Music Stream receiver.
pub struct ReceiverClient {
    base_url: String,
    client: reqwest::Client,
    device_token: Option<String>,
}

impl ReceiverClient {
    pub fn new(host: &str, port: u16) -> Self {
        Self {
            base_url: format!("http://{}:{}", host, port),
            client: reqwest::Client::new(),
            device_token: None,
        }
    }

    pub fn with_token(host: &str, port: u16, token: &str) -> Self {
        Self {
            base_url: format!("http://{}:{}", host, port),
            client: reqwest::Client::new(),
            device_token: Some(token.to_string()),
        }
    }

    pub async fn get_info(&self) -> Result<ReceiverInfo, String> {
        let resp = self.client.get(format!("{}/api/v1/receiver/info", self.base_url))
            .send().await.map_err(|e| format!("request failed: {}", e))?;
        resp.json().await.map_err(|e| format!("parse failed: {}", e))
    }

    pub async fn pair_start(&self) -> Result<PairStartResponse, String> {
        let resp = self.client.post(format!("{}/api/v1/pair/start", self.base_url))
            .json(&serde_json::json!({"device_type": "michi-stream"}))
            .send().await.map_err(|e| format!("request failed: {}", e))?;
        resp.json().await.map_err(|e| format!("parse failed: {}", e))
    }

    pub async fn pair_confirm(&mut self, code: &str) -> Result<PairConfirmResponse, String> {
        let resp = self.client.post(format!("{}/api/v1/pair/confirm", self.base_url))
            .json(&PairConfirmRequest { code: code.to_string() })
            .send().await.map_err(|e| format!("request failed: {}", e))?;
        let result: PairConfirmResponse = resp.json().await.map_err(|e| format!("parse failed: {}", e))?;
        self.device_token = Some(result.device_token.clone());
        Ok(result)
    }

    pub async fn heartbeat(&self) -> Result<ApiResponse, String> {
        let mut req = self.client.post(format!("{}/api/v1/receiver/heartbeat", self.base_url));
        if let Some(ref token) = self.device_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let resp = req.json(&HeartbeatRequest {
            device_id: Uuid::nil(),
            status: "online".into(),
            uptime_seconds: 0,
        }).send().await.map_err(|e| format!("heartbeat failed: {}", e))?;
        resp.json().await.map_err(|e| format!("parse failed: {}", e))
    }

    pub async fn session_start(&self, track_id: Uuid, stream_url: &str, position_ms: u64, volume: u32) -> Result<ApiResponse, String> {
        let mut req = self.client.post(format!("{}/api/v1/receiver/session/start", self.base_url));
        if let Some(ref token) = self.device_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let resp = req.json(&SessionStartRequest {
            track_id, stream_url: stream_url.to_string(), position_ms, volume,
        }).send().await.map_err(|e| format!("session start failed: {}", e))?;
        resp.json().await.map_err(|e| format!("parse failed: {}", e))
    }

    pub async fn session_stop(&self, reason: &str) -> Result<ApiResponse, String> {
        let mut req = self.client.post(format!("{}/api/v1/receiver/session/stop", self.base_url));
        if let Some(ref token) = self.device_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let resp = req.json(&SessionStopRequest { reason: reason.to_string() })
            .send().await.map_err(|e| format!("session stop failed: {}", e))?;
        resp.json().await.map_err(|e| format!("parse failed: {}", e))
    }

    pub async fn set_volume(&self, volume: u32) -> Result<ApiResponse, String> {
        let mut req = self.client.post(format!("{}/api/v1/receiver/volume", self.base_url));
        if let Some(ref token) = self.device_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }
        let resp = req.json(&VolumeRequest { volume })
            .send().await.map_err(|e| format!("volume failed: {}", e))?;
        resp.json().await.map_err(|e| format!("parse failed: {}", e))
    }
}
