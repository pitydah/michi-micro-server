use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::client::ReceiverClient;
use crate::models::*;
use crate::transport::{AudioTransport, RtpReceiverTransport, TransportStreamConfig};

pub type SharedAudioTransport = Arc<tokio::sync::Mutex<Box<dyn AudioTransport>>>;

/// Manages receiver sessions: pairing, heartbeat, session start/stop, volume.
#[derive(Clone)]
pub struct ReceiverSessionManager {
    registry: Arc<RwLock<ReceiverRegistry>>,
    identity: Option<Arc<michi_identity::IdentityManager>>,
    pending_pairings: Arc<RwLock<HashMap<String, PendingReceiverPairing>>>,
    active_sessions: Arc<RwLock<HashMap<String, ReceiverActiveSession>>>,
    active_transports: Arc<RwLock<HashMap<String, SharedAudioTransport>>>,
    heartbeat_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
}

impl std::fmt::Debug for ReceiverSessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiverSessionManager")
            .field("identity", &self.identity.is_some())
            .finish()
    }
}

impl ReceiverSessionManager {
    pub fn new() -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_with_identity(identity: Arc<michi_identity::IdentityManager>) -> Self {
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: Some(identity),
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn new_with(registry: Arc<RwLock<ReceiverRegistry>>) -> Self {
        Self {
            registry,
            identity: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn get_transport(
        &self,
        receiver_id: &str,
    ) -> Option<Arc<tokio::sync::Mutex<Box<dyn AudioTransport>>>> {
        self.active_transports
            .read()
            .await
            .get(receiver_id)
            .cloned()
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
        } else if let Some(ref err) = start_resp.error {
            return Err(format!("pair_start failed: {}: {}", err.code, err.message));
        } else {
            return Err(
                "INVALID_RECEIVER_RESPONSE: session_id is required in pair_start response"
                    .to_string(),
            );
        };

        let now = chrono::Utc::now();
        let expires_at = if let Some(ref exp_str) = start_resp.expires_at {
            chrono::DateTime::parse_from_rfc3339(exp_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| {
                    format!(
                        "INVALID_RECEIVER_RESPONSE: invalid RFC3339 expires_at '{exp_str}': {e}"
                    )
                })?
        } else {
            return Err(
                "INVALID_RECEIVER_RESPONSE: expires_at is required in pair_start response"
                    .to_string(),
            );
        };

        let pairing_id = uuid::Uuid::new_v4().to_string();

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
    pub async fn confirm_pairing(&self, pairing_id: &str, pin: &str) -> Result<String, String> {
        let pending = {
            let p = self.pending_pairings.read().await;
            p.get(pairing_id).cloned()
        }
        .ok_or_else(|| "pairing session not found or expired".to_string())?;

        if chrono::Utc::now() > pending.expires_at {
            let mut p = self.pending_pairings.write().await;
            p.remove(pairing_id);
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
            .await;

        let confirm_resp = match confirm_resp {
            Ok(resp) => resp,
            Err(e) => {
                // Strict typed handling of receiver error codes
                match e.code.as_str() {
                    "PAIRING_PIN_MISMATCH" => {
                        // Keep pending for user retry
                    }
                    "PAIRING_EXPIRED"
                    | "PAIRING_NOT_FOUND"
                    | "PAIRING_ALREADY_CONSUMED"
                    | "PAIRING_ATTEMPTS_EXCEEDED" => {
                        let mut p = self.pending_pairings.write().await;
                        p.remove(pairing_id);
                    }
                    _ => {
                        if e.http_status == 408 || e.http_status == 410 {
                            let mut p = self.pending_pairings.write().await;
                            p.remove(pairing_id);
                        }
                    }
                }
                return Err(format!("pair_confirm failed: {}: {}", e.code, e.message));
            }
        };

        if let Some(ref err) = confirm_resp.error {
            if err.code == "PAIRING_EXPIRED"
                || err.code == "PAIRING_NOT_FOUND"
                || err.code == "PAIRING_ALREADY_CONSUMED"
                || err.code == "PAIRING_ATTEMPTS_EXCEEDED"
            {
                let mut p = self.pending_pairings.write().await;
                p.remove(pairing_id);
            }
            return Err(format!(
                "pair_confirm failed: {}: {}",
                err.code, err.message
            ));
        }

        // On successful confirmation, clean up pending pairing
        {
            let mut p = self.pending_pairings.write().await;
            p.remove(pairing_id);
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

        // Extract discrete capabilities without fake defaults - require canonical info.audio
        let audio = info
            .audio
            .as_ref()
            .ok_or_else(|| "receiver failed capability negotiation: missing canonical 'audio' specification".to_string())?;

        let transports: Vec<String> = audio
            .get("transports")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .ok_or_else(|| "receiver capabilities missing valid audio.transports".to_string())?;

        if !transports.iter().any(|t| t == "rtp_udp") {
            return Err("receiver does not support required 'rtp_udp' audio transport".to_string());
        }

        let sample_rates: Vec<u32> = audio
            .get("sample_rates")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_u64().map(|n| n as u32))
                    .collect()
            })
            .filter(|v: &Vec<u32>| !v.is_empty())
            .ok_or_else(|| "receiver capabilities missing valid audio.sample_rates".to_string())?;

        let bit_depths: Vec<u32> = audio
            .get("bit_depths")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_u64().map(|n| n as u32))
                    .collect()
            })
            .filter(|v: &Vec<u32>| !v.is_empty())
            .ok_or_else(|| "receiver capabilities missing valid audio.bit_depths".to_string())?;

        let channels: Vec<u8> = audio
            .get("channels")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_u64().map(|n| n as u8))
                    .collect()
            })
            .filter(|v: &Vec<u8>| !v.is_empty())
            .ok_or_else(|| "receiver capabilities missing valid audio.channels".to_string())?;

        let codecs: Vec<String> = audio
            .get("codecs")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .filter(|v: &Vec<String>| !v.is_empty())
            .ok_or_else(|| "receiver capabilities missing valid audio.codecs".to_string())?;

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
    ) -> Result<NegotiatedReceiverSession, String> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| format!("receiver not found: {receiver_id}"))?;

        // ── Discrete Capability Negotiation (SERVER_CAPS ∩ RECEIVER_CAPS) ──
        if !entry.supported_sample_rates.is_empty()
            && !entry.supported_sample_rates.contains(&sample_rate)
        {
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

        if !entry.supported_bit_depths.is_empty()
            && !entry.supported_bit_depths.contains(&bit_depth)
        {
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

        if !entry.supported_channels.is_empty()
            && !entry.supported_channels.contains(&(channels as u8))
        {
            return Err(format!(
                "requested channel count {channels} is not in receiver supported channels {:?}",
                entry.supported_channels
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
        client.token = token.clone();

        let negotiated = client
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

        let receiver_session_id = negotiated.session_id.clone();
        let session_token = Some(negotiated.session_token.clone());
        let effective_port = negotiated.stream_port;
        let lease_seconds = negotiated.lease_seconds;
        let ssrc = negotiated.ssrc;

        // Create and start RtpReceiverTransport targeting receiver_host:effective_port with EXACT negotiated SSRC
        let host = base_url
            .trim_start_matches("http://")
            .trim_start_matches("https://")
            .split(':')
            .next()
            .unwrap_or("127.0.0.1");
        let target_addr = format!("{host}:{effective_port}");

        let mut transport = RtpReceiverTransport::new(&target_addr, ssrc);
        let config = TransportStreamConfig {
            codec: negotiated.codec.clone(),
            sample_rate: negotiated.sample_rate,
            bit_depth: negotiated.bit_depth,
            channels: negotiated.channels as u8,
            packet_ms: negotiated.packet_ms,
        };

        if let Err(e) = transport.start(config).await {
            // Best effort close remote receiver session if transport cannot start
            let _ = client.session_stop().await;
            return Err(format!(
                "failed to initialize audio transport to {target_addr}: {e}"
            ));
        }

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
            negotiated_codec: negotiated.codec.clone(),
            negotiated_sample_rate: negotiated.sample_rate,
            negotiated_bit_depth: negotiated.bit_depth,
            negotiated_channels: negotiated.channels,
            payload_type: negotiated.payload_type,
            ssrc,
            state: ReceiverActiveSessionState::Active,
            created_at: chrono::Utc::now(),
            last_heartbeat: chrono::Utc::now(),
        };

        {
            let mut sessions = self.active_sessions.write().await;
            sessions.insert(receiver_id.to_string(), active_sess);
        }

        {
            let mut transports = self.active_transports.write().await;
            transports.insert(
                receiver_id.to_string(),
                Arc::new(tokio::sync::Mutex::new(Box::new(transport))),
            );
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

        Ok(negotiated)
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
        let interval_secs = (lease_seconds / 3).clamp(2, 10);

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
        // 1. Mark session as Closing in RAM; do not delete active_transports yet
        let active_sess = {
            let mut sessions = self.active_sessions.write().await;
            let sess = sessions.get_mut(receiver_id).ok_or_else(|| {
                "NoActiveSession: cannot stop receiver session when none is active".to_string()
            })?;
            sess.state = ReceiverActiveSessionState::Closing;
            sess.clone()
        };

        // 2. Pause audio emission on active transport while preserving ownership
        if let Some(transport_lock) = self.active_transports.read().await.get(receiver_id) {
            let mut tr = transport_lock.lock().await;
            let _ = tr.pause().await;
        }

        // 3. Cancel heartbeat task
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

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        client.active_session_id = Some(active_sess.receiver_session_id.clone());
        client.active_session_token = active_sess.session_token.clone();

        // 4. Remote DELETE
        let resp = match client.session_stop().await {
            Ok(r) => r,
            Err(e) => {
                // Keep session marked Failed in RAM for retryability; preserve transport
                let mut sessions = self.active_sessions.write().await;
                if let Some(sess) = sessions.get_mut(receiver_id) {
                    sess.state = ReceiverActiveSessionState::Failed;
                }
                return Err(e);
            }
        };

        // 5. Success: call AudioTransport::stop(), clean active transport and active session
        if let Some(transport_lock) = self.active_transports.write().await.remove(receiver_id) {
            let mut tr = transport_lock.lock().await;
            let _ = tr.stop().await;
        }

        {
            let mut sessions = self.active_sessions.write().await;
            sessions.remove(receiver_id);
        }

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
            sessions.get(receiver_id).cloned().ok_or_else(|| {
                "NoActiveSession: cannot set volume without active session".to_string()
            })?
        };

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        client.active_session_id = Some(active_sess.receiver_session_id.clone());
        client.active_session_token = active_sess.session_token.clone();
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
            sessions.get(receiver_id).cloned().ok_or_else(|| {
                "NoActiveSession: cannot heartbeat without active session".to_string()
            })?
        };

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&entry.base_url, id.clone())
        } else {
            ReceiverClient::new(&entry.base_url)
        };
        client.token = entry.token.clone();
        client.active_session_id = Some(active_sess.receiver_session_id.clone());
        client.active_session_token = active_sess.session_token.clone();
        client.heartbeat_sequence.store(
            active_sess.heartbeat_sequence,
            std::sync::atomic::Ordering::SeqCst,
        );

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

    /// Internal & test-only method to stream synthetic PCM audio frames through the active RTP transport
    pub async fn send_test_pcm(&self, receiver_id: &str, pcm_data: &[u8]) -> Result<usize, String> {
        let transport_lock = self
            .get_transport(receiver_id)
            .await
            .ok_or_else(|| format!("NoActiveTransport for receiver {receiver_id}"))?;

        let mut tr = transport_lock.lock().await;
        tr.write_pcm(pcm_data)
            .await
            .map_err(|e| format!("write_pcm failed: {e:?}"))
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
        assert!(
            err.contains("requested sample rate 96000 is not in receiver supported rates [48000]")
        );
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

    #[tokio::test]
    async fn heartbeat_without_session_fails_locally() {
        let mgr = ReceiverSessionManager::new();
        let entry = ReceiverRegistryEntry {
            receiver_id: "rec-test-3".into(),
            name: "Test".into(),
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

        let res = mgr.heartbeat("rec-test-3").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("NoActiveSession"));
    }

    #[tokio::test]
    async fn volume_without_session_fails_locally() {
        let mgr = ReceiverSessionManager::new();
        let entry = ReceiverRegistryEntry {
            receiver_id: "rec-test-4".into(),
            name: "Test".into(),
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

        let res = mgr.set_volume("rec-test-4", 80).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("NoActiveSession"));
    }

    #[tokio::test]
    async fn stop_without_session_fails_locally() {
        let mgr = ReceiverSessionManager::new();
        let entry = ReceiverRegistryEntry {
            receiver_id: "rec-test-5".into(),
            name: "Test".into(),
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

        let res = mgr.stop_session("rec-test-5").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("NoActiveSession"));
    }
}
