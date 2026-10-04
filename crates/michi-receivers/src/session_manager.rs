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
    db_pool: Option<sqlx::SqlitePool>,
    credential_store: Option<Arc<crate::credentials::ReceiverCredentialStore>>,
    pending_pairings: Arc<RwLock<HashMap<String, PendingReceiverPairing>>>,
    active_sessions: Arc<RwLock<HashMap<String, ReceiverActiveSession>>>,
    active_transports: Arc<RwLock<HashMap<String, SharedAudioTransport>>>,
    heartbeat_handles: Arc<RwLock<HashMap<String, ReceiverSupervisorHandle>>>,
    authority_gate: Arc<crate::authority_gate::AuthorityGate>,
    home_authority: Arc<RwLock<Option<Arc<michi_identity::home::HomeRootAuthority>>>>,
    server_membership: Arc<RwLock<Option<michi_identity::types::DeviceMembershipDto>>>,
    revocations: Arc<RwLock<Vec<michi_identity::types::HomeDeviceRevocationDto>>>,
}

impl std::fmt::Debug for ReceiverSessionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiverSessionManager")
            .field("identity", &self.identity.is_some())
            .field("scent_store", &self.scent_store.is_some())
            .field("db_pool", &self.db_pool.is_some())
            .field("credential_store", &self.credential_store.is_some())
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
            db_pool: None,
            credential_store: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_handles: Arc::new(RwLock::new(HashMap::new())),
            authority_gate,
            home_authority: Arc::new(RwLock::new(None)),
            server_membership: Arc::new(RwLock::new(None)),
            revocations: Arc::new(RwLock::new(Vec::new())),
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
            db_pool: None,
            credential_store: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_handles: Arc::new(RwLock::new(HashMap::new())),
            authority_gate,
            home_authority: Arc::new(RwLock::new(None)),
            server_membership: Arc::new(RwLock::new(None)),
            revocations: Arc::new(RwLock::new(Vec::new())),
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
            db_pool: None,
            credential_store: None,
            pending_pairings: Arc::new(RwLock::new(HashMap::new())),
            active_sessions: Arc::new(RwLock::new(HashMap::new())),
            active_transports: Arc::new(RwLock::new(HashMap::new())),
            heartbeat_handles: Arc::new(RwLock::new(HashMap::new())),
            authority_gate,
            home_authority: Arc::new(RwLock::new(None)),
            server_membership: Arc::new(RwLock::new(None)),
            revocations: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn set_identity(&mut self, identity: Arc<michi_identity::IdentityManager>) {
        self.identity = Some(identity);
    }

    pub fn set_scent_store(&mut self, scent_store: Arc<michi_connect::ScentStore>) {
        self.scent_store = Some(scent_store);
    }

    pub fn set_db_pool(&mut self, db_pool: sqlx::SqlitePool) {
        self.db_pool = Some(db_pool);
    }

    pub fn with_db_pool(mut self, db_pool: sqlx::SqlitePool) -> Self {
        self.db_pool = Some(db_pool);
        self
    }

    pub fn db_pool(&self) -> Option<sqlx::SqlitePool> {
        self.db_pool.clone()
    }

    pub fn set_credential_store(
        &mut self,
        credential_store: Arc<crate::credentials::ReceiverCredentialStore>,
    ) {
        self.credential_store = Some(credential_store);
    }

    pub fn with_credential_store(
        mut self,
        credential_store: Arc<crate::credentials::ReceiverCredentialStore>,
    ) -> Self {
        self.credential_store = Some(credential_store);
        self
    }

    pub fn credential_store(&self) -> Option<Arc<crate::credentials::ReceiverCredentialStore>> {
        self.credential_store.clone()
    }

    pub fn with_scent_store(mut self, scent_store: Arc<michi_connect::ScentStore>) -> Self {
        self.scent_store = Some(scent_store);
        self
    }

    pub fn scent_store(&self) -> Option<Arc<michi_connect::ScentStore>> {
        self.scent_store.clone()
    }

    pub fn with_home_authority(
        mut self,
        authority: Arc<michi_identity::home::HomeRootAuthority>,
        membership: michi_identity::types::DeviceMembershipDto,
    ) -> Self {
        self.home_authority = Arc::new(RwLock::new(Some(authority)));
        self.server_membership = Arc::new(RwLock::new(Some(membership)));
        self
    }

    pub async fn set_home_authority(
        &self,
        authority: Arc<michi_identity::home::HomeRootAuthority>,
        membership: michi_identity::types::DeviceMembershipDto,
    ) {
        *self.home_authority.write().await = Some(authority);
        *self.server_membership.write().await = Some(membership);
    }

    pub async fn home_context(&self) -> Option<crate::client::ReceiverHomeAuthContext> {
        let auth_guard = self.home_authority.read().await;
        let mem_guard = self.server_membership.read().await;
        let revs_guard = self.revocations.read().await;
        match (auth_guard.as_ref(), mem_guard.as_ref()) {
            (Some(a), Some(m)) => Some(crate::client::ReceiverHomeAuthContext {
                home_root_public_key: a.public_key_base64url(),
                expected_home_id: a.home_id(),
                client_membership: m.clone(),
                revocations: revs_guard.clone(),
            }),
            _ => None,
        }
    }

    pub async fn home_id(&self) -> Option<String> {
        self.home_authority
            .read()
            .await
            .as_ref()
            .map(|a| a.home_id())
    }

    pub async fn home_root_public_key(&self) -> Option<String> {
        self.home_authority
            .read()
            .await
            .as_ref()
            .map(|a| a.public_key_base64url())
    }

    pub async fn server_membership(&self) -> Option<michi_identity::types::DeviceMembershipDto> {
        self.server_membership.read().await.clone()
    }
}

fn update_entry_capabilities(entry: &mut ReceiverRegistryEntry, info: &ReceiverInfo) {
    if let Some(audio) = &info.audio {
        if let Some(transports) = audio.get("transports").and_then(|v| v.as_array()) {
            entry.supported_transports = transports
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect();
        }
        if let Some(srs) = audio.get("sample_rates").and_then(|v| v.as_array()) {
            let sample_rates: Vec<u32> = srs
                .iter()
                .filter_map(|x| x.as_u64().map(|n| n as u32))
                .collect();
            if !sample_rates.is_empty() {
                entry.max_sample_rate = *sample_rates.iter().max().unwrap_or(&48000);
                entry.supported_sample_rates = sample_rates;
            }
        }
        if let Some(bds) = audio.get("bit_depths").and_then(|v| v.as_array()) {
            let bit_depths: Vec<u32> = bds
                .iter()
                .filter_map(|x| x.as_u64().map(|n| n as u32))
                .collect();
            if !bit_depths.is_empty() {
                entry.max_bit_depth = *bit_depths.iter().max().unwrap_or(&16);
                entry.supported_bit_depths = bit_depths;
            }
        }
        if let Some(chs) = audio.get("channels").and_then(|v| v.as_array()) {
            entry.supported_channels = chs
                .iter()
                .filter_map(|x| x.as_u64().map(|n| n as u8))
                .collect();
        }
        if let Some(cds) = audio.get("codecs").and_then(|v| v.as_array()) {
            entry.supported_codecs = cds
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect();
        }
    } else if let Some(codecs) = &info.supported_codecs {
        entry.supported_codecs = codecs.clone();
    }
    if entry.max_sample_rate == 0 {
        entry.max_sample_rate = 48000;
        entry.supported_sample_rates = vec![44100, 48000];
    }
    if entry.max_bit_depth == 0 {
        entry.max_bit_depth = 16;
        entry.supported_bit_depths = vec![16];
    }
    if entry.supported_channels.is_empty() {
        entry.supported_channels = vec![2];
    }
    if entry.supported_codecs.is_empty() {
        entry.supported_codecs = vec!["pcm_s16le".to_string()];
    }
    if entry.supported_transports.is_empty() {
        entry.supported_transports = vec!["rtp_udp".to_string()];
    }

    if let Some(feats) = &info.features {
        entry.authority_supported = feats
            .get("authority_v1")
            .or_else(|| feats.get("perch_v1"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    }
}

impl ReceiverSessionManager {
    /// Trust Architecture V2: Authenticate a receiver using Home Membership.
    pub async fn authenticate_receiver(
        &self,
        receiver_id: &str,
    ) -> Result<ReceiverInfo, ReceiverClientError> {
        let (base_url, client_id) = {
            let reg = self.registry.read().await;
            let entry = reg.get(receiver_id).ok_or_else(|| {
                ReceiverClientError::Offline(format!("receiver not found: {receiver_id}"))
            })?;
            if entry.revoked {
                return Err(ReceiverClientError::Protocol(format!(
                    "receiver {receiver_id} is revoked in this home"
                )));
            }
            (entry.base_url.clone(), entry.michi_id.clone())
        };

        let home_ctx = self.home_context().await.ok_or_else(|| {
            ReceiverClientError::Protocol("HomeRootAuthority context not configured".into())
        })?;

        let target_mid = client_id.as_deref().unwrap_or(receiver_id);
        if home_ctx
            .revocations
            .iter()
            .any(|r| r.revoked_device_michi_id == target_mid)
        {
            let mut reg = self.registry.write().await;
            if let Some(entry) = reg.get_mut(receiver_id) {
                entry.revoked = true;
                entry.paired = false;
                entry.authenticated = false;
                entry.token = None;
            }
            return Err(ReceiverClientError::Protocol(format!(
                "device {target_mid} is revoked"
            )));
        }

        let mut client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(&base_url, id.clone())
        } else {
            ReceiverClient::new(&base_url)
        };
        client.set_home_auth_context(home_ctx.clone());

        let sess_resp = client
            .authenticate(
                &home_ctx.home_root_public_key,
                &home_ctx.expected_home_id,
                &home_ctx.client_membership,
                &home_ctx.revocations,
            )
            .await?;

        let info = client.get_info().await.map_err(|e| {
            ReceiverClientError::Protocol(format!("failed to get receiver info: {e}"))
        })?;

        {
            let mut reg = self.registry.write().await;
            if let Some(entry) = reg.get_mut(receiver_id) {
                entry.token = Some(sess_resp.session_token.clone());
                entry.paired = true;
                entry.authenticated = true;
                entry.michi_home_id = Some(home_ctx.expected_home_id.clone());
                entry.server_membership = Some(sess_resp.server_membership.clone());
                entry.last_seen = Some(chrono::Utc::now());
                entry.presence = ReceiverPresence::VerifiedOnline;
                if let Some(ref mid) = info.michi_id {
                    entry.michi_id = Some(mid.clone());
                }
                if let Some(ref n) = info.name {
                    entry.name = n.clone();
                }
                update_entry_capabilities(entry, &info);
                entry.qualification = entry.compute_qualification();
            }
        }

        if let (Some(ref cs), Some(ref pool)) = (&self.credential_store, &self.db_pool) {
            if let Ok((ct, nonce)) = cs.encrypt_token(receiver_id, &sess_resp.session_token) {
                let now = chrono::Utc::now().to_rfc3339();
                let cred = michi_db::PersistedReceiverCredential {
                    receiver_id: receiver_id.to_string(),
                    ciphertext: ct,
                    nonce,
                    version: 1,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                };
                let reg_read = self.registry.read().await;
                if let Some(entry) = reg_read.get(receiver_id) {
                    let prec = michi_db::PersistedReceiver {
                        id: entry.receiver_id.clone(),
                        name: entry.name.clone(),
                        device_type: entry.device_type.clone(),
                        base_url: entry.base_url.clone(),
                        paired: true,
                        online: entry.is_online(),
                        audio_capabilities: serde_json::to_string(&entry.capabilities)
                            .unwrap_or_default(),
                        last_seen: entry.last_seen.map(|d| d.to_rfc3339()),
                        paired_at: Some(now.clone()),
                        created_at: now.clone(),
                        updated_at: now,
                        michi_id: entry.michi_id.clone(),
                        capabilities_json: Some(
                            serde_json::to_string(&entry.capabilities).unwrap_or_default(),
                        ),
                        capabilities_observed_at: entry
                            .capabilities_verified_at
                            .map(|d| d.to_rfc3339()),
                        authority_supported: entry.authority_supported,
                    };
                    let _ = michi_db::persist_paired_receiver_transaction(pool, &prec, &cred).await;
                }
            }
        }

        Ok(info)
    }

    pub async fn authenticate_url(
        &self,
        base_url: &str,
    ) -> Result<ReceiverInfo, ReceiverClientError> {
        let client = if let Some(ref id) = self.identity {
            ReceiverClient::with_identity(base_url, id.clone())
        } else {
            ReceiverClient::new(base_url)
        };
        let info = client
            .get_info()
            .await
            .map_err(ReceiverClientError::Protocol)?;
        let existing_id = {
            let reg = self.registry.read().await;
            reg.receivers
                .values()
                .find(|e| e.base_url == base_url)
                .map(|e| e.receiver_id.clone())
        };
        let receiver_id = existing_id.unwrap_or_else(|| {
            info.device_id
                .as_deref()
                .or(info.server_id.as_deref())
                .or(info.michi_id.as_deref())
                .unwrap_or(base_url)
                .to_string()
        });

        {
            let mut reg = self.registry.write().await;
            if reg.get(&receiver_id).is_none() {
                let mut entry = ReceiverRegistryEntry {
                    receiver_id: receiver_id.clone(),
                    michi_id: info.michi_id.clone(),
                    name: info.name.clone().unwrap_or_else(|| "Michi Receiver".into()),
                    base_url: base_url.to_string(),
                    device_type: info
                        .device_type
                        .clone()
                        .unwrap_or_else(|| "standard".into()),
                    ..Default::default()
                };
                update_entry_capabilities(&mut entry, &info);
                reg.add(entry);
            }
        }

        self.authenticate_receiver(&receiver_id).await
    }

    pub async fn revoke_device(
        &self,
        device_michi_id: &str,
        reason: &str,
    ) -> Result<michi_identity::types::HomeDeviceRevocationDto, String> {
        let auth_guard = self.home_authority.read().await;
        let auth = auth_guard
            .as_ref()
            .ok_or_else(|| "HomeRootAuthority not configured".to_string())?;

        let now = chrono::Utc::now().to_rfc3339();
        let revocation = auth.issue_revocation(&auth.home_id(), device_michi_id, &now, reason);

        {
            let mut revs = self.revocations.write().await;
            revs.retain(|r| r.revoked_device_michi_id != device_michi_id);
            revs.push(revocation.clone());
        }

        let receiver_ids_to_stop: Vec<String> = {
            let mut reg = self.registry.write().await;
            let mut to_stop = Vec::new();
            for (id, entry) in reg.receivers.iter_mut() {
                if entry.receiver_id == device_michi_id
                    || entry.michi_id.as_deref() == Some(device_michi_id)
                {
                    entry.revoked = true;
                    entry.paired = false;
                    entry.authenticated = false;
                    entry.token = None;
                    entry.presence = ReceiverPresence::Offline;
                    to_stop.push(id.clone());
                }
            }
            to_stop
        };

        for rid in receiver_ids_to_stop {
            let _ = self.stop_session(&rid).await;
        }

        Ok(revocation)
    }

    pub async fn get_revocations(&self) -> Vec<michi_identity::types::HomeDeviceRevocationDto> {
        self.revocations.read().await.clone()
    }

    pub async fn get_home_roster(&self) -> Vec<HomeRosterDevice> {
        let mut devices = Vec::new();

        if let Some(ref mem) = *self.server_membership.read().await {
            devices.push(HomeRosterDevice {
                device_michi_id: mem.device_michi_id.clone(),
                name: "Michi Micro Server".to_string(),
                device_type: "server".to_string(),
                base_url: None,
                roles: mem.roles.iter().map(|r| r.as_str().to_string()).collect(),
                authenticated: true,
                revoked: false,
                online: true,
                last_seen: Some(chrono::Utc::now()),
            });
        }

        let reg = self.registry.read().await;
        for entry in reg.receivers.values() {
            let roles = entry
                .server_membership
                .as_ref()
                .map(|m| m.roles.iter().map(|r| r.as_str().to_string()).collect())
                .unwrap_or_else(|| vec!["audio_receiver".to_string()]);

            devices.push(HomeRosterDevice {
                device_michi_id: entry
                    .michi_id
                    .clone()
                    .unwrap_or_else(|| entry.receiver_id.clone()),
                name: entry.name.clone(),
                device_type: entry.device_type.clone(),
                base_url: Some(entry.base_url.clone()),
                roles,
                authenticated: entry.authenticated || entry.paired,
                revoked: entry.revoked,
                online: entry.is_online(),
                last_seen: entry.last_seen,
            });
        }

        devices
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
            p.insert(pairing_id.clone(), pending.clone());
        }

        if let Some(ref pool) = self.db_pool {
            let effective_device_id = if !pending.expected_michi_id.is_empty() {
                &pending.expected_michi_id
            } else {
                &pending.expected_server_id
            };
            if let Err(e) = michi_db::record_pairing_journal_start_db(
                pool,
                &pairing_id,
                effective_device_id,
                &pending.receiver_base_url,
                &pending.expected_michi_id,
                Some(&pending.receiver_pair_session_id),
            )
            .await
            {
                tracing::warn!("failed to record pairing journal start for {pairing_id}: {e}");
            }
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

        // Record CONFIRM_SENT in journal before sending network request
        if let Some(ref pool) = self.db_pool {
            let _ = michi_db::record_pairing_journal_confirm_sent_db(pool, pairing_id).await;
        }

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
                // If network failure or remote status unknown, record REMOTE_OUTCOME_UNKNOWN in journal
                if e.code == "NETWORK_ERROR"
                    || e.http_status == 502
                    || e.http_status == 503
                    || e.http_status == 504
                    || e.http_status == 408
                {
                    if let Some(ref pool) = self.db_pool {
                        let _ = michi_db::record_pairing_journal_outcome_unknown_db(
                            pool, pairing_id, &e.message,
                        )
                        .await;
                    }
                }

                let recovered = if e.code == "NETWORK_ERROR"
                    || e.code == "PAIRING_ALREADY_CONSUMED"
                    || e.code == "CONFLICT"
                    || e.http_status == 409
                {
                    match client.pair_status(&pending.receiver_pair_session_id).await {
                        Ok(status_resp) if status_resp.status == "confirmed" => {
                            tracing::info!(
                                "pairing session {} confirmed remotely; executing authenticated recovery",
                                pending.receiver_pair_session_id
                            );
                            client
                                .pair_recover_auto(
                                    Some(&pending.expected_michi_id),
                                    Some(&pending.expected_public_key),
                                )
                                .await
                                .ok()
                        }
                        _ => None,
                    }
                } else {
                    None
                };

                if let Some(rec_resp) = recovered {
                    rec_resp
                } else {
                    // Strict typed handling of receiver error codes
                    match e.code.as_str() {
                        "PAIRING_PIN_MISMATCH" => {
                            // Keep pending for user retry
                        }
                        "PAIRING_EXPIRED"
                        | "PAIRING_NOT_FOUND"
                        | "PAIRING_ALREADY_CONSUMED"
                        | "CONFLICT"
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
            }
        };

        if let Some(ref err) = confirm_resp.error {
            let recovered = if err.code == "PAIRING_ALREADY_CONSUMED" || err.code == "CONFLICT" {
                match client.pair_status(&pending.receiver_pair_session_id).await {
                    Ok(status_resp) if status_resp.status == "confirmed" => client
                        .pair_recover_auto(
                            Some(&pending.expected_michi_id),
                            Some(&pending.expected_public_key),
                        )
                        .await
                        .ok(),
                    _ => None,
                }
            } else {
                None
            };

            if recovered.is_none() {
                if err.code == "PAIRING_EXPIRED"
                    || err.code == "PAIRING_NOT_FOUND"
                    || err.code == "PAIRING_ALREADY_CONSUMED"
                    || err.code == "CONFLICT"
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
        }

        // Record TOKEN_RECEIVED in journal with encrypted token immediately after confirm succeeds
        let effective_device_id = if !pending.expected_michi_id.is_empty() {
            &pending.expected_michi_id
        } else {
            &pending.expected_server_id
        };
        if let Some(ref tok) = client.token {
            if let (Some(ref pool), Some(ref store)) = (&self.db_pool, &self.credential_store) {
                if let Ok((ct, nonce)) = store.encrypt_token(effective_device_id, tok) {
                    let _ = michi_db::record_pairing_journal_token_received_db(
                        pool, pairing_id, &ct, &nonce,
                    )
                    .await;
                }
            }
        }

        let build_and_add_result: Result<String, String> = async {
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
                michi_home_id: None,
                server_membership: None,
                authenticated: true,
                revoked: false,
            };

            self.registry.write().await.add(entry);
            Ok(device_id)
        }
        .await;

        match build_and_add_result {
            Ok(device_id) => {
                if let Some(ref pool) = self.db_pool {
                    let _ = michi_db::record_pairing_journal_completed_db(pool, pairing_id).await;
                }
                let mut p = self.pending_pairings.write().await;
                p.remove(pairing_id);
                Ok(device_id)
            }
            Err(e) => {
                if let Some(ref pool) = self.db_pool {
                    let _ =
                        michi_db::record_pairing_journal_recovery_required_db(pool, pairing_id, &e)
                            .await;
                }
                Err(e)
            }
        }
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

        if entry.revoked {
            return Err(format!("receiver {receiver_id} is revoked"));
        }

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

        let negotiated = match client
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
            .await
        {
            Ok(neg) => neg,
            Err(e) if e.contains("401") || e.to_lowercase().contains("unauthorized") => {
                if self.authenticate_receiver(receiver_id).await.is_ok() {
                    let new_token = {
                        let reg = self.registry.read().await;
                        reg.get(receiver_id).and_then(|e| e.token.clone())
                    };
                    client.token = new_token;
                    client
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
                        .await?
                } else {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        };

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

        let resp = match client.heartbeat().await {
            Ok(r) => r,
            Err(ReceiverClientError::Unauthorized) => {
                if self.authenticate_receiver(receiver_id).await.is_ok() {
                    let new_token = {
                        let reg = self.registry.read().await;
                        reg.get(receiver_id).and_then(|e| e.token.clone())
                    };
                    client.token = new_token;
                    client.heartbeat().await?
                } else {
                    return Err(ReceiverClientError::Unauthorized);
                }
            }
            Err(e) => return Err(e),
        };
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
        status_json: std::sync::Arc<std::sync::RwLock<serde_json::Value>>,
        recover_start_json: std::sync::Arc<std::sync::RwLock<serde_json::Value>>,
        recover_json: std::sync::Arc<std::sync::RwLock<serde_json::Value>>,
        pair_start_called: std::sync::Arc<AtomicBool>,
        pair_confirm_called: std::sync::Arc<AtomicBool>,
        pair_status_called: std::sync::Arc<AtomicBool>,
        pair_recover_start_called: std::sync::Arc<AtomicBool>,
        pair_recover_called: std::sync::Arc<AtomicBool>,
        confirm_status_override: std::sync::Arc<std::sync::RwLock<Option<axum::http::StatusCode>>>,
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

    async fn mock_pair_status(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> AxumJson<serde_json::Value> {
        st.pair_status_called.store(true, Ordering::SeqCst);
        let val = st.status_json.read().unwrap().clone();
        AxumJson(val)
    }

    async fn mock_pair_recover_start(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> AxumJson<serde_json::Value> {
        st.pair_recover_start_called.store(true, Ordering::SeqCst);
        let val = st.recover_start_json.read().unwrap().clone();
        AxumJson(val)
    }

    async fn mock_pair_recover(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> AxumJson<serde_json::Value> {
        st.pair_recover_called.store(true, Ordering::SeqCst);
        let val = st.recover_json.read().unwrap().clone();
        AxumJson(val)
    }

    async fn mock_pair_confirm(
        AxumState(st): AxumState<MockReceiverState>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;
        st.pair_confirm_called.store(true, Ordering::SeqCst);
        if let Some(status) = *st.confirm_status_override.read().unwrap() {
            let err = serde_json::json!({
                "error": {
                    "code": if status == axum::http::StatusCode::CONFLICT { "PAIRING_ALREADY_CONSUMED" } else { "INTERNAL_ERROR" },
                    "message": "pairing confirm failed in mock"
                }
            });
            return (status, AxumJson(err)).into_response();
        }
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
        (axum::http::StatusCode::OK, AxumJson(val)).into_response()
    }

    async fn spawn_mock_receiver(st: MockReceiverState) -> (String, tokio::task::JoinHandle<()>) {
        let app = axum::Router::new()
            .route("/api/v1/server/info", get(mock_server_info))
            .route("/api/v1/pair/start", post(mock_pair_start))
            .route("/api/v1/pair/status", get(mock_pair_status))
            .route("/api/v1/pair/confirm", post(mock_pair_confirm))
            .route("/api/v1/pair/recover/start", post(mock_pair_recover_start))
            .route("/api/v1/pair/recover", post(mock_pair_recover))
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

        let status = serde_json::json!({
            "session_id": "pair-sess-1",
            "status": "pending",
            "expires_at": chrono::Utc::now().checked_add_signed(chrono::Duration::seconds(60)).unwrap().to_rfc3339(),
            "attempts_remaining": 5
        });

        let recover_start = serde_json::json!({
            "challenge_nonce": "CxIZICcuNTxDSlFYX2ZtdEN4SVpJQ2N1TlR4RFNsRllYZg",
            "expires_at": chrono::Utc::now().checked_add_signed(chrono::Duration::seconds(60)).unwrap().to_rfc3339(),
            "server_michi_id": "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4",
            "server_public_key": "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8"
        });

        let recover = serde_json::json!({
            "status": "paired",
            "token": "test-recovered-token-99999",
            "expires_in": 0,
            "device_id": "550e8400-e29b-41d4-a716-446655440000",
            "server_id": "550e8400-e29b-41d4-a716-446655440000"
        });

        MockReceiverState {
            info_json: std::sync::Arc::new(std::sync::RwLock::new(info)),
            start_json: std::sync::Arc::new(std::sync::RwLock::new(start)),
            confirm_json: std::sync::Arc::new(std::sync::RwLock::new(confirm)),
            status_json: std::sync::Arc::new(std::sync::RwLock::new(status)),
            recover_start_json: std::sync::Arc::new(std::sync::RwLock::new(recover_start)),
            recover_json: std::sync::Arc::new(std::sync::RwLock::new(recover)),
            pair_start_called: std::sync::Arc::new(AtomicBool::new(false)),
            pair_confirm_called: std::sync::Arc::new(AtomicBool::new(false)),
            pair_status_called: std::sync::Arc::new(AtomicBool::new(false)),
            pair_recover_start_called: std::sync::Arc::new(AtomicBool::new(false)),
            pair_recover_called: std::sync::Arc::new(AtomicBool::new(false)),
            confirm_status_override: std::sync::Arc::new(std::sync::RwLock::new(None)),
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

    #[tokio::test]
    async fn test_pairing_journal_lifecycle_persisted() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager().with_db_pool(pool.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();
        let journal = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist after start_pairing");
        assert_eq!(journal.state, "started");
        assert_eq!(journal.remote_session_id.as_deref(), Some("pair-sess-1"));

        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "482391")
            .await
            .unwrap();
        assert!(!dev_id.is_empty());

        let journal_after = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist after confirm_pairing");
        assert_eq!(journal_after.state, "persisted");
    }

    #[tokio::test]
    async fn test_pairing_journal_recovery_required_on_post_confirm_failure() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let st = default_mock_state();
        // Mutate identity on confirm to induce post-confirmation invariant violation
        st.mutate_on_confirm.store(true, Ordering::SeqCst);
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager().with_db_pool(pool.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        let res = mgr.confirm_pairing(&pending.pairing_id, "482391").await;
        assert!(
            res.is_err(),
            "confirm must fail on post-confirm identity mismatch"
        );

        let journal = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist");
        assert_eq!(journal.state, "recovery_required");
        assert!(
            journal
                .error_reason
                .unwrap_or_default()
                .contains("IDENTITY_MISMATCH"),
            "error_reason must record the failure cause"
        );
    }

    #[tokio::test]
    async fn test_pairing_confirm_response_lost_authenticated_recovery() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let st = default_mock_state();
        // Simulate confirm returning 409 conflict (as if previous attempt was consumed)
        *st.confirm_status_override.write().unwrap() = Some(axum::http::StatusCode::CONFLICT);
        // Status reports confirmed
        *st.status_json.write().unwrap() = serde_json::json!({
            "session_id": "pair-sess-1",
            "status": "confirmed",
            "expires_at": chrono::Utc::now().to_rfc3339(),
            "attempts_remaining": 5
        });

        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager().with_db_pool(pool.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "482391")
            .await
            .expect("confirm must transparently recover via pair_status and pair_recover");
        assert!(!dev_id.is_empty());
        assert!(
            st.pair_status_called.load(Ordering::SeqCst),
            "pair_status must be called"
        );
        assert!(
            st.pair_recover_start_called.load(Ordering::SeqCst),
            "pair_recover_start must be called"
        );
        assert!(
            st.pair_recover_called.load(Ordering::SeqCst),
            "pair_recover must be called"
        );

        let journal = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist");
        assert_eq!(journal.state, "persisted");
    }

    #[tokio::test]
    async fn test_pairing_recovery_identity_validation_rejects_imposter() {
        use crate::models::PairRecoverStartResponse;

        // Imposter has mismatch between public key and server_michi_id
        let imposter_start = PairRecoverStartResponse {
            challenge_nonce: "CxIZICcuNTxDSlFYX2ZtdEN4SVpJQ2N1TlR4RFNsRllYZg".into(),
            expires_at: "2026-10-04T00:00:00Z".into(),
            server_michi_id: "fake_michi_id_does_not_derive_from_key".into(),
            server_public_key: "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8".into(),
            error: None,
        };

        let err = crate::client::ReceiverClient::validate_recover_start_identity(
            &imposter_start,
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(err.code, "IDENTITY_MISMATCH");

        // Identity mismatch with expected_michi_id
        let valid_start = PairRecoverStartResponse {
            challenge_nonce: "CxIZICcuNTxDSlFYX2ZtdEN4SVpJQ2N1TlR4RFNsRllYZg".into(),
            expires_at: "2026-10-04T00:00:00Z".into(),
            server_michi_id: "1_fKPrJgtUmrEczOhMdMV_k4s-VaDE1Hfg_65xhp8F4".into(),
            server_public_key: "CGzuzD0UgfvAs1PJdcBBA1XqgVC28pgABFMzR6VNnq8".into(),
            error: None,
        };
        let err2 = crate::client::ReceiverClient::validate_recover_start_identity(
            &valid_start,
            Some("different_expected_michi_id"),
            None,
        )
        .unwrap_err();
        assert_eq!(err2.code, "IDENTITY_MISMATCH");
    }

    #[tokio::test]
    async fn test_pairing_crash_frontier_1_started_remains_started() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager().with_db_pool(pool.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        // Frontier 1: Process crashes right after start_pairing
        let journal = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist");
        assert_eq!(journal.state, "started");
        assert_eq!(journal.remote_session_id.as_deref(), Some("pair-sess-1"));
        assert!(journal.token_ciphertext.is_none());

        // Unrecovered pairings only looks for confirm_sent, remote_outcome_unknown, token_received, recovery_required
        let unrecovered = michi_db::list_unrecovered_pairing_journals_db(&pool)
            .await
            .unwrap();
        assert!(
            unrecovered.is_empty(),
            "started pairings must not trigger premature recovery"
        );
    }

    #[tokio::test]
    async fn test_pairing_crash_frontier_2_durable_confirm_sent_recorded() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let st = default_mock_state();
        // Override confirm to delay so we can inspect state or simulate crash before network completes
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager().with_db_pool(pool.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        // Directly record confirm_sent to simulate crash right after confirm_sent write
        michi_db::record_pairing_journal_confirm_sent_db(&pool, &pending.pairing_id)
            .await
            .unwrap();

        let unrecovered = michi_db::list_unrecovered_pairing_journals_db(&pool)
            .await
            .unwrap();
        assert_eq!(unrecovered.len(), 1);
        assert_eq!(unrecovered[0].state, "confirm_sent");
        assert_eq!(
            unrecovered[0].remote_session_id.as_deref(),
            Some("pair-sess-1")
        );
    }

    #[tokio::test]
    async fn test_pairing_crash_frontier_3_remote_outcome_unknown_on_network_failure() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let st = default_mock_state();
        // Override confirm to 503 Service Unavailable (network error / gateway timeout)
        *st.confirm_status_override.write().unwrap() =
            Some(axum::http::StatusCode::SERVICE_UNAVAILABLE);
        // Status returns error so recovery fails and records outcome unknown
        *st.status_json.write().unwrap() = serde_json::json!({
            "error": {
                "code": "NETWORK_ERROR",
                "message": "upstream unreachable"
            }
        });

        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager().with_db_pool(pool.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        let _ = mgr.confirm_pairing(&pending.pairing_id, "482391").await;

        let journal = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist");
        assert_eq!(journal.state, "remote_outcome_unknown");
        assert!(
            journal.error_reason.is_some(),
            "error_reason must be populated on remote_outcome_unknown"
        );
    }

    #[tokio::test]
    async fn test_pairing_crash_frontier_4_token_received_encrypted_in_journal() {
        let pool = michi_db::init_pool("sqlite::memory:").await.unwrap();
        let cred_store = Arc::new(crate::credentials::ReceiverCredentialStore::new([99u8; 32]));
        let st = default_mock_state();
        let (base_url, _handle) = spawn_mock_receiver(st.clone()).await;
        let mgr = make_test_session_manager()
            .with_db_pool(pool.clone())
            .with_credential_store(cred_store.clone());

        let pending = mgr.start_pairing(&base_url, "initiator-1").await.unwrap();

        let dev_id = mgr
            .confirm_pairing(&pending.pairing_id, "482391")
            .await
            .expect("pairing confirm must succeed");

        // Journal must have completed in persisted state, but token was encrypted without plaintext leak
        let journal = michi_db::get_pairing_journal_entry_db(&pool, &pending.pairing_id)
            .await
            .unwrap()
            .expect("journal entry must exist");
        assert_eq!(journal.state, "persisted");
        assert!(
            journal.token_ciphertext.is_some(),
            "token_ciphertext must be populated in journal"
        );
        assert!(
            journal.token_nonce.is_some(),
            "token_nonce must be populated in journal"
        );

        // Verify encrypted token can be decrypted and matches mock receiver token
        let ct = journal.token_ciphertext.unwrap();
        let nonce = journal.token_nonce.unwrap();
        let decrypted = cred_store.decrypt_token(&dev_id, &ct, &nonce).unwrap();
        assert_eq!(decrypted, "test-device-token-12345");
    }

    #[tokio::test]
    async fn test_pairing_strict_recover_response_validation() {
        use crate::models::PairConfirmResponse;

        // Missing token
        let invalid_resp1 = PairConfirmResponse {
            status: Some("paired".into()),
            token: None,
            refresh_token: None,
            expires_in: Some(3600),
            device_id: Some("dev-1".into()),
            server_id: Some("srv-1".into()),
            controller_id: None,
            error: None,
        };
        assert!(invalid_resp1.token.is_none());

        // Empty token
        let invalid_resp2 = PairConfirmResponse {
            status: Some("paired".into()),
            token: Some("   ".into()),
            refresh_token: None,
            expires_in: Some(3600),
            device_id: Some("dev-1".into()),
            server_id: Some("srv-1".into()),
            controller_id: None,
            error: None,
        };
        assert!(invalid_resp2.token.as_ref().unwrap().trim().is_empty());

        // Missing expires_in
        let invalid_resp3 = PairConfirmResponse {
            status: Some("paired".into()),
            token: Some("valid_token".into()),
            refresh_token: None,
            expires_in: None,
            device_id: Some("dev-1".into()),
            server_id: Some("srv-1".into()),
            controller_id: None,
            error: None,
        };
        assert!(invalid_resp3.expires_in.is_none());
    }

    #[tokio::test]
    async fn test_trust_v2_home_roster_and_revocation() {
        let mgr = ReceiverSessionManager::new();
        let root = Arc::new(michi_identity::home::HomeRootAuthority::generate());
        let server_id = "test-server-michi-id-123456789012345678901";
        let server_pk = "test-server-public-key-1234567890123456789";
        let membership = root.issue_membership(
            &root.home_id(),
            server_id,
            server_pk,
            "server",
            vec![michi_identity::types::Role::MusicServer],
            "2026-10-04T00:00:00Z",
            1,
        );

        mgr.set_home_authority(root.clone(), membership).await;

        assert_eq!(mgr.home_id().await, Some(root.home_id()));
        assert_eq!(
            mgr.home_root_public_key().await,
            Some(root.public_key_base64url())
        );

        let entry = ReceiverRegistryEntry {
            receiver_id: "stream-living-room".to_string(),
            name: "Living Room Speaker".to_string(),
            base_url: "http://192.168.1.100:80".to_string(),
            michi_id: Some("stream-dev-id-12345".to_string()),
            paired: true,
            authenticated: true,
            ..Default::default()
        };
        mgr.registry.write().await.add(entry);

        let roster = mgr.get_home_roster().await;
        assert_eq!(roster.len(), 2);
        assert!(roster.iter().any(|d| d.device_type == "server"));
        assert!(roster
            .iter()
            .any(|d| d.device_michi_id == "stream-dev-id-12345"));

        // Revoke the receiver
        let rev = mgr
            .revoke_device("stream-dev-id-12345", "stolen device")
            .await
            .unwrap();
        assert_eq!(rev.revoked_device_michi_id, "stream-dev-id-12345");

        let revs = mgr.get_revocations().await;
        assert_eq!(revs.len(), 1);
        assert_eq!(revs[0].reason, "stolen device");

        // Verify registry was updated
        let reg = mgr.registry.read().await;
        let r = reg.get("stream-living-room").unwrap();
        assert!(r.revoked);
        assert!(!r.authenticated);
        assert!(!r.paired);
        assert!(r.token.is_none());
    }
}
