use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::client::ReceiverClient;
use crate::models::*;
use crate::session_supervisor::ReceiverClientError;
use crate::transport::{AudioTransport, RtpReceiverTransport, TransportStreamConfig};

pub type SharedAudioTransport = Arc<tokio::sync::Mutex<Box<dyn AudioTransport>>>;

/// Supervised heartbeat task handle holding cancellation token and background join handle.
#[derive(Debug)]
pub struct ReceiverSupervisorHandle {
    pub cancel: CancellationToken,
    pub join: tokio::task::JoinHandle<()>,
}

/// Manages receiver sessions: pairing, heartbeat, session start/stop, volume.
#[derive(Clone)]
pub struct ReceiverSessionManager {
    registry: Arc<RwLock<ReceiverRegistry>>,
    identity: Option<Arc<michi_identity::IdentityManager>>,
    scent_store: Option<Arc<michi_connect::ScentStore>>,
    pending_pairings: Arc<RwLock<HashMap<String, PendingReceiverPairing>>>,
    active_sessions: Arc<RwLock<HashMap<String, ReceiverActiveSession>>>,
    active_transports: Arc<RwLock<HashMap<String, SharedAudioTransport>>>,
    heartbeat_handles: Arc<RwLock<HashMap<String, ReceiverSupervisorHandle>>>,
    authority_gate: Arc<crate::authority_gate::AuthorityGate>,
}

impl std::fmt::Debug for ReceiverSessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiverSessionManager")
            .field("identity", &self.identity.is_some())
            .field("scent_store", &self.scent_store.is_some())
            .finish()
    }
}

impl ReceiverSessionManager {
    pub fn new() -> Self {
        let michi_id = "anonymous".to_string();
        let authority_gate = Arc::new(crate::authority_gate::AuthorityGate::new(
            michi_id,
            "Michi Micro Server".to_string(),
            "micro-server".to_string(),
        ));
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: None,
            scent_store: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_handles: Arc::new(RwLock::new(HashMap::new())),
            authority_gate,
        }
    }

    pub fn new_with_identity(identity: Arc<michi_identity::IdentityManager>) -> Self {
        let michi_id = identity.michi_id().to_string();
        let authority_gate = Arc::new(crate::authority_gate::AuthorityGate::new(
            michi_id,
            "Michi Micro Server".to_string(),
            "micro-server".to_string(),
        ));
        Self {
            registry: Arc::new(RwLock::new(ReceiverRegistry::new())),
            identity: Some(identity),
            scent_store: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_handles: Arc::new(RwLock::new(HashMap::new())),
            authority_gate,
        }
    }

    pub fn new_with_identity_and_scent(
        identity: Arc<michi_identity::IdentityManager>,
        scent_store: Arc<michi_connect::ScentStore>,
    ) -> Self {
        let mut mgr = Self::new_with_identity(identity);
        mgr.set_scent_store(scent_store);
        mgr
    }

    pub fn new_with(registry: Arc<RwLock<ReceiverRegistry>>) -> Self {
        let michi_id = "anonymous".to_string();
        let authority_gate = Arc::new(crate::authority_gate::AuthorityGate::new(
            michi_id,
            "Michi Micro Server".to_string(),
            "micro-server".to_string(),
        ));
        Self {
            registry,
            identity: None,
            scent_store: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_handles: Arc::new(RwLock::new(HashMap::new())),
            authority_gate,
        }
    }

    pub fn set_identity(&mut self, identity: Arc<michi_identity::IdentityManager>) {
        self.identity = Some(identity);
    }

    pub fn set_scent_store(&mut self, scent_store: Arc<michi_connect::ScentStore>) {
        self.scent_store = Some(scent_store);
    }

    pub fn with_scent_store(mut self, scent_store: Arc<michi_connect::ScentStore>) -> Self {
        self.scent_store = Some(scent_store);
        self
    }

    pub fn scent_store(&self) -> Option<Arc<michi_connect::ScentStore>> {
        self.scent_store.clone()
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
}

fn validate_audio_capabilities(info: &ReceiverInfo) -> Result<(), String> {
    let audio = info.audio.as_ref().ok_or_else(|| {
        "CONTRACT_VIOLATION: receiver missing canonical 'audio' specification".to_string()
    })?;

    let transports: Vec<String> = audio
        .get("transports")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .ok_or_else(|| {
            "CONTRACT_VIOLATION: receiver capabilities missing valid audio.transports".to_string()
        })?;

    if !transports.iter().any(|t| t == "rtp_udp") {
        return Err(
            "CONTRACT_VIOLATION: receiver audio does not support required 'rtp_udp' transport"
                .to_string(),
        );
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
        .ok_or_else(|| {
            "CONTRACT_VIOLATION: receiver capabilities missing valid audio.sample_rates".to_string()
        })?;

    if !sample_rates.contains(&48000) {
        return Err(
            "CONTRACT_VIOLATION: receiver audio does not support required 48000 Hz sample rate"
                .to_string(),
        );
    }

    let bit_depths: Vec<u32> = audio
        .get("bit_depths")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().map(|n| n as u32))
                .collect()
        })
        .filter(|v: &Vec<u32>| !v.is_empty())
        .ok_or_else(|| {
            "CONTRACT_VIOLATION: receiver capabilities missing valid audio.bit_depths".to_string()
        })?;

    if !bit_depths.contains(&16) {
        return Err(
            "CONTRACT_VIOLATION: receiver audio does not support required 16-bit depth".to_string(),
        );
    }

    let channels: Vec<u8> = audio
        .get("channels")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_u64().map(|n| n as u8))
                .collect()
        })
        .filter(|v: &Vec<u8>| !v.is_empty())
        .ok_or_else(|| {
            "CONTRACT_VIOLATION: receiver capabilities missing valid audio.channels".to_string()
        })?;

    if !channels.contains(&2) {
        return Err(
            "CONTRACT_VIOLATION: receiver audio does not support required 2 channels (stereo)"
                .to_string(),
        );
    }

    let codecs: Vec<String> = audio
        .get("codecs")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
        .ok_or_else(|| {
            "CONTRACT_VIOLATION: receiver capabilities missing valid audio.codecs".to_string()
        })?;

    if !codecs.iter().any(|c| c == "pcm_s16le") {
        return Err(
            "CONTRACT_VIOLATION: receiver audio does not support required 'pcm_s16le' codec"
                .to_string(),
        );
    }

    let _ = (sample_rates, bit_depths, channels, codecs);
    Ok(())
}

impl ReceiverSessionManager {
    /// Step 1 of receiver pairing: Initiate pairing with Stream, store pending state, return pairing_id & session info.
    pub async fn start_pairing(
        &self,
        base_url: &str,
        initiator_id: &str,
    ) -> Result<PendingReceiverPairing, String> {
        self.start_pairing_ext(base_url, initiator_id, false).await
    }

    /// Step 1 of receiver pairing with optional re-pairing permission for already-paired receivers.
    pub async fn start_pairing_ext(
        &self,
        base_url: &str,
        initiator_id: &str,
        allow_re_pair: bool,
    ) -> Result<PendingReceiverPairing, String> {
        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(base_url, id.clone())
        } else {
            ReceiverClient::new(base_url)
        };
        let info = client.get_info().await?;

        // Mandatory contract validation BEFORE initiating pair_start remotely:
        // 1. server_id not empty and valid canonical UUID
        let expected_server_id = info
            .server_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: server_id is required and non-empty in receiver info"
                    .to_string()
            })?
            .to_string();

        uuid::Uuid::parse_str(&expected_server_id).map_err(|e| {
            format!("CONTRACT_VIOLATION: server_id must be a valid canonical UUID: {e}")
        })?;

        // 2. michi_id not empty
        let expected_michi_id = info
            .michi_id
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: michi_id is required and non-empty in receiver info"
                    .to_string()
            })?
            .to_string();

        // 3. public_key not empty, valid base64url, exactly 32 bytes, valid Ed25519
        let expected_public_key = info
            .public_key
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: public_key is required and non-empty in receiver info"
                    .to_string()
            })?
            .to_string();

        let pk_bytes = michi_identity::decode_base64url_strict(&expected_public_key)
            .map_err(|e| format!("CONTRACT_VIOLATION: invalid public_key base64url: {e}"))?;
        if pk_bytes.len() != 32 {
            return Err("CONTRACT_VIOLATION: public_key must be exactly 32 bytes".to_string());
        }
        let key_bytes: [u8; 32] = pk_bytes
            .as_slice()
            .try_into()
            .map_err(|_| "CONTRACT_VIOLATION: failed converting public key bytes".to_string())?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)
            .map_err(|e| format!("CONTRACT_VIOLATION: invalid Ed25519 public key: {e}"))?;
        let derived_id =
            michi_identity::types::MichiId::from_public_key(&verifying_key).to_base64url();

        if expected_michi_id != derived_id {
            return Err(format!(
                "CONTRACT_VIOLATION: michi_id '{expected_michi_id}' does not match derived public_key identity '{derived_id}'"
            ));
        }

        // 4. identity_scheme EXACTLY "ed25519-blake3-v1"
        let identity_scheme = info
            .identity_scheme
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: identity_scheme is required in receiver info".to_string()
            })?;
        if identity_scheme != "ed25519-blake3-v1" {
            return Err(format!(
                "CONTRACT_VIOLATION: identity_scheme must be 'ed25519-blake3-v1', got '{identity_scheme}'"
            ));
        }

        // 5. service EXACTLY "michi-stream-standard" or "michi-stream-hifi"
        let service = info
            .service
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: service is required in receiver info".to_string()
            })?;
        if service != "michi-stream-standard" && service != "michi-stream-hifi" {
            return Err(format!(
                "CONTRACT_VIOLATION: unsupported receiver service '{service}', expected 'michi-stream-standard' or 'michi-stream-hifi'"
            ));
        }

        // 6. api_version EXACTLY "v1-lite"
        let api_version = info
            .api_version
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: api_version is required in receiver info".to_string()
            })?;
        if api_version != "v1-lite" {
            return Err(format!(
                "CONTRACT_VIOLATION: unsupported api_version '{api_version}', expected 'v1-lite'"
            ));
        }

        // 7. roles contains "audio_receiver"
        let roles = info
            .roles
            .as_ref()
            .ok_or_else(|| "CONTRACT_VIOLATION: roles is required in receiver info".to_string())?;
        if !roles.iter().any(|r| r.trim() == "audio_receiver") {
            return Err("CONTRACT_VIOLATION: roles must contain 'audio_receiver'".to_string());
        }

        // 8. audio capabilities validation
        validate_audio_capabilities(&info)?;

        // 9. verify receiver is pairable in local registry (no identity mismatch, not already paired unless allow_re_pair)
        {
            let reg = self.registry.read().await;
            if let Some(entry) = reg
                .get(&expected_michi_id)
                .or_else(|| reg.get(&expected_server_id))
            {
                if entry.qualification == ReceiverQualification::IdentityMismatch {
                    return Err(
                        "IDENTITY_MISMATCH: pairing blocked due to identity conflict".to_string(),
                    );
                }
                if entry.paired && !allow_re_pair {
                    return Err("ALREADY_PAIRED: receiver is already paired".to_string());
                }
            }
        }

        // ONLY IF FULL CONTRACT IS VALID: initiate remote pair_start
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

        let start_michi_id = start_resp.server_michi_id.as_ref().ok_or_else(|| {
            "CONTRACT_VIOLATION: server_michi_id is required in pair_start response".to_string()
        })?;
        let start_public_key = start_resp.server_public_key.as_ref().ok_or_else(|| {
            "CONTRACT_VIOLATION: server_public_key is required in pair_start response".to_string()
        })?;

        if start_michi_id != &expected_michi_id {
            return Err(format!(
                "CONTRACT_VIOLATION: pair_start server_michi_id '{start_michi_id}' does not match server/info michi_id '{expected_michi_id}'"
            ));
        }
        if start_public_key != &expected_public_key {
            return Err(format!(
                "CONTRACT_VIOLATION: pair_start server_public_key '{start_public_key}' does not match server/info public_key '{expected_public_key}'"
            ));
        }

        // Verify derive(server_public_key) == server_michi_id
        let start_pk_bytes =
            michi_identity::decode_base64url_strict(start_public_key).map_err(|e| {
                format!("CONTRACT_VIOLATION: invalid pair_start server_public_key base64url: {e}")
            })?;
        if start_pk_bytes.len() != 32 {
            return Err(
                "CONTRACT_VIOLATION: pair_start server_public_key must be exactly 32 bytes"
                    .to_string(),
            );
        }
        let start_key_bytes: [u8; 32] = start_pk_bytes.as_slice().try_into().map_err(|_| {
            "CONTRACT_VIOLATION: failed converting start public key bytes".to_string()
        })?;
        let start_verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&start_key_bytes)
            .map_err(|e| format!("CONTRACT_VIOLATION: invalid Ed25519 start public key: {e}"))?;
        let start_derived_id =
            michi_identity::types::MichiId::from_public_key(&start_verifying_key).to_base64url();
        if start_michi_id != &start_derived_id {
            return Err(format!(
                "CONTRACT_VIOLATION: pair_start server_michi_id '{start_michi_id}' does not match derived identity '{start_derived_id}'"
            ));
        }

        let pending = PendingReceiverPairing {
            pairing_id: pairing_id.clone(),
            receiver_base_url: base_url.trim_end_matches('/').to_string(),
            receiver_info: info,
            receiver_pair_session_id: pair_session_id,
            initiator_id: initiator_id.to_string(),
            created_at: now,
            expires_at,
            server_michi_id: Some(expected_michi_id.clone()),
            server_public_key: Some(expected_public_key.clone()),
            expected_server_id,
            expected_michi_id,
            expected_public_key,
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

        // Enforce identity pre-verification BEFORE calling pair_confirm
        let pre_info = client.get_info().await?;
        let pre_server_id = pre_info.server_id.as_ref().ok_or_else(|| {
            "CONTRACT_VIOLATION: server_id is required in receiver info".to_string()
        })?;
        uuid::Uuid::parse_str(pre_server_id).map_err(|e| {
            format!("CONTRACT_VIOLATION: server_id must be a valid canonical UUID: {e}")
        })?;
        if pre_server_id != &pending.expected_server_id {
            return Err(format!(
                "IDENTITY_MISMATCH: receiver server_id changed before pair_confirm (expected '{}', got '{}')",
                pending.expected_server_id, pre_server_id
            ));
        }
        if pre_info.michi_id.as_ref() != Some(&pending.expected_michi_id) {
            return Err(format!(
                "IDENTITY_MISMATCH: receiver michi_id changed before pair_confirm (expected '{}', got '{:?}')",
                pending.expected_michi_id, pre_info.michi_id
            ));
        }
        if pre_info.public_key.as_ref() != Some(&pending.expected_public_key) {
            return Err(format!(
                "IDENTITY_MISMATCH: receiver public_key changed before pair_confirm (expected '{}', got '{:?}')",
                pending.expected_public_key, pre_info.public_key
            ));
        }

        let pre_scheme = pre_info
            .identity_scheme
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "CONTRACT_VIOLATION: identity_scheme is required in receiver info".to_string()
            })?;
        if pre_scheme != "ed25519-blake3-v1" {
            return Err(format!(
                "CONTRACT_VIOLATION: identity_scheme must be 'ed25519-blake3-v1', got '{pre_scheme}'"
            ));
        }

        // Validate audio capabilities before pair_confirm so remote pairing is never consumed if capabilities are invalid
        validate_audio_capabilities(&pre_info)?;

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

        // Re-fetch fresh info to enforce identity pinning post-confirmation
        let fresh_info = client.get_info().await?;
        let fresh_server_id = fresh_info.server_id.as_ref().ok_or_else(|| {
            "CONTRACT_VIOLATION: server_id is required in receiver info".to_string()
        })?;
        if fresh_server_id != &pending.expected_server_id {
            return Err(format!(
                "IDENTITY_MISMATCH: receiver server_id changed post-confirmation (expected '{}', got '{}')",
                pending.expected_server_id, fresh_server_id
            ));
        }
        if fresh_info.michi_id.as_ref() != Some(&pending.expected_michi_id) {
            return Err(format!(
                "IDENTITY_MISMATCH: receiver michi_id changed post-confirmation (expected '{}', got '{:?}')",
                pending.expected_michi_id, fresh_info.michi_id
            ));
        }
        if fresh_info.public_key.as_ref() != Some(&pending.expected_public_key) {
            return Err(format!(
                "IDENTITY_MISMATCH: receiver public_key changed post-confirmation (expected '{}', got '{:?}')",
                pending.expected_public_key, fresh_info.public_key
            ));
        }

        let info = fresh_info;
        let device_id = info
            .michi_id
            .clone()
            .or_else(|| info.server_id.clone())
            .ok_or_else(|| {
                "IDENTITY_MISMATCH: missing device identifier post-confirmation".to_string()
            })?;
        let name = info.name.clone().unwrap_or_else(|| device_id.clone());
        let device_type = info
            .device_type
            .clone()
            .or_else(|| info.service.clone())
            .unwrap_or_else(|| "unknown".into());

        // Extract discrete capabilities without fake defaults - require canonical info.audio
        let audio = info.audio.as_ref().ok_or_else(|| {
            "receiver failed capability negotiation: missing canonical 'audio' specification"
                .to_string()
        })?;

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

        let authority_supported = info
            .features
            .as_ref()
            .and_then(|f| f.get("authority_v1").or_else(|| f.get("perch_v1")))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let target_presence = if let Some(ref scent) = self.scent_store {
            if let Some(record) = scent.get(&pending.expected_michi_id) {
                match record.effective_presence(std::time::Instant::now()) {
                    michi_connect::scent_store::EffectivePresence::VerifiedOnline => {
                        ReceiverPresence::VerifiedOnline
                    }
                    michi_connect::scent_store::EffectivePresence::ProvisionalMdns => {
                        ReceiverPresence::ProvisionalMdns
                    }
                    michi_connect::scent_store::EffectivePresence::Offline => {
                        ReceiverPresence::Offline
                    }
                }
            } else {
                ReceiverPresence::Offline
            }
        } else {
            ReceiverPresence::Offline
        };

        let entry = ReceiverRegistryEntry {
            receiver_id: device_id.clone(),
            michi_id: info.michi_id.clone(),
            name,
            device_type,
            base_url: pending.receiver_base_url,
            paired: true,
            token: client.token.clone(),
            presence: target_presence,
            last_seen: Some(chrono::Utc::now()),
            capabilities: caps,
            capabilities_verified_at: Some(chrono::Utc::now()),
            capabilities_stale: false,
            authority_supported,
            owner_michi_id: None,
            owner_name: None,
            active_session_id: None,
            max_sample_rate: max_sr,
            max_bit_depth: max_bd,
            supported_transports: transports,
            supported_codecs: codecs,
            supported_sample_rates: sample_rates,
            supported_bit_depths: bit_depths,
            supported_channels: channels,
            maximum_safe_volume: Some(100),
            qualification: ReceiverQualification::Qualified,
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

    pub fn authority_gate(&self) -> Arc<crate::authority_gate::AuthorityGate> {
        self.authority_gate.clone()
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
        self.start_session_with_authority(
            receiver_id,
            session_id,
            codec,
            sample_rate,
            bit_depth,
            channels,
            stream_port,
            buffer_ms,
            volume,
            None,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start_session_with_authority(
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
        authority: Option<&crate::authority_models::AuthorityGrant>,
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

        // If grant not explicitly passed, attempt to ensure claim via AuthorityGate
        let claimed_grant = if authority.is_none() {
            if entry.supports_authority_v1() {
                match self.authority_gate.ensure_claim(&entry).await {
                    Ok(grant) => grant,
                    Err(crate::authority_models::AuthorityError::Unsupported) => None,
                    Err(err) => {
                        return Err(format!(
                            "PERCH_AUTHORITY_FAILED: authority claim failed ({err:?})"
                        ));
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        let effective_grant = authority.or(claimed_grant.as_ref());

        let negotiated = client
            .session_start_with_authority(
                session_id,
                codec,
                sample_rate,
                bit_depth,
                channels,
                stream_port,
                buffer_ms,
                volume,
                effective_grant,
            )
            .await?;

        let receiver_session_id = negotiated.session_id.clone();
        let session_token = Some(negotiated.session_token.clone());
        let effective_port = negotiated.stream_port;
        let lease_seconds = negotiated.lease_seconds;
        let ssrc = negotiated.ssrc;

        // Create and start RtpReceiverTransport targeting receiver_host:effective_port with EXACT negotiated SSRC
        let endpoint = url::Url::parse(&base_url)
            .map_err(|e| format!("Invalid receiver base_url '{base_url}': {e}"))?;
        let host = endpoint
            .host_str()
            .ok_or_else(|| format!("Invalid receiver endpoint host in '{base_url}'"))?;
        let target_addr = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]:{effective_port}")
        } else {
            format!("{host}:{effective_port}")
        };

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
        // Cancel existing task and join if any
        let old_handle = {
            let mut handles = self.heartbeat_handles.write().await;
            handles.remove(receiver_id)
        };
        if let Some(h) = old_handle {
            h.cancel.cancel();
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), h.join).await;
        }

        let cancel_token = CancellationToken::new();
        let loop_token = cancel_token.clone();

        let mgr = self.clone();
        let rec_id = receiver_id.to_string();
        let interval_secs = (lease_seconds / 6).clamp(1, 4);
        let lease_dur = std::time::Duration::from_secs(lease_seconds);

        let join_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(interval_secs));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // Skip the immediate first tick so interval starts ticking at interval_secs
            interval.tick().await;

            let mut consecutive_failures = 0u32;
            let mut last_success = std::time::Instant::now();

            loop {
                tokio::select! {
                    _ = loop_token.cancelled() => {
                        break;
                    }
                    _ = interval.tick() => {
                        match mgr.heartbeat(&rec_id).await {
                            Ok(_) => {
                                consecutive_failures = 0;
                                last_success = std::time::Instant::now();
                            }
                            Err(e) => {
                                consecutive_failures += 1;
                                let elapsed = last_success.elapsed();
                                let disp = crate::session_supervisor::classify_heartbeat_error_typed(
                                    &e,
                                    consecutive_failures,
                                    elapsed,
                                    lease_dur,
                                );

                                match disp {
                                    crate::session_supervisor::HeartbeatDisposition::Continue => {}
                                    crate::session_supervisor::HeartbeatDisposition::RetryTransient(n) => {
                                        tracing::warn!(
                                            receiver_id = %rec_id,
                                            consecutive = n,
                                            err = %e,
                                            "managed receiver heartbeat transient failure, retrying"
                                        );
                                    }
                                    crate::session_supervisor::HeartbeatDisposition::SessionLost(reason) => {
                                        tracing::error!(
                                            receiver_id = %rec_id,
                                            reason = %reason,
                                            err = %e,
                                            "managed receiver heartbeat lost session! Tearing down active session in RAM and RTP transport"
                                        );
                                        mgr.handle_session_lost(&rec_id).await;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        {
            let mut handles = self.heartbeat_handles.write().await;
            handles.insert(
                receiver_id.to_string(),
                ReceiverSupervisorHandle {
                    cancel: cancel_token,
                    join: join_handle,
                },
            );
        }
    }

    /// Tear down local session in RAM and stop RTP transport when session is lost or revoked
    pub async fn handle_session_lost(&self, receiver_id: &str) {
        // 1. Remove active session from RAM
        let _ = self.active_sessions.write().await.remove(receiver_id);

        // 2. Stop and drop RTP transport
        let transport_opt = self.active_transports.write().await.remove(receiver_id);
        if let Some(transport_lock) = transport_opt {
            let mut tr = transport_lock.lock().await;
            let _ = tr.stop().await;
        }

        // 3. Clear active_session_id in registry
        {
            let mut reg = self.registry.write().await;
            if let Some(e) = reg.get_mut(receiver_id) {
                e.active_session_id = None;
            }
        }

        // 4. Cancel and await heartbeat handle (avoid self-join if called from within the supervisor task)
        let handle_opt = {
            let mut handles = self.heartbeat_handles.write().await;
            handles.remove(receiver_id)
        };
        if let Some(h) = handle_opt {
            h.cancel.cancel();
            let is_self = tokio::task::try_id()
                .map(|id| id == h.join.id())
                .unwrap_or(false);
            if !is_self {
                let _ = tokio::time::timeout(std::time::Duration::from_millis(500), h.join).await;
            }
        }

        // 5. Invalidate Perch authority grant in RAM
        self.authority_gate.invalidate_grant(receiver_id).await;
    }

    /// Propagate pause / resume to active receiver session
    pub async fn patch_session(&self, receiver_id: &str, paused: bool) -> Result<(), String> {
        let (base_url, token, session_id, session_token) = {
            let sessions = self.active_sessions.read().await;
            let sess = sessions.get(receiver_id).ok_or_else(|| {
                format!("NoActiveSession: receiver {receiver_id} has no active session to patch")
            })?;
            let reg = self.registry.read().await;
            let entry = reg.get(receiver_id).ok_or_else(|| {
                format!("ReceiverNotFound: receiver {receiver_id} not in registry")
            })?;
            (
                entry.base_url.clone(),
                entry.token.clone(),
                sess.receiver_session_id.clone(),
                sess.session_token.clone(),
            )
        };

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&base_url, id.clone())
        } else {
            ReceiverClient::new(&base_url)
        };
        client.token = token;
        client.active_session_id = Some(session_id);
        client.active_session_token = session_token;

        client.session_pause(paused).await
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

        // 3. Cancel and await heartbeat task
        let handle_opt = {
            let mut handles = self.heartbeat_handles.write().await;
            handles.remove(receiver_id)
        };
        if let Some(h) = handle_opt {
            h.cancel.cancel();
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), h.join).await;
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

        // Release Perch authority if held
        let _ = self.authority_gate.release_grant(&entry).await;

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

    pub async fn heartbeat(
        &self,
        receiver_id: &str,
    ) -> Result<HeartbeatResponse, ReceiverClientError> {
        let entry = {
            let reg = self.registry.read().await;
            reg.get(receiver_id).cloned()
        }
        .ok_or_else(|| {
            ReceiverClientError::Offline(format!("receiver not found: {receiver_id}"))
        })?;

        let active_sess = {
            let sessions = self.active_sessions.read().await;
            sessions.get(receiver_id).cloned().ok_or_else(|| {
                ReceiverClientError::Protocol(
                    "NoActiveSession: cannot heartbeat without active session".to_string(),
                )
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

    /// Productive method to stream PCM audio frames to an active receiver transport.
    pub async fn write_pcm(&self, receiver_id: &str, pcm_data: &[u8]) -> Result<usize, String> {
        let transport_lock = self
            .get_transport(receiver_id)
            .await
            .ok_or_else(|| format!("NoActiveTransport for receiver {receiver_id}"))?;

        let mut tr = transport_lock.lock().await;
        tr.write_pcm(pcm_data)
            .await
            .map_err(|e| format!("write_pcm failed: {e:?}"))
    }

    /// Compatibility method to stream PCM audio frames through the active RTP transport.
    #[deprecated(note = "use write_pcm() in production")]
    pub async fn send_test_pcm(&self, receiver_id: &str, pcm_data: &[u8]) -> Result<usize, String> {
        self.write_pcm(receiver_id, pcm_data).await
    }

    /// Query the local UDP socket port used by the active RTP transport for a receiver.
    pub async fn get_rtp_local_port(&self, receiver_id: &str) -> Option<u16> {
        let transports = self.active_transports.read().await;
        let transport_lock = transports.get(receiver_id)?.clone();
        let tr = transport_lock.lock().await;
        tr.local_port()
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
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".into()],
            supported_codecs: vec!["pcm_s16le".into()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            ..Default::default()
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
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".into()],
            supported_codecs: vec!["pcm_s16le".into()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            ..Default::default()
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
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".into()],
            supported_codecs: vec!["pcm_s16le".into()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        let res = mgr.heartbeat("rec-test-3").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("NoActiveSession"));
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
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".into()],
            supported_codecs: vec!["pcm_s16le".into()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            ..Default::default()
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
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".into()],
            supported_codecs: vec!["pcm_s16le".into()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        let res = mgr.stop_session("rec-test-5").await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("NoActiveSession"));
    }

    #[tokio::test]
    async fn supervisor_self_join_does_not_deadlock_on_session_loss() {
        let mgr = std::sync::Arc::new(ReceiverSessionManager::new());
        let rec_id = "rec-self-join-test";

        let entry = ReceiverRegistryEntry {
            receiver_id: rec_id.into(),
            name: "Test".into(),
            device_type: "standard".into(),
            base_url: "http://127.0.0.1:9999".into(),
            paired: true,
            max_sample_rate: 48000,
            max_bit_depth: 16,
            supported_transports: vec!["rtp_udp".into()],
            supported_codecs: vec!["pcm_s16le".into()],
            supported_sample_rates: vec![48000],
            supported_bit_depths: vec![16],
            supported_channels: vec![2],
            active_session_id: Some("sess-123".into()),
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        // Simulate an active session
        mgr.active_sessions.write().await.insert(
            rec_id.into(),
            crate::ReceiverActiveSession {
                receiver_id: rec_id.into(),
                playback_session_id: "pb-123".into(),
                receiver_session_id: "sess-123".into(),
                session_token: Some("sess-tok-1".into()),
                device_token: Some("tok-1".into()),
                stream_port: 53318,
                lease_seconds: 30,
                heartbeat_sequence: 1,
                negotiated_codec: "pcm_s16le".into(),
                negotiated_sample_rate: 48000,
                negotiated_bit_depth: 16,
                negotiated_channels: 2,
                payload_type: 96,
                ssrc: 12345,
                state: crate::ReceiverActiveSessionState::Active,
                created_at: chrono::Utc::now(),
                last_heartbeat: chrono::Utc::now(),
            },
        );

        let mgr_clone = mgr.clone();
        let rec_id_str = rec_id.to_string();
        let cancel = tokio_util::sync::CancellationToken::new();

        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        let join = tokio::spawn(async move {
            let _ = started_tx.send(());
            mgr_clone.handle_session_lost(&rec_id_str).await;
            let _ = done_tx.send(());
        });

        // Register handle in heartbeat_handles before task executes handle_session_lost
        {
            let mut handles = mgr.heartbeat_handles.write().await;
            handles.insert(
                rec_id.into(),
                super::ReceiverSupervisorHandle { cancel, join },
            );
        }

        let _ = started_rx.await;
        let finished = tokio::time::timeout(std::time::Duration::from_secs(2), done_rx).await;
        assert!(finished.is_ok(), "task must complete without deadlock");

        // Verify session cleared
        assert!(mgr.active_sessions.read().await.get(rec_id).is_none());
        assert!(mgr.heartbeat_handles.read().await.get(rec_id).is_none());
        let reg = mgr.registry.read().await;
        assert_eq!(
            reg.get(rec_id).and_then(|e| e.active_session_id.as_ref()),
            None
        );
    }

    // =========================================================================
    // A18 Test Matrix: Pairing Tests 1-10
    // =========================================================================

    use axum::extract::State as AxumState;
    use axum::routing::{get, post};
    use axum::Json as AxumJson;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    #[derive(Clone)]
    struct MockReceiverState {
        info_json: std::sync::Arc<std::sync::RwLock<serde_json::Value>>,
        start_json: std::sync::Arc<std::sync::RwLock<serde_json::Value>>,
        confirm_json: std::sync::Arc<std::sync::RwLock<serde_json::Value>>,
        pair_start_called: std::sync::Arc<AtomicBool>,
        pair_confirm_called: std::sync::Arc<AtomicBool>,
        info_call_count: std::sync::Arc<AtomicUsize>,
        mutate_on_confirm: std::sync::Arc<AtomicBool>,
    }

    async fn mock_server_info(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> AxumJson<serde_json::Value> {
        st.info_call_count.fetch_add(1, Ordering::SeqCst);
        let val = st.info_json.read().unwrap().clone();
        AxumJson(val)
    }

    async fn mock_pair_start(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> AxumJson<serde_json::Value> {
        st.pair_start_called.store(true, Ordering::SeqCst);
        let val = st.start_json.read().unwrap().clone();
        AxumJson(val)
    }

    async fn mock_pair_confirm(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> AxumJson<serde_json::Value> {
        st.pair_confirm_called.store(true, Ordering::SeqCst);
        if st.mutate_on_confirm.load(Ordering::SeqCst) {
            let mut info = st.info_json.write().unwrap();
            let obj = info.as_object_mut().unwrap();
            obj.insert(
                "server_id".to_string(),
                serde_json::Value::String("660e8400-e29b-41d4-a716-446655440000".to_string()),
            );
            obj.insert(
                "device_id".to_string(),
                serde_json::Value::String("660e8400-e29b-41d4-a716-446655440000".to_string()),
            );
        }
        let val = st.confirm_json.read().unwrap().clone();
        AxumJson(val)
    }

    async fn spawn_mock_receiver(st: MockReceiverState) -> (String, tokio::task::JoinHandle<()>) {
        let app = axum::Router::new()
            .route("/api/v1/server/info", get(mock_server_info))
            .route("/api/v1/pair/start", post(mock_pair_start))
            .route("/api/v1/pair/confirm", post(mock_pair_confirm))
            .with_state(st);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        (format!("http://{addr}"), handle)
    }

    fn default_mock_state() -> MockReceiverState {
        let info = serde_json::json!({
            "service": "michi-stream-standard",
            "name": "Test Stream",
            "device_id": "550e8400-e29b-41d4-a716-446655440000",
            "server_id": "550e8400-e29b-41d4-a716-446655440000",
            "identity_scheme": "ed25519-blake3-v1",
            "michi_id": "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4",
            "public_key": "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8",
            "api_version": "v1-lite",
            "roles": ["audio_receiver"],
            "supported_codecs": ["pcm_s16le"],
            "audio": {
                "transports": ["rtp_udp"],
                "codecs": ["pcm_s16le"],
                "sample_rates": [48000],
                "bit_depths": [16],
                "channels": [2],
            }
        });

        let start = serde_json::json!({
            "session_id": "pair-sess-1",
            "expires_at": chrono::Utc::now().checked_add_signed(chrono::Duration::seconds(60)).unwrap().to_rfc3339(),
            "server_michi_id": "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4",
            "server_public_key": "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8",
        });

        let confirm = serde_json::json!({
            "status": "paired",
            "token": "test-device-token-12345",
            "device_id": "550e8400-e29b-41d4-a716-446655440000",
            "server_id": "550e8400-e29b-41d4-a716-446655440000",
        });

        MockReceiverState {
            info_json: std::sync::Arc::new(std::sync::RwLock::new(info)),
            start_json: std::sync::Arc::new(std::sync::RwLock::new(start)),
            confirm_json: std::sync::Arc::new(std::sync::RwLock::new(confirm)),
            pair_start_called: std::sync::Arc::new(AtomicBool::new(false)),
            pair_confirm_called: std::sync::Arc::new(AtomicBool::new(false)),
            info_call_count: std::sync::Arc::new(AtomicUsize::new(0)),
            mutate_on_confirm: std::sync::Arc::new(AtomicBool::new(false)),
        }
    }

    fn make_test_session_manager() -> ReceiverSessionManager {
        let dir = std::env::temp_dir().join(format!("test-id-sess-{}", uuid::Uuid::new_v4()));
        let _ = std::fs::create_dir_all(&dir);
        let id = std::sync::Arc::new(
            michi_identity::IdentityManager::generate(&dir, "Session Test", "").unwrap(),
        );
        ReceiverSessionManager::new_with_identity(id)
    }

    // 1. start_pairing and confirm_pairing succeed with valid receiver info & pair_start response
    #[tokio::test]
    async fn test_pairing_1_success_valid_flow() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let pending = mgr
            .start_pairing(&base_url, "initiator-1")
            .await
            .expect("start_pairing should succeed");
        assert_eq!(
            pending.expected_server_id,
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(
            pending.expected_michi_id,
            "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4"
        );
        assert_eq!(
            pending.expected_public_key,
            "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8"
        );

        let rec_id = mgr
            .confirm_pairing(&pending.pairing_id, "123456")
            .await
            .expect("confirm_pairing should succeed");
        assert_eq!(rec_id, "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4");
        assert!(st.pair_confirm_called.load(Ordering::SeqCst));

        let reg = mgr.registry.read().await;
        assert!(reg.get(&rec_id).is_some());
    }

    // 2. start_pairing fails if michi_id missing in info; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_2_fails_missing_michi_id() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("michi_id");
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(err.contains("michi_id is required"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when michi_id is missing"
        );
    }

    // 3. start_pairing fails if public_key missing in info; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_3_fails_missing_public_key() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("public_key");
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(err.contains("public_key is required"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when public_key is missing"
        );
    }

    // 4. start_pairing fails if server_id missing in info; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_4_fails_missing_server_id() {
        let st = default_mock_state();
        {
            let mut info = st.info_json.write().unwrap();
            let obj = info.as_object_mut().unwrap();
            obj.remove("server_id");
            obj.insert(
                "device_id".to_string(),
                serde_json::Value::String("legacy-device-id".to_string()),
            );
        }
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(err.contains("server_id is required"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when server_id is missing"
        );
    }

    // 5. start_pairing fails if public_key does not derive to michi_id; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5_fails_derived_michi_id_mismatch() {
        let st = default_mock_state();
        // Give valid HiFi michi_id with standard public_key
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "michi_id".to_string(),
                serde_json::Value::String(
                    "lz4CalNVFwbIecx40oFy7Z1HCzkonqkdcBP_eG3FZjo".to_string(),
                ),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(
            err.contains("does not match derived public_key identity"),
            "got: {err}"
        );
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when derived identity mismatches"
        );
    }

    // 5b. start_pairing fails if api_version is wrong; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5b_fails_wrong_api_version() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "api_version".to_string(),
                serde_json::Value::String("v2-alpha".to_string()),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION: {err}"
        );
        assert!(err.contains("unsupported api_version"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called on unsupported api_version"
        );
    }

    // 5c. start_pairing fails if service is unknown; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5c_fails_unknown_service() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "service".to_string(),
                serde_json::Value::String("michi-stream-foo".to_string()),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION: {err}"
        );
        assert!(err.contains("unsupported receiver service"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called on unknown service"
        );
    }

    // 5d. start_pairing fails if audio_receiver role is missing; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5d_fails_missing_audio_receiver_role() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "roles".to_string(),
                serde_json::json!(["audio_transmitter"]),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION: {err}"
        );
        assert!(
            err.contains("roles must contain 'audio_receiver'"),
            "got: {err}"
        );
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when audio_receiver role is missing"
        );
    }

    // 5e. start_pairing fails if public_key is invalid base64 or length; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5e_fails_invalid_public_key_bytes() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "public_key".to_string(),
                serde_json::Value::String("not-valid-base64-or-length".to_string()),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION: {err}"
        );
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when public_key is invalid"
        );
    }

    // 5f. start_pairing fails if audio spec is missing; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5f_fails_missing_audio_spec() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("audio");
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION: {err}"
        );
        assert!(
            err.contains("missing canonical 'audio' specification"),
            "got: {err}"
        );
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when audio spec is missing"
        );
    }

    // 5g. start_pairing fails if rtp_udp transport is missing; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5g_fails_missing_rtp_udp_transport() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .get_mut("audio")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("transports".to_string(), serde_json::json!(["http_stream"]));
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION: {err}"
        );
        assert!(
            err.contains("does not support required 'rtp_udp'"),
            "got: {err}"
        );
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when rtp_udp transport is missing"
        );
    }

    // 5h. start_pairing fails if device has IdentityMismatch in local registry; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5h_fails_blocked_identity_mismatch() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        // Seed registry with conflicting entry
        let entry = ReceiverRegistryEntry {
            receiver_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            michi_id: Some("1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string()),
            name: "Tampered Device".to_string(),
            device_type: "standard".to_string(),
            base_url: base_url.clone(),
            qualification: ReceiverQualification::IdentityMismatch,
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("IDENTITY_MISMATCH"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when receiver has IdentityMismatch"
        );
    }

    // 5i. start_pairing fails if device is already paired; pair_start NOT called
    #[tokio::test]
    async fn test_pairing_5i_fails_already_paired() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        // Seed registry with already paired entry
        let entry = ReceiverRegistryEntry {
            receiver_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            michi_id: Some("1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string()),
            name: "Paired Device".to_string(),
            device_type: "standard".to_string(),
            base_url: base_url.clone(),
            paired: true,
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("ALREADY_PAIRED"), "got: {err}");
        assert!(
            !st.pair_start_called.load(Ordering::SeqCst),
            "remote pair_start must NOT be called when receiver is already paired"
        );
    }

    // 5j. start_pairing fails if server_id is not a valid UUID
    #[tokio::test]
    async fn test_pairing_5j_fails_server_id_not_uuid() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "server_id".to_string(),
                serde_json::Value::String("not-a-valid-uuid".to_string()),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(err.contains("valid canonical UUID"), "got: {err}");
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5k. start_pairing fails if identity_scheme is missing
    #[tokio::test]
    async fn test_pairing_5k_fails_missing_identity_scheme() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("identity_scheme");
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(err.contains("identity_scheme is required"), "got: {err}");
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5l. start_pairing fails if identity_scheme is unsupported
    #[tokio::test]
    async fn test_pairing_5l_fails_invalid_identity_scheme() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "identity_scheme".to_string(),
                serde_json::Value::String("rsa-sha256".to_string()),
            );
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(
            err.contains("identity_scheme must be 'ed25519-blake3-v1'"),
            "got: {err}"
        );
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5m. start_pairing fails if pcm_s16le codec is missing from audio.codecs
    #[tokio::test]
    async fn test_pairing_5m_fails_missing_codec_pcm_s16le() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .get_mut("audio")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("codecs".to_string(), serde_json::json!(["opus"]));
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(err.contains("required 'pcm_s16le' codec"), "got: {err}");
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5n. start_pairing fails if 48000 Hz is missing from audio.sample_rates
    #[tokio::test]
    async fn test_pairing_5n_fails_missing_sample_rate_48000() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .get_mut("audio")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("sample_rates".to_string(), serde_json::json!([44100]));
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(err.contains("required 48000 Hz sample rate"), "got: {err}");
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5o. start_pairing fails if 16-bit depth is missing from audio.bit_depths
    #[tokio::test]
    async fn test_pairing_5o_fails_missing_bit_depth_16() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .get_mut("audio")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("bit_depths".to_string(), serde_json::json!([24]));
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(err.contains("required 16-bit depth"), "got: {err}");
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5p. start_pairing fails if channel count 2 is missing from audio.channels
    #[tokio::test]
    async fn test_pairing_5p_fails_missing_channel_count_2() {
        let st = default_mock_state();
        st.info_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .get_mut("audio")
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("channels".to_string(), serde_json::json!([1]));
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("CONTRACT_VIOLATION"), "got: {err}");
        assert!(err.contains("required 2 channels (stereo)"), "got: {err}");
        assert!(!st.pair_start_called.load(Ordering::SeqCst));
    }

    // 5q. start_pairing_ext allows re-pairing an already paired receiver
    #[tokio::test]
    async fn test_pairing_5q_start_pairing_ext_allows_re_pair() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let entry = ReceiverRegistryEntry {
            receiver_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            michi_id: Some("1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string()),
            name: "Paired Device".to_string(),
            device_type: "standard".to_string(),
            base_url: base_url.clone(),
            paired: true,
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        let res = mgr.start_pairing_ext(&base_url, "initiator-1", true).await;
        assert!(
            res.is_ok(),
            "start_pairing_ext must succeed when allow_re_pair is true: {:?}",
            res.err()
        );
        assert!(st.pair_start_called.load(Ordering::SeqCst));
    }

    // 6. start_pairing fails if pair_start response missing server_michi_id
    #[tokio::test]
    async fn test_pairing_6_fails_missing_start_server_michi_id() {
        let st = default_mock_state();
        st.start_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("server_michi_id");
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(err.contains("server_michi_id is required"), "got: {err}");
    }

    // 7. start_pairing fails if pair_start response missing server_public_key
    #[tokio::test]
    async fn test_pairing_7_fails_missing_start_server_public_key() {
        let st = default_mock_state();
        st.start_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove("server_public_key");
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(err.contains("server_public_key is required"), "got: {err}");
    }

    // 8. start_pairing fails if pair_start server_michi_id != info.michi_id
    #[tokio::test]
    async fn test_pairing_8_fails_start_michi_id_mismatch() {
        let st = default_mock_state();
        st.start_json
            .write()
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(
                "server_michi_id".to_string(),
                serde_json::Value::String(
                    "lz4CalNVFwbIecx40oFy7Z1HCzkonqkdcBP_eG3FZjo".to_string(),
                ),
            );
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let mgr = make_test_session_manager();

        let res = mgr.start_pairing(&base_url, "initiator-1").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("CONTRACT_VIOLATION"),
            "expected CONTRACT_VIOLATION, got: {err}"
        );
        assert!(
            err.contains("does not match server/info michi_id"),
            "got: {err}"
        );
    }

    // 9. confirm_pairing aborts BEFORE pair_confirm if receiver identity changes between start and confirm
    #[tokio::test]
    async fn test_pairing_9_confirm_aborts_before_remote_call_on_identity_mismatch() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        // Mutate receiver identity before confirm
        {
            let mut info = st.info_json.write().unwrap();
            let obj = info.as_object_mut().unwrap();
            obj.insert(
                "michi_id".to_string(),
                serde_json::Value::String(
                    "lz4CalNVFwbIecx40oFy7Z1HCzkonqkdcBP_eG3FZjo".to_string(),
                ),
            );
            obj.insert(
                "public_key".to_string(),
                serde_json::Value::String(
                    "4CggHpvLXArVU2CJypgueD9MOtNfT9l1dfbCXrNdOts".to_string(),
                ),
            );
        }

        let res = mgr.confirm_pairing(&pending.pairing_id, "123456").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("IDENTITY_MISMATCH"),
            "expected IDENTITY_MISMATCH, got: {err}"
        );

        // CRITICAL: pair_confirm on remote receiver MUST NOT have been called!
        assert!(
            !st.pair_confirm_called.load(Ordering::SeqCst),
            "remote pair_confirm must not be called when identity mismatches"
        );
    }

    // 10. confirm_pairing post-confirmation fails closed if identity is invalid/mismatched
    #[tokio::test]
    async fn test_pairing_10_post_confirmation_fails_closed_on_identity_mismatch() {
        let st = default_mock_state();
        st.mutate_on_confirm.store(true, Ordering::SeqCst);
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager();

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        let res = mgr.confirm_pairing(&pending.pairing_id, "123456").await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(
            err.contains("IDENTITY_MISMATCH"),
            "expected IDENTITY_MISMATCH, got: {err}"
        );

        // Receiver must NOT be in registry
        let reg = mgr.registry.read().await;
        assert_eq!(
            reg.list().len(),
            0,
            "registry must remain empty on identity mismatch"
        );
    }

    // 11. confirm_pairing sets ProvisionalMdns when mDNS candidate in ScentStore
    #[tokio::test]
    async fn test_pairing_11_provisional_presence_when_mdns_candidate_in_scent() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let scent = Arc::new(michi_connect::ScentStore::new());
        scent.observe_mdns_candidate(
            michi_connect::scent_store::VerifiedServerInfo {
                michi_id: "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string(),
                device_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                name: "Test Stream".to_string(),
                service: "michi-stream-standard".to_string(),
                roles: vec!["audio_receiver".to_string()],
            },
            url::Url::parse(&base_url).unwrap(),
            None,
            std::time::Instant::now(),
        );
        let mgr = make_test_session_manager().with_scent_store(scent);

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();
        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "123456")
            .await
            .unwrap();

        let reg = mgr.registry.read().await;
        let entry = reg.get(&dev_id).expect("receiver must exist in registry");
        assert!(entry.paired, "receiver must be marked paired");
        assert_eq!(
            entry.presence,
            ReceiverPresence::ProvisionalMdns,
            "presence must be ProvisionalMdns when mDNS candidate exists in ScentStore"
        );
    }

    // 11b. confirm_pairing sets Offline when no record exists in ScentStore
    #[tokio::test]
    async fn test_pairing_11b_offline_presence_when_no_scent_record() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let scent = Arc::new(michi_connect::ScentStore::new());
        let mgr = make_test_session_manager().with_scent_store(scent);

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();
        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "123456")
            .await
            .unwrap();

        let reg = mgr.registry.read().await;
        let entry = reg.get(&dev_id).expect("receiver must exist in registry");
        assert!(entry.paired, "receiver must be marked paired");
        assert_eq!(
            entry.presence,
            ReceiverPresence::Offline,
            "presence must be Offline when no Scent record exists (pairing over HTTP does not prove mDNS)"
        );
    }

    // 12. confirm_pairing sets VerifiedOnline when fresh Whisker signed announcement exists in ScentStore
    #[tokio::test]
    async fn test_pairing_12_verified_presence_when_whisker_scent_fresh() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let scent = Arc::new(michi_connect::ScentStore::new());
        let now = std::time::Instant::now();
        scent.observe_signed(
            "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string(),
            "550e8400-e29b-41d4-a716-446655440000".to_string(),
            "Test Stream".to_string(),
            "michi-stream-standard".to_string(),
            vec!["audio_receiver".to_string()],
            Some("127.0.0.1:53318".parse().unwrap()),
            now,
        );

        let mgr = make_test_session_manager().with_scent_store(scent);
        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();
        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "123456")
            .await
            .unwrap();

        let reg = mgr.registry.read().await;
        let entry = reg.get(&dev_id).expect("receiver must exist in registry");
        assert!(entry.paired, "receiver must be marked paired");
        assert_eq!(
            entry.presence,
            ReceiverPresence::VerifiedOnline,
            "presence must be VerifiedOnline when fresh Whisker announcement exists"
        );
    }

    // 13. paired receiver truth lifecycle: provisional -> verified upgrade -> degraded to provisional -> offline
    #[tokio::test]
    async fn test_pairing_13_truth_lifecycle_upgrade_and_degradation() {
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st).await;
        let scent = Arc::new(michi_connect::ScentStore::new());
        scent.observe_mdns_candidate(
            michi_connect::scent_store::VerifiedServerInfo {
                michi_id: "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string(),
                device_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
                name: "Test Stream".to_string(),
                service: "michi-stream-standard".to_string(),
                roles: vec!["audio_receiver".to_string()],
            },
            url::Url::parse(&base_url).unwrap(),
            None,
            std::time::Instant::now(),
        );
        let mgr = make_test_session_manager().with_scent_store(scent.clone());
        let bridge =
            crate::discovery_bridge::ReceiverDiscoveryBridge::new(scent.clone(), mgr.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();
        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "123456")
            .await
            .unwrap();

        // 1. Initially provisional
        {
            let reg = mgr.registry.read().await;
            let entry = reg.get(&dev_id).unwrap();
            assert!(entry.paired);
            assert_eq!(entry.presence, ReceiverPresence::ProvisionalMdns);
        }

        // 2. Fresh signed Whisker announcement arrives: upgrades to VerifiedOnline without duplicate
        let signed_rec = michi_connect::ScentRecord {
            michi_id: "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string(),
            device_id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            name: "Test Stream".to_string(),
            service: "michi-stream-standard".to_string(),
            roles: vec!["audio_receiver".to_string()],
            verified: true,
            presence_source: michi_connect::scent_store::ScentPresenceSource::WhiskerSigned,
            endpoints: vec!["127.0.0.1:8080".parse().unwrap()],
            base_url: Some(url::Url::parse(&base_url).unwrap()),
            last_signed_seen: Some(std::time::Instant::now()),
            last_mdns_seen: None,
            server_info_verified_at: Some(std::time::Instant::now()),
            online: true,
        };
        bridge
            .handle_event(michi_connect::ScentEvent::Discovered(signed_rec.clone()))
            .await;

        {
            let reg = mgr.registry.read().await;
            assert_eq!(
                reg.list().len(),
                1,
                "must not create duplicate registry entry"
            );
            let entry = reg.get(&dev_id).unwrap();
            assert!(entry.paired);
            assert_eq!(entry.presence, ReceiverPresence::VerifiedOnline);
        }

        // 3. Whisker expires, but mDNS still fresh: degrades to ProvisionalMdns while remaining paired
        let mut degraded_rec = signed_rec.clone();
        degraded_rec.presence_source =
            michi_connect::scent_store::ScentPresenceSource::MdnsProvisional;
        degraded_rec.verified = false;
        bridge
            .handle_event(michi_connect::ScentEvent::Updated(degraded_rec))
            .await;

        {
            let reg = mgr.registry.read().await;
            let entry = reg.get(&dev_id).unwrap();
            assert!(entry.paired);
            assert_eq!(entry.presence, ReceiverPresence::ProvisionalMdns);
        }

        // 4. Both expire: degrades to Offline while remaining paired
        bridge
            .handle_event(michi_connect::ScentEvent::Offline {
                michi_id: "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".to_string(),
            })
            .await;

        {
            let reg = mgr.registry.read().await;
            let entry = reg.get(&dev_id).unwrap();
            assert!(entry.paired);
            assert_eq!(entry.presence, ReceiverPresence::Offline);
        }
    }
}
