use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::client::ReceiverClient;
use crate::models::*;

/// Manages receiver sessions: pairing, heartbeat, session start/stop, volume.
#[derive(Debug, Clone)]
pub struct ReceiverSessionManager {
    registry: Arc<RwLock<ReceiverRegistry>>,
    identity: Option<Arc<michi_identity::IdentityManager>>,
    pending_pairings: Arc<RwLock<HashMap<String, PendingReceiverPairing>>>,
    active_sessions: Arc<RwLock<HashMap<String, ReceiverActiveSession>>>,
    heartbeat_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl ReceiverSessionManager {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_with_identity(identity: Arc<michi_identity::IdentityManager>) -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: Some(identity),
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_with(registry: Arc<RwLock<ReceiverRegistry>>) -> Self {
        Self {
            registry,
            identity: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set_identity(&mut self, identity: Arc<michi_identity::IdentityManager>) {
        self.identity = Some(identity);
    }

    pub async fn registry(&self) -> Arc<RwLock<ReceiverRegistry>> {
        self.registry.clone()
    }

    pub async fn active_sessions(&self) -> Arc<RwLock<HashMap<String, ReceiverActiveSession>>> {
        self.active_sessions.clone()
    }

    pub async fn get_active_session(&self, receiver_id: &str) -> Option<ReceiverActiveSession> {
        self.active_sessions.read().await.get(receiver_id).cloned()
    }

    /// Step 1 of receiver pairing: Initiate pairing with Stream, store pending state, return pairing_id & session info.
    pub async fn start_pairing(
        &self,
        base_url: &str,
        initiator_id: &str,
    ) -> Result<PendingReceiverPairing, String> {
        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(base_url, id.clone())
        } else {
            ReceiverClient::new(base_url)
        };
        let info = client.get_info().await?;
        let start_resp = client.pair_start(initiator_id).await?;
        let pair_session_id = if let Some(ref s_id) = start_resp.session_id {
            s_id.clone()
        } else if let Some(ref n) = start_resp.nonce {
            n.clone()
        } else if let Some(ref err) = start_resp.error {
            return Err(format!("pair_start failed: {}: {}", err.code, err.message));
        } else {
            return Err("no session_id in pair_start response".to_string());
        };

        let pairing_id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now();
        let expires_in_secs = start_resp.expires_in.unwrap_or(start_resp.pairing_window_seconds.unwrap_or(60));
        let expires_at = now + chrono::Duration::seconds(expires_in_secs as i64);

        let pending = PendingReceiverPairing {
            pairing_id: pairing_id.clone(),
            receiver_base_url: base_url.trim_end_matches('/').to_string(),
            receiver_info: info,
            receiver_pair_session_id: pair_session_id,
            initiator_id: initiator_id.to_string(),
            created_at: now,
            expires_at,
        };

        // Clean expired pairings and save new pending pairing
        {
            let mut p = self.pending_pairings.write().await;
            p.retain(|_, v| v.expires_at > now);
            p.insert(pairing_id, pending.clone());
        }

        Ok(pending)
    }

    /// Step 2 of receiver pairing: Confirm pairing using the pairing_id and PIN entered by user.
    pub async fn confirm_pairing(
        &self,
        pairing_id: &str,
        pin: &str,
    ) -> Result<String, String> {
        let pending = {
            let mut p = self.pending_pairings.write().await;
            p.remove(pairing_id)
        }
        .ok_or_else(|| "pairing session not found or expired".to_string())?;

        if chrono::Utc::now() > pending.expires_at {
            return Err("pairing session expired".to_string());
        }

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&pending.receiver_base_url, id.clone())
        } else {
            ReceiverClient::new(&pending.receiver_base_url)
        };

        let confirm_resp = client
            .pair_confirm(
                &pending.receiver_pair_session_id,
                &pending.initiator_id,
                pin,
            )
            .await?;

        if let Some(ref err) = confirm_resp.error {
            return Err(format!("pair_confirm failed: {}: {}", err.code, err.message));
        }

        let info = pending.receiver_info;
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

        // Extract discrete capabilities without fake defaults
        let (sample_rates, bit_depths, channels, codecs) = if let Some(audio) = &info.audio {
            let srs: Vec<u32> = audio
                .get("sample_rates")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect())
                .filter(|v: &Vec<u32>| !v.is_empty())
                .ok_or_else(|| "receiver capabilities missing valid sample_rates".to_string())?;

            let bds: Vec<u32> = audio
                .get("bit_depths")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u32)).collect())
                .filter(|v: &Vec<u32>| !v.is_empty())
                .ok_or_else(|| "receiver capabilities missing valid bit_depths".to_string())?;

            let chs: Vec<u8> = audio
                .get("channels")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect())
                .unwrap_or_else(|| vec![2]);

            let cds: Vec<String> = audio
                .get("codecs")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .filter(|v: &Vec<String>| !v.is_empty())
                .ok_or_else(|| "receiver capabilities missing valid audio codecs".to_string())?;

            (srs, bds, chs, cds)
        } else if let Some(output) = &info.output {
            let max_sr = output
                .get("max_sample_rate")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "receiver output missing valid max_sample_rate".to_string())? as u32;
            let max_bd = output
                .get("max_bit_depth")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| "receiver output missing valid max_bit_depth".to_string())? as u32;
            let cds = info
                .supported_codecs
                .clone()
                .filter(|c| !c.is_empty())
                .ok_or_else(|| "receiver output missing supported_codecs".to_string())?;
            (vec![max_sr], vec![max_bd], vec![2], cds)
        } else {
            return Err("receiver failed capability negotiation: no audio/output specifications found".to_string());
        };

        let max_sr = *sample_rates.iter().max().unwrap_or(&48000);
        let max_bd = *bit_depths.iter().max().unwrap_or(&16);

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
            base_url: pending.receiver_base_url,
            paired: true,
            token: client.token.clone(),
            last_seen: Some(chrono::Utc::now()),
            capabilities: caps,
            active_session_id: None,
            max_sample_rate: max_sr,
            max_bit_depth: max_bd,
            supported_codecs: codecs,
            supported_sample_rates: sample_rates,
            supported_bit_depths: bit_depths,
            supported_channels: channels,
            maximum_safe_volume: Some(100),
        };

        self.registry.write().await.add(entry);
        Ok(device_id)
    }

    /// High-level 2-in-1 convenience for tests and internal workflows with known PIN.
    pub async fn discover_and_pair(
        &self,
        base_url: &str,
        initiator_id: &str,
        pin: &str,
    ) -> Result<String, String> {
        let pending = self.start_pairing(base_url, initiator_id).await?;
        self.confirm_pairing(&pending.pairing_id, pin).await
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

        // ── Discrete Capability Negotiation (SERVER_CAPS ∩ RECEIVER_CAPS) ──
        if !entry.supported_sample_rates.is_empty() && !entry.supported_sample_rates.contains(&sample_rate) {
            return Err(format!(
                "requested sample rate {sample_rate} is not in receiver supported rates {:?}",
                entry.supported_sample_rates
            ));
        } else if sample_rate > entry.max_sample_rate {
            return Err(format!(
                "requested sample rate {sample_rate} exceeds receiver maximum {}",
                entry.max_sample_rate
            ));
        }

        if !entry.supported_bit_depths.is_empty() && !entry.supported_bit_depths.contains(&bit_depth) {
            return Err(format!(
                "requested bit depth {bit_depth} is not in receiver supported depths {:?}",
                entry.supported_bit_depths
            ));
        } else if bit_depth > entry.max_bit_depth {
            return Err(format!(
                "requested bit depth {bit_depth} exceeds receiver maximum {}",
                entry.max_bit_depth
            ));
        }

        if !entry.supported_channels.is_empty() && !entry.supported_channels.contains(&(channels as u8)) {
            return Err(format!(
                "requested channel count {channels} is not in receiver supported channels {:?}",
                entry.supported_channels
            ));
        }

        if !entry.supported_codecs.is_empty() && !entry.supported_codecs.iter().any(|c| c == codec) {
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
        client.token = token.clone();

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

        let receiver_session_id = resp
            .session_id
            .clone()
            .unwrap_or_else(|| session_id.to_string());
        let session_token = resp.session_token.clone().or(client.active_session_token.clone());
        let effective_port = resp.stream_port.unwrap_or(stream_port);
        let lease_seconds = resp.lease_seconds.unwrap_or(30);

        // Store authoritative active session in manager RAM
        let active_sess = ReceiverActiveSession {
            receiver_id: receiver_id.to_string(),
            playback_session_id: session_id.to_string(),
            receiver_session_id: receiver_session_id.clone(),
            session_token: session_token.clone(),
            device_token: token.clone(),
            stream_port: effective_port,
            lease_seconds,
            heartbeat_sequence: 0,
            negotiated_codec: codec.to_string(),
            negotiated_sample_rate: sample_rate,
            negotiated_bit_depth: bit_depth,
            negotiated_channels: channels,
            payload_type: 97,
            ssrc: rand::random::<u32>().max(1),
            created_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
        };

        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(receiver_id.to_string(), active_sess);
        }

        {
            let mut reg = self.registry.write().await;
            if let Some(e) = reg.get_mut(receiver_id) {
                e.active_session_id = Some(receiver_session_id);
                e.last_seen = Some(chrono::Utc::now());
            }
        }

        // Spawn managed background heartbeat task
        self.spawn_heartbeat_task(receiver_id, lease_seconds).await;

        Ok(resp)
    }

    async fn spawn_heartbeat_task(&self, receiver_id: &str, lease_seconds: u64) {
        // Cancel existing task if any
        {
            let mut tokens = self.heartbeat_tokens.write().await;
            if let Some(old_token) = tokens.remove(receiver_id) {
                old_token.cancel();
            }
        }

        let cancel_token = CancellationToken::new();
        {
            let mut tokens = self.heartbeat_tokens.write().await;
            tokens.insert(receiver_id.to_string(), cancel_token.clone());
        }

        let mgr = self.clone();
        let rec_id = receiver_id.to_string();
        let interval_secs = (lease_seconds / 3).max(2).min(10);

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        break;
                    }
                    _ = interval.tick() => {
                        if let Err(e) = mgr.heartbeat(&rec_id).await {
                            tracing::warn!("managed receiver heartbeat failed for {}: {}", rec_id, e);
                        }
                    }
                }
            }
        });
    }

    pub async fn stop_session(&self, receiver_id: &str) -> Result<SessionStopResponse, String> {
        // Cancel heartbeat task immediately
        {
            let mut tokens = self.heartbeat_tokens.write().await;
            if let Some(token) = tokens.remove(receiver_id) {
                token.cancel();
            }
        }

        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        let active_sess = {
            let mut sessions = self.active_sessions.write().await;
            sessions.remove(receiver_id)
        };

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        if let Some(ref sess) = active_sess {
            client.active_session_id = Some(sess.receiver_session_id.clone());
            client.active_session_token = sess.session_token.clone();
        }

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

        let active_sess = {
            let sessions = self.active_sessions.read().await;
            sessions.get(receiver_id).cloned()
        };

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        if let Some(ref sess) = active_sess {
            client.active_session_id = Some(sess.receiver_session_id.clone());
            client.active_session_token = sess.session_token.clone();
        }
        client.set_volume(volume).await
    }

    pub async fn heartbeat(&self, receiver_id: &str) -> Result<HeartbeatResponse, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        let active_sess = {
            let sessions = self.active_sessions.read().await;
            sessions.get(receiver_id).cloned()
        };

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        if let Some(ref sess) = active_sess {
            client.active_session_id = Some(sess.receiver_session_id.clone());
            client.active_session_token = sess.session_token.clone();
            client.heartbeat_sequence.store(sess.heartbeat_sequence, std::sync::atomic::Ordering::SeqCst);
        }

        let resp = client.heartbeat().await?;

        // Update sequence and last_heartbeat in active session
        {
            let mut sessions = self.active_sessions.write().await;
            if let Some(sess) = sessions.get_mut(receiver_id) {
                sess.heartbeat_sequence += 1;
                sess.last_heartbeat = chrono::Utc::now();
            }
        }

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
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
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
        assert!(err.contains("requested sample rate 96000 is not in receiver supported rates [48000]"));
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
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
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
