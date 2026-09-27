use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::time::Instant;
use uuid::Uuid;

use crate::output::{resolve_output, PlaybackOutputSelection};
use crate::playback_queue::get_or_create_active_queue;
use crate::AppState;
use michi_sync::content_identity::{ContentIdentityResolver, ContentResolutionError};
use michi_sync::playback_transfer::{
    sign_target_ready_proof, PendingPlaybackTransfer, TailSyncV1, DEFAULT_TRANSFER_TTL,
};

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

#[derive(Debug, Deserialize, Serialize)]
pub struct PlaybackTransferPrepareRequest {
    pub transfer_id: String,
    pub source_michi_id: String,
    pub target_michi_id: String,
    pub receiver_michi_id: String,
    pub tailsync: TailSyncV1,
    #[serde(default)]
    pub nonce: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaybackTransferPrepareResponse {
    pub transfer_id: String,
    pub status: String,
    pub target_michi_id: String,
    pub target_ready_proof: String,
    pub resolved_current_track_id: Option<String>,
    pub resolved_queue_count: usize,
    pub expires_in_seconds: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlaybackTransferCommitRequest {
    pub transfer_id: String,
    pub grant: michi_receivers::authority_models::AuthorityGrant,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaybackTransferCommitResponse {
    pub transfer_id: String,
    pub status: String,
    pub playback_session_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlaybackTransferAbortRequest {
    pub transfer_id: String,
    #[serde(default)]
    pub reason: Option<String>,
}

/// POST /api/v1/playback-transfer/prepare
pub async fn playback_transfer_prepare_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackTransferPrepareRequest>,
) -> Result<Json<PlaybackTransferPrepareResponse>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::FORBIDDEN,
            "MODULE_DISABLED",
            "playback module is currently disabled",
        ));
    }

    // 1. Validate TailSync bounds
    body.tailsync
        .validate_bounds()
        .map_err(|e| v1_error(StatusCode::BAD_REQUEST, "INVALID_TAILSYNC", &e.to_string()))?;

    // 2. Target validation: target must be this server
    let our_michi_id = state.identity.michi_id().to_string();
    if !body.target_michi_id.is_empty() && body.target_michi_id != our_michi_id {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "TARGET_MISMATCH",
            &format!(
                "target_michi_id '{}' does not match this server '{}'",
                body.target_michi_id, our_michi_id
            ),
        ));
    }

    // 3. Receiver existence & paired verification
    let reg_arc = state.receiver_manager.registry().await;
    let reg = reg_arc.read().await;
    let receiver = reg
        .list()
        .into_iter()
        .find(|r| {
            r.receiver_id == body.receiver_michi_id
                || r.michi_id.as_deref() == Some(&body.receiver_michi_id)
        })
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "RECEIVER_NOT_FOUND",
                &format!(
                    "receiver '{}' not found in registry",
                    body.receiver_michi_id
                ),
            )
        })?
        .clone();

    if !receiver.paired {
        return Err(v1_error(
            StatusCode::FORBIDDEN,
            "RECEIVER_NOT_PAIRED",
            "receiver must be paired before accepting playback transfer",
        ));
    }

    if receiver.presence == michi_receivers::models::ReceiverPresence::Offline {
        return Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "RECEIVER_OFFLINE",
            "receiver is currently offline",
        ));
    }
    drop(reg);

    // 4. Content identity resolution for current track
    let resolver = ContentIdentityResolver::new(state.db.clone());
    let current_track = resolver
        .resolve(&body.tailsync.current_track)
        .await
        .map_err(|e| match e {
            ContentResolutionError::NotFound { title, artist } => v1_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "CURRENT_TRACK_NOT_FOUND",
                &format!("current track '{title}' by '{artist}' not found in local library"),
            ),
            ContentResolutionError::AmbiguousMatch { title, count } => v1_error(
                StatusCode::UNPROCESSABLE_ENTITY,
                "CURRENT_TRACK_AMBIGUOUS",
                &format!("ambiguous matches ({count}) for current track '{title}'"),
            ),
            ContentResolutionError::Database(err) => {
                v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &err)
            }
        })?;

    // Verify media file is openable / accessible on disk
    if let Err(e) = tokio::fs::metadata(&current_track.file_path).await {
        return Err(v1_error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "MEDIA_FILE_INACCESSIBLE",
            &format!(
                "media file at '{}' is inaccessible: {e}",
                current_track.file_path
            ),
        ));
    }

    // Resolve queue tracks (best-effort resolution)
    let mut prepared_queue = Vec::with_capacity(body.tailsync.queue.len());
    for qref in &body.tailsync.queue {
        if let Ok(track) = resolver.resolve(qref).await {
            prepared_queue.push(track);
        }
    }

    // 5. Generate Target Ready Proof
    let nonce = match &body.nonce {
        Some(n) if !n.trim().is_empty() && n.trim() != "0" => n.trim().to_string(),
        _ => {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_NONCE",
                "nonce must be provided and non-trivial (cannot be empty or '0')",
            ));
        }
    };

    let expected_epoch = if receiver.authority_supported {
        if let Ok(endpoint) = reqwest::Url::parse(&receiver.base_url) {
            let auth_client = michi_receivers::authority_client::ReceiverAuthorityClient::new();
            if let Ok(st) = auth_client
                .state(&endpoint, receiver.token.as_deref())
                .await
            {
                st.lease_epoch + 1
            } else if let Some(g) = state
                .receiver_manager
                .authority_gate()
                .get_grant(&receiver.receiver_id)
                .await
            {
                g.lease_epoch + 1
            } else {
                1
            }
        } else {
            1
        }
    } else {
        1
    };

    let ready_proof = sign_target_ready_proof(
        &state.identity,
        &body.transfer_id,
        &body.source_michi_id,
        &our_michi_id,
        &body.receiver_michi_id,
        expected_epoch,
        &nonce,
    );

    // 6. Store Pending Playback Transfer in RAM
    let resolved_count = prepared_queue.len();
    let current_track_id_str = current_track.id.to_string();

    let pending = PendingPlaybackTransfer {
        transfer_id: body.transfer_id.clone(),
        source_michi_id: body.source_michi_id.clone(),
        target_michi_id: our_michi_id.clone(),
        receiver_michi_id: receiver.receiver_id.clone(),
        tailsync: body.tailsync,
        prepared_current_track: current_track,
        prepared_queue,
        created_at: chrono::Utc::now(),
        expires_at: Instant::now() + DEFAULT_TRANSFER_TTL,
    };

    state.pending_transfers.insert(pending).await;

    Ok(Json(PlaybackTransferPrepareResponse {
        transfer_id: body.transfer_id,
        status: "ready".to_string(),
        target_michi_id: our_michi_id,
        target_ready_proof: ready_proof,
        resolved_current_track_id: Some(current_track_id_str),
        resolved_queue_count: resolved_count,
        expires_in_seconds: DEFAULT_TRANSFER_TTL.as_secs(),
    }))
}

/// POST /api/v1/playback-transfer/commit
pub async fn playback_transfer_commit_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackTransferCommitRequest>,
) -> Result<Json<PlaybackTransferCommitResponse>, (StatusCode, Json<serde_json::Value>)> {
    if state.disabled_modules.read().await.contains("playback") {
        return Err(v1_error(
            StatusCode::FORBIDDEN,
            "MODULE_DISABLED",
            "playback module is currently disabled",
        ));
    }

    // 1. Load pending transfer from RAM
    let pending = state
        .pending_transfers
        .get(&body.transfer_id)
        .await
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "TRANSFER_NOT_FOUND_OR_EXPIRED",
                &format!("transfer '{}' expired or not found", body.transfer_id),
            )
        })?;

    // 2. Validate grant token
    if body.grant.grant_token.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_GRANT",
            "grant_token cannot be empty",
        ));
    }

    // 3. Set output selection to the target receiver
    let receiver_id = pending.receiver_michi_id.clone();
    let selection = PlaybackOutputSelection::Receiver {
        id: receiver_id.clone(),
    };
    *state.playback_output_selection.write().await = Some(selection.clone());

    // 4. Start receiver session with the authority grant
    let session_id = Uuid::new_v4().to_string();
    state
        .receiver_manager
        .start_session_with_authority(
            &receiver_id,
            &session_id,
            "pcm_s16le",
            48000,
            16,
            2,
            0,
            200,
            80,
            Some(&body.grant),
        )
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::BAD_GATEWAY,
                "RECEIVER_SESSION_FAILED",
                &format!("failed to start receiver session with authority grant: {e}"),
            )
        })?;

    // 5. Install queue in database transaction
    let queue_id = get_or_create_active_queue(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    let mut tx = state.db.begin().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    sqlx::query("DELETE FROM queue_items WHERE queue_id = ?")
        .bind(queue_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let now = chrono::Utc::now().to_rfc3339();
    let queue_tracks = if pending.prepared_queue.is_empty() {
        vec![pending.prepared_current_track.clone()]
    } else {
        pending.prepared_queue.clone()
    };

    for (i, track) in queue_tracks.iter().enumerate() {
        let item_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO queue_items (id, queue_id, track_id, position, added_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(item_id.to_string())
        .bind(queue_id.to_string())
        .bind(track.id.to_string())
        .bind(i as i64)
        .bind(&now)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", &e.to_string())
        })?;
    }

    tx.commit().await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;

    // 6. Synchronize PlaybackEngine
    let current_index = pending
        .tailsync
        .queue_index
        .min(queue_tracks.len().saturating_sub(1));
    let current_track = queue_tracks
        .get(current_index)
        .cloned()
        .unwrap_or_else(|| pending.prepared_current_track.clone());

    state
        .playback_engine
        .set_queue(queue_tracks, current_index, Some(current_track.id))
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RUNTIME_SYNC_FAILED",
                &e.to_string(),
            )
        })?;

    let plan = resolve_output(&selection, &state)
        .await
        .map_err(|e| v1_error(StatusCode::BAD_GATEWAY, e.error_code(), &e.to_string()))?;

    if pending.tailsync.status == "playing" {
        state
            .playback_engine
            .play(
                current_track,
                plan.sinks,
                plan.description,
                pending.tailsync.position_ms,
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
        state
            .playback_engine
            .load_track(current_track, pending.tailsync.position_ms)
            .await
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    e.error_code(),
                    &e.to_string(),
                )
            })?;
    }

    // 7. Cleanup pending transfer
    state.pending_transfers.remove(&body.transfer_id).await;

    Ok(Json(PlaybackTransferCommitResponse {
        transfer_id: body.transfer_id,
        status: "committed".to_string(),
        playback_session_id: session_id,
    }))
}

/// POST /api/v1/playback-transfer/abort
pub async fn playback_transfer_abort_handler(
    State(state): State<AppState>,
    Json(body): Json<PlaybackTransferAbortRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    state.pending_transfers.remove(&body.transfer_id).await;
    Ok(Json(serde_json::json!({
        "transfer_id": body.transfer_id,
        "status": "aborted",
    })))
}
