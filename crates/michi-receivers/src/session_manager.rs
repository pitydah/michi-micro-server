use std::sync::Arc;
use tokio::sync::RwLock;

use crate::client::ReceiverClient;
use crate::models::*;

/// Manages receiver sessions: pairing, heartbeat, session start/stop, volume.
#[derive(Debug, Clone)]
pub struct ReceiverSessionManager {
    registry: Arc<RwLock<ReceiverRegistry>>,
    identity: Option<Arc<michi_identity::IdentityManager>>,
}

impl ReceiverSessionManager {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: None,
        }
    }

    pub fn new_with_identity(identity: Arc<michi_identity::IdentityManager>) -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: Some(identity),
        }
    }

    pub fn new_with(registry: Arc<RwLock<ReceiverRegistry>>) -> Self {
        Self {
            registry,
            identity: None,
        }
    }

    pub fn set_identity(&mut self, identity: Arc<michi_identity::IdentityManager>) {
        self.identity = Some(identity);
    }

    pub async fn registry(&self) -> Arc<RwLock<ReceiverRegistry>> {
        self.registry.clone()
    }

    /// Step 1 of receiver pairing: Initiate pairing and return the pending session_id.
    pub async fn start_pairing(
        &self,
        base_url: &str,
        initiator_id: &str,
    ) -> Result<(String, ReceiverInfo, ReceiverClient), String> {
        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(base_url, id.clone())
        } else {
            ReceiverClient::new(base_url)
        };
        let info = client.get_info().await?;
        let start_resp = client.pair_start(initiator_id).await?;
        let session_id = if let Some(ref s_id) = start_resp.session_id {
            s_id.clone()
        } else if let Some(ref n) = start_resp.nonce {
            n.clone()
        } else if let Some(ref err) = start_resp.error {
            return Err(format!("pair_start failed: {}: {}", err.code, err.message));
        } else {
            return Err("no session_id in pair_start response".to_string());
        };
        Ok((session_id, info, client))
    }

    /// Step 2 of receiver pairing: Confirm pairing using the PIN entered by user.
    pub async fn discover_and_pair(
        &self,
        base_url: &str,
        initiator_id: &str,
        pin: &str,
    ) -> Result<String, String> {
        let (session_id, info, client) = self.start_pairing(base_url, initiator_id).await?;

        let device_id = info
            .michi_id
            .clone()
            .or_else(|| info.server_id.clone())
            .or_else(|| info.device_id.clone())
            .unwrap_or_else(|| client.base_url.clone());
        let name = info.name.clone().unwrap_or_else(|| device_id.clone());
        let device_type = info
            .device_type
            .clone()
            .or_else(|| info.service.clone())
            .unwrap_or_else(|| "unknown".into());

        let mut client = client;
        let _confirm_resp = client.pair_confirm(&session_id, initiator_id, pin).await?;

        // Extract capabilities strictly without assuming fake default codecs/rates
        let (max_sr, max_bd, codecs) = if let Some(audio) = &info.audio {
            let max_sr = audio
                .get("sample_rates")
                .and_then(|v| v.as_array())
                .and_then(|a| a.iter().filter_map(|x| x.as_u64()).max())
                .ok_or_else(|| "receiver capabilities missing valid sample_rates".to_string())? as u32;
            let max_bd = audio
                .get("bit_depths")
                .and_then(|v| v.as_array())
                .and_then(|a| a.iter().filter_map(|x| x.as_u64()).max())
                .ok_or_else(|| "receiver capabilities missing valid bit_depths".to_string())? as u32;
            let codecs: Vec<String> = audio
                .get("codecs")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .filter(|c: &Vec<String>| !c.is_empty())
                .ok_or_else(|| "receiver capabilities missing valid audio codecs".to_string())?;
            (max_sr, max_bd, codecs)
        } else if let Some(output) = &info.output {
            let max_sr = output
                .get("max_sample_rate")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "receiver output missing valid max_sample_rate".to_string())? as u32;
            let max_bd = output
                .get("max_bit_depth")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "receiver output missing valid max_bit_depth".to_string())? as u32;
            let codecs = info
                .supported_codecs
                .clone()
                .filter(|c| !c.is_empty())
                .ok_or_else(|| "receiver output missing supported_codecs".to_string())?;
            (max_sr, max_bd, codecs)
        } else {
            return Err("receiver failed capability negotiation: no audio/output specifications found".to_string());
        };

        let mut caps = vec![
            "stream".to_string(),
            "volume".to_string(),
            "heartbeat".to_string(),
        ];
        if let Some(feats) = &info.features {
            if feats
                .get("ota_update")
                .and_then(|v| v.as_bool())
                .or_else(|| feats.get("ota").and_then(|v| v.as_bool()))
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
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        // ── Capability Negotiation (SERVER_CAPS ∩ RECEIVER_CAPS) ───────────
        if sample_rate > entry.max_sample_rate {
            return Err(format!(
                "requested sample rate {sample_rate} exceeds receiver maximum {}",
                entry.max_sample_rate
            ));
        }
        if bit_depth > entry.max_bit_depth {
            return Err(format!(
                "requested bit depth {bit_depth} exceeds receiver maximum {}",
                entry.max_bit_depth
            ));
        }
        if !entry.supported_codecs.is_empty() && !entry.supported_codecs.iter().any(|c| c == codec)
        {
            return Err(format!(
                "requested codec {codec} is not supported by receiver (supported: {:?})",
                entry.supported_codecs
            ));
        }

        let base_url = entry.base_url.clone();
        let token = entry.token.clone();
        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&base_url, id.clone())
        } else {
            ReceiverClient::new(&base_url)
        };
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

        if let Some(err) = &resp.error {
            let code = &err.code;
            let message = &err.message;
            return Err(format!("session_start failed: {code}: {message}"));
        }

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
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
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
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        client.set_volume(volume).await
    }

    pub async fn heartbeat(&self, receiver_id: &str) -> Result<HeartbeatResponse, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
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
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        let client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.get_info().await
    }
}

impl Default for ReceiverSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_session_rejects_exceeding_sample_rate() {
        let mgr = ReceiverSessionManager::new();
        let entry = ReceiverRegistryEntry {
            receiver_id: "rec-test-1".into(),
            name: "Standard Receiver".into(),
            device_type: "standard".into(),
            base_url: "http://127.0.0.1:9999".into(),
            paired: true,
            token: None,
            last_seen: None,
            capabilities: vec![],
            active_session_id: None,
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_codecs: vec!["pcm_s16le".into()],
            maximum_safe_volume: Some(100),
        };
        mgr.registry.write().await.add(entry);

        let res = mgr
            .start_session(
                "rec-test-1",
                "sess-1",
                "pcm_s16le",
                96000, // exceeds 48000
                16,
                2,
                9000,
                100,
                80,
            )
            .await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("requested sample rate 96000 exceeds receiver maximum 48000"));
    }

    #[tokio::test]
    async fn start_session_rejects_unsupported_codec() {
        let mgr = ReceiverSessionManager::new();
        let entry = ReceiverRegistryEntry {
            receiver_id: "rec-test-2".into(),
            name: "Standard Receiver".into(),
            device_type: "standard".into(),
            base_url: "http://127.0.0.1:9999".into(),
            paired: true,
            token: None,
            last_seen: None,
            capabilities: vec![],
            active_session_id: None,
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_codecs: vec!["pcm_s16le".into()],
            maximum_safe_volume: Some(100),
        };
        mgr.registry.write().await.add(entry);

        let res = mgr
            .start_session(
                "rec-test-2",
                "sess-2",
                "flac", // unsupported
                48000,
                16,
                2,
                9000,
                100,
                80,
            )
            .await;

        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("requested codec flac is not supported by receiver"));
    }
}
