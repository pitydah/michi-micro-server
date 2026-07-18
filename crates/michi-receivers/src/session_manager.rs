use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::client::ReceiverClient;
use crate::models::*;

/// Manages receiver sessions: pairing, heartbeat, session start/stop, volume.
#[derive(Debug, Clone)]
pub struct ReceiverSessionManager {
    registry: Arc<RwLock<ReceiverRegistry>>,
}

impl ReceiverSessionManager {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
        }
    }

    pub fn new_with(registry: Arc<RwLock<ReceiverRegistry>>) -> Self {
        Self { registry }
    }

    pub async fn registry(&self) -> Arc<RwLock<ReceiverRegistry>> {
        self.registry.clone()
    }

    pub async fn discover_and_pair(
        &self,
        base_url: &str,
        initiator_id: &str,
    ) -> Result<String, String> {
        let client = ReceiverClient::new(base_url);
        let info = client.get_info().await?;
        let device_id = info
            .device_id
            .clone()
            .unwrap_or_else(|| base_url.to_string());
        let name = info.name.clone().unwrap_or_else(|| device_id.clone());
        let device_type = info.device_type.clone().unwrap_or_else(|| "unknown".into());

        // Attempt to get a pairing window. If it fails because one is already open,
        // we can still try to use it (the test simulators keep the same nonce).
        let start_resp = client.pair_start(initiator_id).await?;
        let nonce = if let Some(ref n) = start_resp.nonce {
            n.clone()
        } else if let Some(ref err) = start_resp.error {
            // If window is already open, we can't get the nonce — fail gracefully
            return Err(format!("pair_start failed: {}: {}", err.code, err.message));
        } else {
            return Err("no nonce in pair_start response".to_string());
        };
        let token = Uuid::new_v4().to_string();

        let mut client = client;
        let _confirm_resp = client.pair_confirm(&nonce, initiator_id, &token).await?;

        // Extract capabilities
        let output = info.output.as_ref();
        let max_sr = output
            .and_then(|o| o.get("max_sample_rate").and_then(|v| v.as_u64()))
            .unwrap_or(48000) as u32;
        let max_bd = output
            .and_then(|o| o.get("max_bit_depth").and_then(|v| v.as_u64()))
            .unwrap_or(16) as u32;
        let codecs = info.supported_codecs.clone().unwrap_or_default();
        let mut caps = vec![
            "stream".to_string(),
            "volume".to_string(),
            "heartbeat".to_string(),
        ];
        if let Some(feats) = &info.features {
            if feats
                .get("ota_update")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                caps.push("ota_update".to_string());
            }
        }

        let entry = ReceiverRegistryEntry {
            receiver_id: device_id.clone(),
            name,
            device_type,
            base_url: base_url.to_string(),
            paired: true,
            token: client.token.clone(),
            last_seen: Some(chrono::Utc::now()),
            capabilities: caps,
            active_session_id: None,
            max_sample_rate: max_sr,
            max_bit_depth: max_bd,
            supported_codecs: codecs,
            maximum_safe_volume: Some(100),
        };

        self.registry.write().await.add(entry);
        Ok(device_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_session(
        &self,
        receiver_id: &str,
        session_id: &str,
        codec: &str,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
        stream_port: u16,
        buffer_ms: u64,
        volume: u32,
    ) -> Result<SessionStartResponse, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {}", receiver_id))?;

        let base_url = entry.base_url.clone();
        let token = entry.token.clone();
        let mut client = ReceiverClient::new(&base_url);
        client.token = token;

        let resp = client
            .session_start(
                session_id,
                codec,
                sample_rate,
                bit_depth,
                channels,
                stream_port,
                buffer_ms,
                volume,
            )
            .await?;

        {
            let mut reg = self.registry.write().await;
            if let Some(e) = reg.get_mut(receiver_id) {
                e.active_session_id = Some(session_id.to_string());
                e.last_seen = Some(chrono::Utc::now());
            }
        }

        Ok(resp)
    }

    pub async fn stop_session(&self, receiver_id: &str) -> Result<SessionStopResponse, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {}", receiver_id))?;

        let mut client = ReceiverClient::new(&entry.base_url);
        client.token = entry.token.clone();
        let resp = client.session_stop().await?;

        {
            let mut reg = self.registry.write().await;
            if let Some(e) = reg.get_mut(receiver_id) {
                e.active_session_id = None;
                e.last_seen = Some(chrono::Utc::now());
            }
        }

        Ok(resp)
    }

    pub async fn set_volume(
        &self,
        receiver_id: &str,
        volume: u32,
    ) -> Result<VolumeResponse, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {}", receiver_id))?;

        let mut client = ReceiverClient::new(&entry.base_url);
        client.token = entry.token.clone();
        client.set_volume(volume).await
    }

    pub async fn heartbeat(&self, receiver_id: &str) -> Result<HeartbeatResponse, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {}", receiver_id))?;

        let mut client = ReceiverClient::new(&entry.base_url);
        client.token = entry.token.clone();
        let resp = client.heartbeat().await?;

        {
            let mut reg = self.registry.write().await;
            if let Some(e) = reg.get_mut(receiver_id) {
                e.last_seen = Some(chrono::Utc::now());
            }
        }

        Ok(resp)
    }

    pub async fn get_info(&self, receiver_id: &str) -> Result<ReceiverInfo, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {}", receiver_id))?;

        let client = ReceiverClient::new(&entry.base_url);
        client.get_info().await
    }
}

impl Default for ReceiverSessionManager {
    fn default() -> Self {
        Self::new()
    }
}
