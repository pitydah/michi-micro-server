use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::AppState;

fn v1_error_code(
    status: StatusCode,
    code: michi_link::MichiLinkErrorCode,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code.as_str(), "message": message, "details": {} }
        })),
    )
}

fn v1_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code, "message": message, "details": {} }
        })),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PresenceStateFields {
    pub presence: &'static str,
    pub online: bool,
    pub verified: bool,
    pub reachable: bool,
}

pub fn serialize_presence_state(
    presence: michi_receivers::ReceiverPresence,
) -> PresenceStateFields {
    match presence {
        michi_receivers::ReceiverPresence::VerifiedOnline => PresenceStateFields {
            presence: "verified_online",
            online: true,
            verified: true,
            reachable: true,
        },
        michi_receivers::ReceiverPresence::ProvisionalMdns => PresenceStateFields {
            presence: "provisional_mdns",
            online: false,
            verified: false,
            reachable: true,
        },
        michi_receivers::ReceiverPresence::Degraded => PresenceStateFields {
            presence: "degraded",
            online: false,
            verified: false,
            reachable: true,
        },
        michi_receivers::ReceiverPresence::Offline => PresenceStateFields {
            presence: "offline",
            online: false,
            verified: false,
            reachable: false,
        },
        michi_receivers::ReceiverPresence::Unknown => PresenceStateFields {
            presence: "unknown",
            online: false,
            verified: false,
            reachable: false,
        },
    }
}

pub fn serialize_effective_presence(
    effective: michi_connect::scent_store::EffectivePresence,
) -> PresenceStateFields {
    match effective {
        michi_connect::scent_store::EffectivePresence::VerifiedOnline => PresenceStateFields {
            presence: "verified_online",
            online: true,
            verified: true,
            reachable: true,
        },
        michi_connect::scent_store::EffectivePresence::ProvisionalMdns => PresenceStateFields {
            presence: "provisional_mdns",
            online: false,
            verified: false,
            reachable: true,
        },
        michi_connect::scent_store::EffectivePresence::Offline => PresenceStateFields {
            presence: "offline",
            online: false,
            verified: false,
            reachable: false,
        },
    }
}

pub fn compute_effective_receiver_presence(
    entry: &michi_receivers::ReceiverRegistryEntry,
    scent_store: &michi_connect::ScentStore,
) -> michi_receivers::ReceiverPresence {
    let opt_record = entry
        .michi_id
        .as_deref()
        .and_then(|mid| scent_store.get(mid))
        .or_else(|| scent_store.get(&entry.receiver_id));

    if let Some(record) = opt_record {
        match scent_store.effective_presence_now(&record) {
            michi_connect::scent_store::EffectivePresence::VerifiedOnline => {
                michi_receivers::ReceiverPresence::VerifiedOnline
            }
            michi_connect::scent_store::EffectivePresence::ProvisionalMdns => {
                michi_receivers::ReceiverPresence::ProvisionalMdns
            }
            michi_connect::scent_store::EffectivePresence::Offline => {
                michi_receivers::ReceiverPresence::Offline
            }
        }
    } else {
        entry.presence
    }
}

// ── Speaker group management (canonical alias to persistent Room Groups) ──────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeakerGroup {
    pub id: String,
    pub name: String,
    pub receiver_ids: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub async fn list_groups_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let groups = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let speaker_groups: Vec<_> = groups
        .into_iter()
        .map(
            |(gid, name, _mode, receiver_ids, _vols, created_at)| SpeakerGroup {
                id: gid.to_string(),
                name,
                receiver_ids,
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
                    .ok()
                    .map(|d| d.with_timezone(&chrono::Utc))
                    .unwrap_or_else(chrono::Utc::now),
            },
        )
        .collect();

    Ok(Json(serde_json::json!({ "groups": speaker_groups })))
}

#[derive(Debug, Deserialize)]
pub struct CreateGroupBody {
    pub name: String,
    pub receiver_ids: Vec<String>,
}

pub async fn create_group_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "group name is required",
        ));
    }

    let gid = Uuid::new_v4();
    let mut vols = HashMap::new();
    for rid in &body.receiver_ids {
        vols.insert(rid.clone(), 80);
    }

    michi_db::save_room_group_db(
        &state.db,
        &gid,
        &body.name,
        "custom",
        &body.receiver_ids,
        &vols,
    )
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let group = SpeakerGroup {
        id: gid.to_string(),
        name: body.name,
        receiver_ids: body.receiver_ids,
        created_at: chrono::Utc::now(),
    };
    Ok(Json(serde_json::json!({ "group": group })))
}

#[derive(Debug, Deserialize)]
pub struct SyncGroupBody {
    pub track_id: String,
    pub position_ms: u64,
    pub playing: bool,
}

pub async fn sync_group_handler(
    State(state): State<AppState>,
    Path(group_id): Path<String>,
    Json(body): Json<SyncGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let groups = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = groups
        .into_iter()
        .find(|(gid, name, _, _, _, _)| gid.to_string() == group_id || name == &group_id);

    let (gid, group_name, _mode, receiver_ids, _vols, _created_at) = found.ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "GROUP_NOT_FOUND",
            &format!("group {group_id} not found"),
        )
    })?;

    let track_uuid = Uuid::parse_str(&body.track_id).map_err(|_| {
        v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_TRACK_ID",
            "invalid track_id UUID format",
        )
    })?;

    let track = michi_db::get_track(&state.db, &track_uuid)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "TRACK_NOT_FOUND",
                &format!("track not found: {track_uuid}"),
            )
        })?;

    let selection = crate::output::PlaybackOutputSelection::RoomGroup { id: gid };
    let plan = crate::output::resolve_output(&selection, &state)
        .await
        .map_err(|e| v1_error(StatusCode::BAD_GATEWAY, e.error_code(), &e.to_string()))?;

    if body.playing {
        state
            .playback_engine
            .play(
                track,
                plan.sinks,
                plan.description.clone(),
                body.position_ms,
            )
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;
    } else {
        state.playback_engine.pause().await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                e.error_code(),
                &e.to_string(),
            )
        })?;
    }

    *state.playback_output_selection.write().await = Some(selection);

    Ok(Json(serde_json::json!({
        "status": "accepted",
        "lifecycle": if body.playing { "preparing" } else { "paused" },
        "group": group_name,
        "receivers": receiver_ids,
        "track_id": body.track_id,
        "position_ms": body.position_ms,
        "playing": body.playing,
        "output": plan.description,
    })))
}

// ── Existing receivers CRUD ─────────────────────────────────────

pub async fn receivers_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let receivers: Vec<serde_json::Value> = reg_read
        .list()
        .iter()
        .map(|e| {
            let eff_presence = compute_effective_receiver_presence(e, &state.scent_store);
            let p = serialize_presence_state(eff_presence);
            serde_json::json!({
                "id": e.receiver_id,
                "receiver_id": e.receiver_id,
                "michi_id": e.michi_id,
                "name": e.name,
                "device_type": e.device_type,
                "host": e.base_url,
                "base_url": e.base_url,
                "paired": e.paired,
                "online": p.online,
                "verified": p.verified,
                "reachable": p.reachable,
                "presence": p.presence,
                "authority_supported": e.authority_supported,
                "session_active": e.active_session_id.is_some(),
                "capabilities": e.capabilities,
                "qualification": e.compute_qualification(),
                "active_session_id": e.active_session_id,
                "last_seen": e.last_seen,
            })
        })
        .collect();
    Ok(Json(serde_json::json!({ "receivers": receivers })))
}

pub async fn get_receiver_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;
    let entry = reg_read.get(&id).ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("receiver not found: {id}"),
        )
    })?;
    let eff_presence = compute_effective_receiver_presence(entry, &state.scent_store);
    let p = serialize_presence_state(eff_presence);
    Ok(Json(serde_json::json!({
        "id": entry.receiver_id,
        "receiver_id": entry.receiver_id,
        "michi_id": entry.michi_id,
        "name": entry.name,
        "device_type": entry.device_type,
        "host": entry.base_url,
        "base_url": entry.base_url,
        "paired": entry.paired,
        "online": p.online,
        "verified": p.verified,
        "reachable": p.reachable,
        "presence": p.presence,
        "authority_supported": entry.authority_supported,
        "session_active": entry.active_session_id.is_some(),
        "capabilities": entry.capabilities,
        "qualification": entry.compute_qualification(),
        "max_sample_rate": entry.max_sample_rate,
        "max_bit_depth": entry.max_bit_depth,
        "supported_codecs": entry.supported_codecs,
        "active_session_id": entry.active_session_id,
        "last_seen": entry.last_seen,
    })))
}

/// DELETE /api/v1/receivers/:id
pub async fn delete_receiver_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // 1. Stop any active receiver session/supervisor
    let _ = state.receiver_manager.stop_session(&id).await;

    // 2. Remove from database
    let db_deleted = michi_db::delete_receiver_db(&state.db, &id)
        .await
        .unwrap_or(false);
    let _ = michi_db::delete_receiver_credential_db(&state.db, &id).await;

    // 3. Unpair in registry and remove entry
    let reg_arc = state.receiver_manager.registry().await;
    let reg_deleted = {
        let mut reg = reg_arc.write().await;
        let exists = reg.get(&id).is_some();
        if let Some(entry) = reg.get_mut(&id) {
            entry.paired = false;
            entry.token = None;
            entry.active_session_id = None;
        }
        reg.remove(&id);
        exists
    };

    if !db_deleted && !reg_deleted {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "RECEIVER_NOT_FOUND",
            &format!("receiver not found: {id}"),
        ));
    }

    Ok(Json(serde_json::json!({
        "status": "unpaired",
        "receiver_id": id,
    })))
}

/// POST /api/v1/receivers/:id/takeover
pub async fn receiver_takeover_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let reg_arc = state.receiver_manager.registry().await;
    let reg = reg_arc.read().await;
    let entry = reg
        .get(&id)
        .or_else(|| {
            reg.list()
                .into_iter()
                .find(|e| e.michi_id.as_deref() == Some(&id))
        })
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "RECEIVER_NOT_FOUND",
                &format!("receiver not found: {id}"),
            )
        })?
        .clone();
    drop(reg);

    if !entry.paired {
        return Err(v1_error(
            StatusCode::FORBIDDEN,
            "RECEIVER_NOT_PAIRED",
            "receiver must be paired to takeover authority",
        ));
    }

    match state
        .receiver_manager
        .authority_gate()
        .explicit_takeover(&entry)
        .await
    {
        Ok(grant) => Ok(Json(serde_json::json!({
            "status": "taken_over",
            "receiver_id": entry.receiver_id,
            "authority_instance_id": grant.authority_instance_id,
            "lease_epoch": grant.lease_epoch,
            "activation_expires_at": grant.activation_expires_at.to_rfc3339(),
        }))),
        Err(e) => Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "TAKEOVER_FAILED",
            &e.to_string(),
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReceiverPairStartBody {
    pub base_url: Option<String>,
    pub receiver_id: Option<String>,
    pub initiator_id: Option<String>,
    pub re_pair: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ReceiverPairConfirmBody {
    pub pairing_id: Option<String>,
    pub receiver_id: Option<String>,
    pub pin: String,
}

pub async fn receiver_pair_start_handler(
    State(state): State<AppState>,
    Json(body): Json<ReceiverPairStartBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let initiator_id = body
        .initiator_id
        .unwrap_or_else(|| "michi-micro-server".into());

    let target_base_url = if let Some(ref url) = body.base_url {
        url.clone()
    } else if let Some(ref rid) = body.receiver_id {
        let reg_arc = state.receiver_manager.registry().await;
        let reg = reg_arc.read().await;
        if let Some(entry) = reg.get(rid) {
            if entry.qualification
                == michi_receivers::models::ReceiverQualification::IdentityMismatch
            {
                return Err(v1_error(
                    StatusCode::BAD_REQUEST,
                    "IDENTITY_MISMATCH",
                    "receiver has an unresolved identity mismatch and cannot be paired",
                ));
            }
            if entry.paired && body.re_pair != Some(true) {
                return Err(v1_error(
                    StatusCode::BAD_REQUEST,
                    "ALREADY_PAIRED",
                    "receiver is already paired",
                ));
            }
            entry.base_url.clone()
        } else if let Some(scent_entry) = state.scent_store.get(rid).or_else(|| {
            state
                .scent_store
                .list()
                .into_iter()
                .find(|s| s.device_id == *rid)
        }) {
            let eff = state.scent_store.effective_presence_now(&scent_entry);
            if eff == michi_connect::scent_store::EffectivePresence::Offline {
                return Err(v1_error(
                    StatusCode::BAD_REQUEST,
                    "RECEIVER_OFFLINE",
                    &format!("receiver {rid} is offline or stale in scent store"),
                ));
            }
            if let Some(ref bu) = scent_entry.base_url {
                bu.to_string()
            } else {
                return Err(v1_error(
                    StatusCode::NOT_FOUND,
                    "RECEIVER_NOT_FOUND",
                    &format!("receiver not found or has no base_url: {rid}"),
                ));
            }
        } else {
            return Err(v1_error(
                StatusCode::NOT_FOUND,
                "RECEIVER_NOT_FOUND",
                &format!("receiver not found: {rid}"),
            ));
        }
    } else {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_REQUEST",
            "either base_url or receiver_id must be provided",
        ));
    };

    let allow_re_pair = body.re_pair.unwrap_or(false);
    match state
        .receiver_manager
        .start_pairing_ext(&target_base_url, &initiator_id, allow_re_pair)
        .await
    {
        Ok(pending) => {
            let _ = michi_db::record_pairing_journal_start_db(
                &state.db,
                &pending.pairing_id,
                &pending.expected_server_id,
                &pending.receiver_base_url,
                &pending.expected_michi_id,
                Some(&pending.receiver_pair_session_id),
            )
            .await;
            Ok(Json(serde_json::json!({
                "status": "pending_confirmation",
                "pairing_id": pending.pairing_id,
                "receiver_base_url": pending.receiver_base_url,
                "receiver_pair_session_id": pending.receiver_pair_session_id,
                "expires_at": pending.expires_at.to_rfc3339(),
            })))
        }
        Err(e) => {
            if e.contains("IDENTITY_MISMATCH") {
                Err(v1_error(StatusCode::BAD_REQUEST, "IDENTITY_MISMATCH", &e))
            } else if e.contains("ALREADY_PAIRED") {
                Err(v1_error(StatusCode::BAD_REQUEST, "ALREADY_PAIRED", &e))
            } else {
                Err(v1_error(StatusCode::BAD_REQUEST, "PAIR_START_FAILED", &e))
            }
        }
    }
}

async fn persist_paired_receiver(state: &AppState, device_id: &str) -> Result<(), String> {
    let reg_arc = state.receiver_manager.registry().await;
    let reg = reg_arc.read().await;
    let entry = reg
        .get(device_id)
        .ok_or_else(|| format!("receiver {device_id} not found in registry"))?;
    let now = chrono::Utc::now().to_rfc3339();
    let caps = entry.to_capabilities();
    let caps_json = serde_json::to_string(&caps).unwrap_or_else(|_| "{}".into());

    let token = entry
        .token
        .as_ref()
        .ok_or_else(|| "receiver token missing for paired receiver".to_string())?;

    let store = state
        .receiver_credential_store
        .as_ref()
        .as_ref()
        .ok_or_else(|| "CREDENTIAL_STORE_UNAVAILABLE: encryption key is unavailable".to_string())?;

    let (ciphertext, nonce) = store
        .encrypt_token(device_id, token)
        .map_err(|e| format!("failed to encrypt token for {device_id}: {e}"))?;

    let cred = michi_db::PersistedReceiverCredential {
        receiver_id: device_id.to_string(),
        ciphertext,
        nonce,
        version: 1,
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    let prec = michi_db::PersistedReceiver {
        id: device_id.to_string(),
        name: entry.name.clone(),
        device_type: entry.device_type.clone(),
        base_url: entry.base_url.clone(),
        paired: entry.paired,
        online: entry.presence == michi_receivers::ReceiverPresence::VerifiedOnline,
        audio_capabilities: caps_json.clone(),
        last_seen: entry.last_seen.map(|d| d.to_rfc3339()),
        paired_at: Some(now.clone()),
        created_at: now.clone(),
        updated_at: now,
        michi_id: entry.michi_id.clone(),
        capabilities_json: Some(caps_json),
        capabilities_observed_at: entry.capabilities_verified_at.map(|d| d.to_rfc3339()),
        authority_supported: entry.authority_supported,
    };

    // Retry database transaction up to 3 times with exponential backoff
    let mut last_err = None;
    for attempt in 1..=3 {
        match michi_db::persist_paired_receiver_transaction(&state.db, &prec, &cred).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(
                    "persist_paired_receiver attempt {attempt}/3 failed for {device_id}: {e}"
                );
                last_err = Some(e);
                tokio::time::sleep(std::time::Duration::from_millis(50 * (1 << attempt))).await;
            }
        }
    }

    Err(format!(
        "failed to persist receiver record and credential for {device_id} after 3 attempts: {last_err:?}"
    ))
}

pub async fn receiver_pair_confirm_handler(
    State(state): State<AppState>,
    Json(body): Json<ReceiverPairConfirmBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.receiver_credential_store.is_none() {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "CREDENTIAL_STORE_UNAVAILABLE",
            "Receiver credential store is unavailable",
        ));
    }

    let pin_trimmed = body.pin.trim();
    if pin_trimmed.len() != 6 || !pin_trimmed.chars().all(|c| c.is_ascii_digit()) {
        return Err(v1_error_code(
            StatusCode::BAD_REQUEST,
            michi_link::MichiLinkErrorCode::InvalidRequest,
            "PIN must be exactly 6 numeric digits",
        ));
    }

    let pairing_id = if let Some(ref pid) = body.pairing_id {
        pid.clone()
    } else {
        return Err(v1_error_code(
            StatusCode::BAD_REQUEST,
            michi_link::MichiLinkErrorCode::InvalidRequest,
            "pairing_id is required to confirm pairing",
        ));
    };

    match state
        .receiver_manager
        .confirm_pairing(&pairing_id, pin_trimmed)
        .await
    {
        Ok(device_id) => match persist_paired_receiver(&state, &device_id).await {
            Ok(()) => {
                let reg = state.receiver_manager.registry().await;
                let reg_read = reg.read().await;
                let (p, qualification) = if let Some(entry) = reg_read.get(&device_id) {
                    let eff_presence =
                        compute_effective_receiver_presence(entry, &state.scent_store);
                    (
                        serialize_presence_state(eff_presence),
                        entry.compute_qualification(),
                    )
                } else {
                    tracing::error!(
                        "PAIRING_REGISTRY_INVARIANT_VIOLATION: receiver {} missing from registry after confirmation and persistence; marking journal recovery required",
                        device_id
                    );
                    drop(reg_read);
                    let _ = michi_db::record_pairing_journal_recovery_required_db(
                        &state.db,
                        &pairing_id,
                        "Receiver entry missing from registry after confirmation and persistence",
                    )
                    .await;
                    return Err(v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "PAIRING_REGISTRY_INVARIANT_VIOLATION",
                        "Receiver registry invariant violation: entry unexpectedly absent after pairing confirmation. Remote pairing may have completed but local registry convergence failed.",
                    ));
                };
                let _ = michi_db::record_pairing_journal_completed_db(&state.db, &pairing_id).await;
                Ok(Json(serde_json::json!({
                    "status": "paired",
                    "device_id": device_id,
                    "receiver_id": device_id,
                    "paired": true,
                    "presence": p.presence,
                    "qualification": qualification,
                    "online": p.online,
                    "reachable": p.reachable,
                    "verified": p.verified,
                })))
            }
            Err(e) => {
                tracing::error!("pairing confirmed but persistence failed: {}; marking journal recovery required", e);
                let _ = michi_db::record_pairing_journal_recovery_required_db(
                    &state.db,
                    &pairing_id,
                    &format!("persistence failed: {e}"),
                )
                .await;
                Err(v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PERSISTENCE_FAILED",
                    &format!("pairing confirmed but persistence failed: {e}"),
                ))
            }
        },
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "PAIR_CONFIRM_FAILED", &e)),
    }
}

/// Reconciles unrecovered pairing journal entries (forward recovery after crash/restart)
pub async fn reconcile_unrecovered_pairings(state: &AppState) -> Result<usize, String> {
    let unrecovered = michi_db::list_unrecovered_pairing_journals_db(&state.db)
        .await
        .map_err(|e| format!("failed to list unrecovered pairing journals: {e}"))?;

    let mut recovered_count = 0;
    for journal in unrecovered {
        let receiver_id = &journal.receiver_id;
        let existing_db = michi_db::get_receiver_db(&state.db, receiver_id)
            .await
            .ok()
            .flatten();
        if existing_db.is_some() {
            let _ =
                michi_db::record_pairing_journal_completed_db(&state.db, &journal.pairing_id)
                    .await;
            continue;
        }

        let reg_arc = state.receiver_manager.registry().await;
        let reg = reg_arc.read().await;
        if reg.get(receiver_id).is_some() {
            drop(reg);
            if persist_paired_receiver(state, receiver_id).await.is_ok() {
                let _ = michi_db::record_pairing_journal_completed_db(
                    &state.db,
                    &journal.pairing_id,
                )
                .await;
                recovered_count += 1;
            }
            continue;
        }
        drop(reg);

        // Process unrecovered transactions (confirm_sent, remote_outcome_unknown, recovery_required)
        let Some(ref remote_session_id) = journal.remote_session_id else {
            tracing::warn!(
                "pairing journal {} has no remote_session_id; cannot query pair_status",
                journal.pairing_id
            );
            continue;
        };

        let mut client = michi_receivers::ReceiverClient::with_identity(
            &journal.base_url,
            state.identity.clone(),
        );

        // Check remote status of the pairing session
        let status_resp = match client.pair_status(remote_session_id).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    "failed to query pair_status for pairing {} (session {}): {}",
                    journal.pairing_id,
                    remote_session_id,
                    e
                );
                continue;
            }
        };

        if status_resp.status == "confirmed" {
            tracing::info!(
                "pairing journal {} (receiver {}) outcome is confirmed; initiating 2-step authenticated recovery",
                journal.pairing_id,
                receiver_id
            );

            // Execute authenticated recovery:
            // 1. POST /api/v1/pair/recover/start
            // 2. Validate server_michi_id and server_public_key (and BLAKE3 link) before signing challenge
            // 3. POST /api/v1/pair/recover with Ed25519 signature
            // 4. Strictly validate recover response
            let recover_resp = match client
                .pair_recover_auto(Some(&journal.michi_id), None)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(
                        "pairing recovery failed for {} (session {}): {}",
                        journal.pairing_id,
                        remote_session_id,
                        e
                    );
                    let _ = michi_db::record_pairing_journal_recovery_required_db(
                        &state.db,
                        &journal.pairing_id,
                        &format!("recovery failed: {e}"),
                    )
                    .await;
                    continue;
                }
            };

            // Verify the recovered token against an authenticated endpoint (GET /api/v1/receiver-lite/session)
            // Never consider a token valid just because /server/info works
            match client.verify_token().await {
                Ok(true) => {
                    tracing::info!(
                        "recovered token successfully authenticated against receiver {}",
                        receiver_id
                    );
                }
                Ok(false) => {
                    tracing::error!(
                        "recovered token was rejected by receiver {} on authenticated endpoint",
                        receiver_id
                    );
                    let _ = michi_db::record_pairing_journal_recovery_required_db(
                        &state.db,
                        &journal.pairing_id,
                        "recovered token rejected on authenticated endpoint",
                    )
                    .await;
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        "network error verifying token for receiver {}: {}",
                        receiver_id,
                        e
                    );
                    continue;
                }
            }

            let info = match client.get_info().await {
                Ok(i) => i,
                Err(e) => {
                    tracing::warn!(
                        "failed to fetch receiver info after recovery for {}: {}",
                        receiver_id,
                        e
                    );
                    continue;
                }
            };

            let device_id = recover_resp
                .device_id
                .clone()
                .or_else(|| info.michi_id.clone())
                .or_else(|| info.server_id.clone())
                .unwrap_or_else(|| receiver_id.clone());

            let token = match recover_resp.token.as_ref() {
                Some(t) => t,
                None => continue,
            };

            let store = match state.receiver_credential_store.as_ref().as_ref() {
                Some(s) => s,
                None => {
                    tracing::error!("receiver_credential_store not configured; cannot persist token");
                    continue;
                }
            };

            let (ciphertext, nonce) = match store.encrypt_token(&device_id, token) {
                Ok(pair) => pair,
                Err(e) => {
                    tracing::error!("failed to encrypt recovered token for {device_id}: {e}");
                    continue;
                }
            };

            let now = chrono::Utc::now().to_rfc3339();
            let cred = michi_db::PersistedReceiverCredential {
                receiver_id: device_id.clone(),
                ciphertext,
                nonce,
                version: 1,
                created_at: now.clone(),
                updated_at: now.clone(),
            };

            let name = info.name.clone().unwrap_or_else(|| device_id.clone());
            let device_type = info
                .device_type
                .clone()
                .or_else(|| info.service.clone())
                .unwrap_or_else(|| "michi-stream-standard".into());
            let audio = info.audio.as_ref();
            let caps_json = audio
                .map(|a| serde_json::to_string(a).unwrap_or_else(|_| "{}".into()))
                .unwrap_or_else(|| "{}".into());

            let prec = michi_db::PersistedReceiver {
                id: device_id.clone(),
                name: name.clone(),
                device_type: device_type.clone(),
                base_url: journal.base_url.clone(),
                paired: true,
                online: false,
                audio_capabilities: caps_json,
                last_seen: Some(now.clone()),
                paired_at: Some(now.clone()),
                created_at: now.clone(),
                updated_at: now,
                michi_id: info.michi_id.clone(),
                capabilities_json: None,
                capabilities_observed_at: None,
                authority_supported: false,
            };

            if let Err(e) =
                michi_db::persist_paired_receiver_transaction(&state.db, &prec, &cred).await
            {
                tracing::error!(
                    "failed to persist recovered receiver transaction for {device_id}: {e}"
                );
                continue;
            }

            // Insert into active registry
            let entry = michi_receivers::ReceiverRegistryEntry {
                receiver_id: device_id.clone(),
                michi_id: info.michi_id.clone(),
                name,
                device_type,
                base_url: journal.base_url.clone(),
                paired: true,
                token: Some(token.clone()),
                presence: michi_receivers::ReceiverPresence::Offline,
                last_seen: Some(chrono::Utc::now()),
                capabilities: vec!["pcm".to_string(), "rtp".to_string()],
                capabilities_verified_at: Some(chrono::Utc::now()),
                capabilities_stale: false,
                authority_supported: false,
                owner_michi_id: None,
                owner_name: None,
                active_session_id: None,
                max_sample_rate: 48000,
                max_bit_depth: 16,
                supported_transports: vec!["rtp_udp".into()],
                supported_codecs: vec!["pcm_s16le".into()],
                supported_sample_rates: vec![48000],
                supported_bit_depths: vec![16],
                supported_channels: vec![2],
                maximum_safe_volume: Some(100),
                qualification: michi_receivers::models::ReceiverQualification::Qualified,
            };
            state.receiver_manager.registry().await.write().await.add(entry);

            let _ = michi_db::record_pairing_journal_completed_db(&state.db, &journal.pairing_id)
                .await;
            recovered_count += 1;
        } else if status_resp.status == "expired"
            || status_resp.status == "not_found"
            || status_resp.status == "locked"
        {
            tracing::info!(
                "pairing journal {} (session {}) status is permanently {}; marking journal recovery required",
                journal.pairing_id,
                remote_session_id,
                status_resp.status
            );
            let _ = michi_db::record_pairing_journal_recovery_required_db(
                &state.db,
                &journal.pairing_id,
                &format!("terminal remote session status: {}", status_resp.status),
            )
            .await;
        }
    }

    Ok(recovered_count)
}

#[derive(Debug, Deserialize)]
pub struct DiscoverReceiverBody {
    pub base_url: String,
    pub initiator_id: Option<String>,
    pub pin: Option<String>,
}

pub async fn discover_receiver_handler(
    State(state): State<AppState>,
    Json(body): Json<DiscoverReceiverBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if state.receiver_credential_store.is_none() {
        return Err(v1_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "CREDENTIAL_STORE_UNAVAILABLE",
            "Receiver credential store is unavailable",
        ));
    }

    let initiator_id = body
        .initiator_id
        .unwrap_or_else(|| "michi-micro-server".into());
    let pin = match body.pin {
        Some(ref p) if p.trim().len() == 6 && p.trim().chars().all(|c| c.is_ascii_digit()) => {
            p.trim().to_string()
        }
        _ => {
            return Err(v1_error_code(
                StatusCode::BAD_REQUEST,
                michi_link::MichiLinkErrorCode::InvalidRequest,
                "PIN must be exactly 6 numeric digits",
            ));
        }
    };
    match state
        .receiver_manager
        .discover_and_pair(&body.base_url, &initiator_id, &pin)
        .await
    {
        Ok(device_id) => match persist_paired_receiver(&state, &device_id).await {
            Ok(()) => {
                let reg = state.receiver_manager.registry().await;
                let reg_read = reg.read().await;
                let (p, qualification) = if let Some(entry) = reg_read.get(&device_id) {
                    let eff_presence =
                        compute_effective_receiver_presence(entry, &state.scent_store);
                    (
                        serialize_presence_state(eff_presence),
                        entry.compute_qualification(),
                    )
                } else {
                    tracing::error!(
                        "PAIRING_REGISTRY_INVARIANT_VIOLATION: receiver {} missing from registry after discovery pairing and persistence; local registry convergence failed",
                        device_id
                    );
                    drop(reg_read);
                    return Err(v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "PAIRING_REGISTRY_INVARIANT_VIOLATION",
                        "Receiver registry invariant violation: entry unexpectedly absent after discovery pairing confirmation. Remote pairing may have completed but local registry convergence failed.",
                    ));
                };
                Ok(Json(serde_json::json!({
                    "status": "paired",
                    "device_id": device_id,
                    "receiver_id": device_id,
                    "paired": true,
                    "presence": p.presence,
                    "qualification": qualification,
                    "online": p.online,
                    "reachable": p.reachable,
                    "verified": p.verified,
                })))
            }
            Err(e) => {
                tracing::error!("discovery pairing confirmed but persistence failed: {}", e);
                Err(v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "PERSISTENCE_FAILED",
                    &format!("discovery pairing confirmed but persistence failed: {e}"),
                ))
            }
        },
        Err(e) => Err(v1_error(
            StatusCode::BAD_REQUEST,
            "DISCOVER_PAIR_FAILED",
            &e,
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReceiverSessionStartBody {
    pub session_id: String,
    pub codec: String,
    pub sample_rate: u32,
    pub bit_depth: u32,
    pub channels: u32,
    pub stream_port: u16,
    pub buffer_ms: u64,
    pub volume: u32,
}

pub async fn receiver_session_start_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReceiverSessionStartBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state
        .receiver_manager
        .start_session(
            &id,
            &body.session_id,
            &body.codec,
            body.sample_rate,
            body.bit_depth,
            body.channels,
            body.stream_port,
            body.buffer_ms,
            body.volume,
        )
        .await
    {
        Ok(resp) => {
            let rtp_local_port = state.receiver_manager.get_rtp_local_port(&id).await;
            Ok(Json(serde_json::json!({
                "status": "session_started",
                "session_id": resp.session_id,
                "stream_port": resp.stream_port,
                "buffer_ms": resp.buffer_ms,
                "ssrc": resp.ssrc,
                "transport": resp.transport,
                "codec": resp.codec,
                "sample_rate": resp.sample_rate,
                "bit_depth": resp.bit_depth,
                "channels": resp.channels,
                "rtp_local_port": rtp_local_port,
            })))
        }
        Err(e) => Err(v1_error(
            StatusCode::BAD_REQUEST,
            "SESSION_START_FAILED",
            &e,
        )),
    }
}

pub async fn receiver_session_stop_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.stop_session(&id).await {
        Ok(resp) => Ok(Json(
            serde_json::json!({ "status": resp.status, "session_id": resp.session_id }),
        )),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "SESSION_STOP_FAILED", &e)),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReceiverVolumeBody {
    pub volume: u32,
}

pub async fn receiver_volume_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReceiverVolumeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.set_volume(&id, body.volume).await {
        Ok(resp) => Ok(Json(
            serde_json::json!({ "status": "ok", "volume": resp.volume }),
        )),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "VOLUME_FAILED", &e)),
    }
}

pub async fn receiver_heartbeat_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    match state.receiver_manager.heartbeat(&id).await {
        Ok(resp) => Ok(Json(
            serde_json::json!({ "status": resp.status, "uptime_seconds": resp.uptime_seconds }),
        )),
        Err(e) => Err(v1_error(
            StatusCode::BAD_REQUEST,
            "HEARTBEAT_FAILED",
            &e.to_string(),
        )),
    }
}

#[cfg(any(feature = "hardware-gate", debug_assertions, test))]
#[derive(Debug, Deserialize)]
pub struct ReceiverTestPcmBody {
    pub pcm_base64: Option<String>,
    pub frequency_hz: Option<f32>,
    pub duration_ms: Option<usize>,
}

#[cfg(any(feature = "hardware-gate", debug_assertions, test))]
pub async fn receiver_stream_test_pcm_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReceiverTestPcmBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let pcm_bytes = if let Some(ref b64) = body.pcm_base64 {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| v1_error(StatusCode::BAD_REQUEST, "INVALID_BASE64", &e.to_string()))?
    } else {
        let freq = body.frequency_hz.unwrap_or(440.0);
        let ms = body.duration_ms.unwrap_or(20);
        let frames = (48000 * ms) / 1000;
        let mut bytes = Vec::with_capacity(frames * 4);
        for i in 0..frames {
            let t = (i as f32) / 48000.0;
            let sample_f = (2.0 * std::f32::consts::PI * freq * t).sin();
            let sample_i = (sample_f * 16384.0) as i16;
            let le = sample_i.to_le_bytes();
            // Stereo
            bytes.extend_from_slice(&le);
            bytes.extend_from_slice(&le);
        }
        bytes
    };

    match state.receiver_manager.write_pcm(&id, &pcm_bytes).await {
        Ok(bytes_sent) => Ok(Json(serde_json::json!({
            "status": "pcm_streamed",
            "bytes_sent": bytes_sent,
            "receiver_id": id,
        }))),
        Err(e) => Err(v1_error(StatusCode::BAD_REQUEST, "STREAM_PCM_FAILED", &e)),
    }
}

pub async fn discover_mdns_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let registry_arc = state.receiver_manager.registry().await;
    let reg = registry_arc.read().await;

    let mut seen_ids = std::collections::HashSet::new();
    let mut receivers = Vec::new();

    for entry in reg.receivers.values() {
        let stable_id = entry.michi_id.as_deref().unwrap_or(&entry.receiver_id);
        seen_ids.insert(stable_id.to_string());
        seen_ids.insert(entry.receiver_id.clone());

        let eff_presence = compute_effective_receiver_presence(entry, &state.scent_store);
        let p = serialize_presence_state(eff_presence);
        let is_valid_url = Url::parse(&entry.base_url).is_ok();
        let pairable = !entry.paired
            && p.reachable
            && is_valid_url
            && entry.qualification
                != michi_receivers::models::ReceiverQualification::IdentityMismatch;
        let host_or_addr = Url::parse(&entry.base_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| "127.0.0.1".to_string());

        receivers.push(serde_json::json!({
            "receiver_id": entry.receiver_id,
            "michi_id": entry.michi_id.clone().unwrap_or_else(|| entry.receiver_id.clone()),
            "name": entry.name,
            "service": if entry.device_type == "hifi" { "michi-stream-hifi" } else { "michi-stream-standard" },
            "device_type": entry.device_type,
            "base_url": entry.base_url,
            "host": entry.base_url,
            "addresses": vec![host_or_addr],
            "verified": p.verified,
            "online": p.online,
            "reachable": p.reachable,
            "paired": entry.paired,
            "presence": p.presence,
            "last_seen": entry.last_seen.map(|d| d.to_rfc3339()),
            "qualification": entry.compute_qualification(),
            "pairable": pairable,
        }));
    }

    for rec in state.scent_store.list_active() {
        if seen_ids.contains(&rec.michi_id) || seen_ids.contains(&rec.device_id) {
            continue;
        }
        let eff = state.scent_store.effective_presence_now(&rec);
        if eff == michi_connect::scent_store::EffectivePresence::Offline {
            continue;
        }
        let p = serialize_effective_presence(eff);

        let base_url_str = rec
            .base_url
            .as_ref()
            .map(|u| u.to_string())
            .unwrap_or_default();
        let host_or_addr = rec
            .endpoints
            .first()
            .map(|e| e.ip().to_string())
            .unwrap_or_else(|| "127.0.0.1".to_string());

        let is_valid_url = Url::parse(&base_url_str).is_ok();
        let pairable = p.reachable && is_valid_url;

        seen_ids.insert(rec.michi_id.clone());
        seen_ids.insert(rec.device_id.clone());
        receivers.push(serde_json::json!({
            "receiver_id": rec.michi_id.clone(),
            "michi_id": rec.michi_id.clone(),
            "name": rec.name.clone(),
            "service": rec.service.clone(),
            "device_type": if rec.service.contains("hifi") { "hifi" } else { "standard" },
            "base_url": base_url_str.clone(),
            "host": base_url_str,
            "addresses": vec![host_or_addr],
            "verified": p.verified,
            "online": p.online,
            "reachable": p.reachable,
            "paired": false,
            "presence": p.presence,
            "last_seen": if p.online { Some(chrono::Utc::now().to_rfc3339()) } else { None },
            "qualification": michi_receivers::models::ReceiverQualification::NeedsCapabilityRefresh,
            "pairable": pairable,
        }));
    }

    let joined = state
        .whisker_metrics
        .joined_interfaces
        .read()
        .map(|g| g.len())
        .unwrap_or(0);
    let whisker_listening = joined > 0;
    let active = whisker_listening;
    let degraded = !active;

    let all_scent = state.scent_store.list();
    let mut provisional_mdns_count = 0;
    let mut verified_stream_count = 0;
    for r in &all_scent {
        match state.scent_store.effective_presence_now(r) {
            michi_connect::scent_store::EffectivePresence::VerifiedOnline => {
                verified_stream_count += 1;
            }
            michi_connect::scent_store::EffectivePresence::ProvisionalMdns => {
                provisional_mdns_count += 1;
            }
            _ => {}
        }
    }

    Ok(Json(serde_json::json!({
        "receivers": receivers,
        "discovery": {
            "active": active,
            "degraded": degraded,
            "whisker_listening": whisker_listening,
            "interfaces_joined": joined,
            "provisional_mdns_count": provisional_mdns_count,
            "verified_stream_count": verified_stream_count,
        }
    })))
}

// ── Room Groups (Persistent) ─────────────────────────────────────

pub async fn list_room_groups_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let reg = state.receiver_manager.registry().await;
    let reg_read = reg.read().await;

    let mut groups = Vec::new();
    for (id, name, mode_str, receiver_ids, volumes, created_at_str) in rows {
        let mode = michi_core::RoomMode::from_config_str(&mode_str);
        let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());

        // Check if any receiver in this room has an active session
        let has_active_session = receiver_ids.iter().any(|rid| {
            reg_read
                .get(rid)
                .and_then(|e| e.active_session_id.as_ref())
                .is_some()
        });

        groups.push(michi_core::RoomGroup {
            id,
            name,
            mode,
            receiver_ids,
            volumes,
            active: has_active_session,
            chain_id: None,
            created_at,
        });
    }

    Ok(Json(serde_json::json!({ "groups": groups })))
}

#[derive(Debug, Deserialize)]
pub struct CreateRoomGroupBody {
    pub name: String,
    pub mode: Option<String>,
    pub receiver_ids: Vec<String>,
}

pub async fn create_room_group_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateRoomGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "name is required",
        ));
    }
    let mode = michi_core::RoomMode::from_config_str(body.mode.as_deref().unwrap_or("party"));
    let default_vol = match mode {
        michi_core::RoomMode::Party => 80,
        michi_core::RoomMode::Relax => 40,
        michi_core::RoomMode::Custom => 60,
    };
    let volumes: HashMap<String, u32> = body
        .receiver_ids
        .iter()
        .map(|id| (id.clone(), default_vol))
        .collect();

    let new_id = Uuid::new_v4();
    let mode_str = match mode {
        michi_core::RoomMode::Party => "party",
        michi_core::RoomMode::Relax => "relax",
        michi_core::RoomMode::Custom => "custom",
    };

    michi_db::save_room_group_db(
        &state.db,
        &new_id,
        body.name.trim(),
        mode_str,
        &body.receiver_ids,
        &volumes,
    )
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let group = michi_core::RoomGroup {
        id: new_id,
        name: body.name.trim().to_string(),
        mode,
        receiver_ids: body.receiver_ids,
        volumes,
        active: false,
        chain_id: None,
        created_at: chrono::Utc::now(),
    };
    Ok(Json(serde_json::json!({ "group": group })))
}

pub async fn get_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    match found {
        Some((gid, name, mode_str, receiver_ids, volumes, created_at_str)) => {
            let mode = michi_core::RoomMode::from_config_str(&mode_str);
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            let reg = state.receiver_manager.registry().await;
            let reg_read = reg.read().await;
            let active = receiver_ids.iter().any(|rid| {
                reg_read
                    .get(rid)
                    .and_then(|e| e.active_session_id.as_ref())
                    .is_some()
            });

            let group = michi_core::RoomGroup {
                id: gid,
                name,
                mode,
                receiver_ids,
                volumes,
                active,
                chain_id: None,
                created_at,
            };
            Ok(Json(serde_json::json!({ "group": group })))
        }
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        )),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoomGroupBody {
    pub name: Option<String>,
    pub mode: Option<String>,
    pub receiver_ids: Option<Vec<String>>,
}

pub async fn update_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRoomGroupBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    match found {
        Some((gid, mut name, mut mode_str, mut receiver_ids, mut volumes, _)) => {
            if let Some(n) = body.name {
                name = n;
            }
            if let Some(m) = body.mode {
                mode_str = m;
            }
            if let Some(rids) = body.receiver_ids {
                receiver_ids = rids;
                let default_vol = match mode_str.as_str() {
                    "party" => 80,
                    "relax" => 40,
                    _ => 60,
                };
                volumes = receiver_ids
                    .iter()
                    .map(|rid| (rid.clone(), default_vol))
                    .collect();
            }

            michi_db::save_room_group_db(
                &state.db,
                &gid,
                &name,
                &mode_str,
                &receiver_ids,
                &volumes,
            )
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_ERROR",
                    &e.to_string(),
                )
            })?;

            let group = michi_core::RoomGroup {
                id: gid,
                name,
                mode: michi_core::RoomMode::from_config_str(&mode_str),
                receiver_ids,
                volumes,
                active: false,
                chain_id: None,
                created_at: chrono::Utc::now(),
            };
            Ok(Json(serde_json::json!({ "group": group })))
        }
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        )),
    }
}

pub async fn delete_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_room_group_db(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    if !deleted {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        ));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn activate_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    let (_gid, name, mode_str, recv_ids, vols, _) =
        found.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found"))?;

    if recv_ids.is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ROOM",
            "cannot activate an empty room group with 0 receivers",
        ));
    }

    let mode = michi_core::RoomMode::from_config_str(&mode_str);
    let mut receiver_results = Vec::new();
    let mut success_count = 0usize;

    for recv_id in &recv_ids {
        let vol = vols.get(recv_id).copied().unwrap_or(match mode {
            michi_core::RoomMode::Party => 80,
            michi_core::RoomMode::Relax => 40,
            michi_core::RoomMode::Custom => 60,
        });

        let capped_vol = {
            let reg_a = state.receiver_manager.registry().await;
            let reg_read_a = reg_a.read().await;
            let max_safe = reg_read_a.get(recv_id).and_then(|e| e.maximum_safe_volume);
            max_safe.map(|max| vol.min(max)).unwrap_or(vol)
        };

        let entry_info = {
            let reg_b = state.receiver_manager.registry().await;
            let reg_read_b = reg_b.read().await;
            reg_read_b
                .get(recv_id)
                .map(|e| (e.paired, e.active_session_id.is_none()))
        };

        if let Some((paired, session_is_none)) = entry_info {
            if !paired {
                receiver_results.push(serde_json::json!({
                    "receiver_id": recv_id,
                    "status": "failed",
                    "error": { "code": "NOT_PAIRED", "message": "receiver is not paired" }
                }));
                continue;
            }

            let session_ok = if session_is_none {
                state
                    .receiver_manager
                    .start_session(
                        recv_id,
                        &id.to_string(),
                        "pcm_s16le",
                        48000,
                        16,
                        2,
                        0,
                        200,
                        capped_vol,
                    )
                    .await
                    .is_ok()
            } else {
                true
            };

            if session_ok {
                let _ = state.receiver_manager.set_volume(recv_id, capped_vol).await;
                success_count += 1;
                receiver_results.push(serde_json::json!({
                    "receiver_id": recv_id,
                    "status": "active",
                    "volume": capped_vol,
                }));
            } else {
                receiver_results.push(serde_json::json!({
                    "receiver_id": recv_id,
                    "status": "failed",
                    "error": { "code": "SESSION_FAILED", "message": "Failed to start receiver session" }
                }));
            }
        } else {
            receiver_results.push(serde_json::json!({
                "receiver_id": recv_id,
                "status": "failed",
                "error": { "code": "NOT_FOUND", "message": "receiver not found in registry" }
            }));
        }
    }

    if success_count == 0 {
        return Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "ROOM_ACTIVATION_FAILED",
            "failed to activate any receivers in room group",
        ));
    }

    let overall_status = if success_count == recv_ids.len() {
        "ready"
    } else {
        "partial_ready"
    };

    *state.playback_output_selection.write().await =
        Some(crate::output::PlaybackOutputSelection::RoomGroup { id });

    Ok(Json(serde_json::json!({
        "status": overall_status,
        "group_id": id,
        "group_name": name,
        "ready": true,
        "successful_receivers": success_count,
        "total_receivers": recv_ids.len(),
        "receivers": receiver_results,
    })))
}

pub async fn deactivate_room_group_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    let (_gid, _name, _mode_str, recv_ids, _, _) =
        found.ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "group not found"))?;

    let mut stopped_count = 0usize;
    let mut failed_count = 0usize;
    let mut per_link = Vec::new();

    for recv_id in &recv_ids {
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(recv_id) {
            if entry.active_session_id.is_some() {
                drop(reg_read);
                drop(reg);
                match state.receiver_manager.stop_session(recv_id).await {
                    Ok(_) => {
                        stopped_count += 1;
                        per_link.push(
                            serde_json::json!({ "receiver_id": recv_id, "status": "stopped" }),
                        );
                    }
                    Err(e) => {
                        failed_count += 1;
                        per_link.push(serde_json::json!({ "receiver_id": recv_id, "status": "failed", "error": e.to_string() }));
                    }
                }
            } else {
                stopped_count += 1;
                per_link.push(
                    serde_json::json!({ "receiver_id": recv_id, "status": "already_inactive" }),
                );
            }
        }
    }

    let status = if failed_count == 0 {
        "deactivated"
    } else if stopped_count > 0 {
        "partial"
    } else {
        "failed"
    };

    Ok(Json(serde_json::json!({
        "status": status,
        "group_id": id,
        "stopped_count": stopped_count,
        "failed_count": failed_count,
        "receivers": per_link,
    })))
}

#[derive(Debug, Deserialize)]
pub struct SetRoomModeBody {
    pub mode: String,
}

pub async fn set_room_mode_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<SetRoomModeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let new_mode = michi_core::RoomMode::from_config_str(&body.mode);
    let mode_str = match new_mode {
        michi_core::RoomMode::Party => "party",
        michi_core::RoomMode::Relax => "relax",
        michi_core::RoomMode::Custom => "custom",
    };

    let rows = michi_db::list_room_groups_db(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let found = rows.into_iter().find(|(gid, _, _, _, _, _)| *gid == id);
    match found {
        Some((gid, name, _, receiver_ids, mut volumes, _)) => {
            let default_vol = match new_mode {
                michi_core::RoomMode::Party => 80,
                michi_core::RoomMode::Relax => 40,
                michi_core::RoomMode::Custom => 60,
            };
            for vol in volumes.values_mut() {
                *vol = default_vol;
            }

            michi_db::save_room_group_db(&state.db, &gid, &name, mode_str, &receiver_ids, &volumes)
                .await
                .map_err(|e| {
                    v1_error(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "DATABASE_ERROR",
                        &e.to_string(),
                    )
                })?;

            let group = michi_core::RoomGroup {
                id: gid,
                name,
                mode: new_mode,
                receiver_ids,
                volumes,
                active: false,
                chain_id: None,
                created_at: chrono::Utc::now(),
            };
            Ok(Json(
                serde_json::json!({ "status": "mode_updated", "group": group }),
            ))
        }
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "group not found",
        )),
    }
}

pub async fn whisker_discovery_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let metrics = &state.whisker_metrics;
    let active_scent = state.scent_store.list_active();
    let all_scent = state.scent_store.list();
    let mut provisional_mdns_count = 0;
    let mut verified_stream_count = 0;
    let mut offline_stream_count = 0;

    for r in &all_scent {
        match state.scent_store.effective_presence_now(r) {
            michi_connect::scent_store::EffectivePresence::VerifiedOnline => {
                verified_stream_count += 1;
            }
            michi_connect::scent_store::EffectivePresence::ProvisionalMdns => {
                provisional_mdns_count += 1;
            }
            michi_connect::scent_store::EffectivePresence::Offline => {
                offline_stream_count += 1;
            }
        }
    }

    let joined = metrics.joined_interfaces.read().unwrap().clone();
    let interface_names: Vec<String> = joined.iter().map(|i| i.name.clone()).collect();
    let interface_ipv4: Vec<String> = joined.iter().map(|i| i.ip.to_string()).collect();

    let last_verified = metrics
        .last_verified_announce_at
        .load(std::sync::atomic::Ordering::Relaxed);
    let last_packet = metrics
        .last_packet_at
        .load(std::sync::atomic::Ordering::Relaxed);

    let scent_items: Vec<serde_json::Value> = active_scent
        .into_iter()
        .filter_map(|r| {
            let eff = state.scent_store.effective_presence_now(&r);
            if eff == michi_connect::scent_store::EffectivePresence::Offline {
                return None;
            }
            let p = serialize_effective_presence(eff);
            Some(serde_json::json!({
                "michi_id": r.michi_id,
                "device_id": r.device_id,
                "name": r.name,
                "service": r.service,
                "roles": r.roles,
                "verified": p.verified,
                "presence_source": match r.presence_source {
                    michi_connect::scent_store::ScentPresenceSource::WhiskerSigned => "whisker_signed",
                    michi_connect::scent_store::ScentPresenceSource::MdnsProvisional => "mdns_provisional",
                },
                "base_url": r.base_url.map(|u| u.to_string()),
                "online": p.online,
                "presence": p.presence,
            }))
        })
        .collect();
    Json(serde_json::json!({
        "status": "ok",
        "multicast_group": metrics.multicast_group.read().unwrap().clone(),
        "multicast_port": metrics.multicast_port.load(std::sync::atomic::Ordering::Relaxed),
        "interfaces_joined": joined.len(),
        "interface_names": interface_names,
        "interface_ipv4": interface_ipv4,
        "provisional_mdns_count": provisional_mdns_count,
        "verified_stream_count": verified_stream_count,
        "offline_stream_count": offline_stream_count,
        "last_verified_announce_at": if last_verified > 0 { Some(last_verified) } else { None },
        "last_packet_at": if last_packet > 0 { Some(last_packet) } else { None },
        "packets_received": metrics.packets_received.load(std::sync::atomic::Ordering::Relaxed),
        "announces_verified": metrics.announces_verified.load(std::sync::atomic::Ordering::Relaxed),
        "signature_rejected": metrics.signature_rejected.load(std::sync::atomic::Ordering::Relaxed),
        "timestamp_rejected": metrics.timestamp_rejected.load(std::sync::atomic::Ordering::Relaxed),
        "replay_rejected": metrics.replay_rejected.load(std::sync::atomic::Ordering::Relaxed),
        "non_stream_filtered": metrics.non_stream_filtered.load(std::sync::atomic::Ordering::Relaxed),
        "active_scent_count": scent_items.len(),
        "active_scent": scent_items,
    }))
}
