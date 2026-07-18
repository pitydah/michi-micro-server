use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;

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

pub async fn list_chains_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let chains = michi_db::list_chains(&state.db).await.map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DATABASE_ERROR",
            &e.to_string(),
        )
    })?;
    Ok(Json(serde_json::json!({ "chains": chains })))
}

pub async fn create_chain_handler(
    State(state): State<AppState>,
    Json(body): Json<michi_core::PlaybackChainCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "chain name is required",
        ));
    }
    let chain = michi_db::create_chain(&state.db, &body)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "chain": chain })))
}

pub async fn get_chain_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let result = michi_db::get_chain_with_links(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    match result {
        Some((chain, links)) => Ok(Json(serde_json::json!({ "chain": chain, "links": links }))),
        None => Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("chain not found: {}", id),
        )),
    }
}

pub async fn update_chain_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<michi_core::PlaybackChainUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let updated = michi_db::update_chain(&state.db, &id, &body)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    if !updated {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("chain not found: {}", id),
        ));
    }
    Ok(Json(serde_json::json!({ "status": "updated" })))
}

pub async fn delete_chain_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_chain(&state.db, &id).await.map_err(|e| {
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
            &format!("chain not found: {}", id),
        ));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

// ── Links ────────────────────────────────────────────────────────

pub async fn add_link_handler(
    State(state): State<AppState>,
    Path(chain_id): Path<Uuid>,
    Json(body): Json<michi_core::ChainLinkCreate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.receiver_id.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "receiver_id is required",
        ));
    }
    let link = michi_db::add_chain_link(&state.db, &chain_id, &body)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "link": link })))
}

pub async fn update_link_handler(
    State(state): State<AppState>,
    Path((_chain_id, link_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<michi_core::ChainLinkUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let updated = michi_db::update_chain_link(&state.db, &link_id, &body)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    if !updated {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("link not found: {}", link_id),
        ));
    }
    Ok(Json(serde_json::json!({ "status": "updated" })))
}

pub async fn delete_link_handler(
    State(state): State<AppState>,
    Path((_chain_id, link_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_chain_link(&state.db, &link_id)
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
            &format!("link not found: {}", link_id),
        ));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

#[derive(Deserialize)]
pub struct ReorderBody {
    pub link_ids: Vec<Uuid>,
}

pub async fn reorder_links_handler(
    State(state): State<AppState>,
    Path(chain_id): Path<Uuid>,
    Json(body): Json<ReorderBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_db::reorder_chain_links(&state.db, &chain_id, &body.link_ids)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "status": "reordered" })))
}

// ── Play control ─────────────────────────────────────────────────

pub async fn play_chain_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let result = michi_db::get_chain_with_links(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let (chain, links) = result.ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            &format!("chain not found: {}", id),
        )
    })?;

    // Update playback state
    {
        let mut ps = state.playback_state.write().await;
        ps.track_id = chain.track_id;
        ps.position_ms = chain.position_ms;
        ps.playing = true;
        ps.updated_at = chrono::Utc::now();
        ps.device_id = Some("chain".into());
    }

    // Start receiver sessions for each link
    for link in &links {
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(&link.receiver_id) {
            if entry.paired && entry.active_session_id.is_none() {
                let _ = state
                    .receiver_manager
                    .start_session(
                        &link.receiver_id,
                        &id.to_string(),
                        "pcm",
                        48000,
                        24,
                        2,
                        0,
                        200,
                        link.volume as u32,
                    )
                    .await;
            }
            // Set volume per receiver
            if link.muted {
                let _ = state
                    .receiver_manager
                    .set_volume(&link.receiver_id, 0)
                    .await;
            } else {
                let _ = state
                    .receiver_manager
                    .set_volume(&link.receiver_id, link.volume as u32)
                    .await;
            }
        }
    }

    let update = michi_core::PlaybackChainUpdate {
        name: None,
        track_id: None,
        position_ms: Some(chain.position_ms),
        playing: Some(true),
        shuffle: None,
        repeat_mode: None,
    };
    let _ = michi_db::update_chain(&state.db, &id, &update).await;

    Ok(Json(serde_json::json!({
        "status": "playing",
        "chain_id": id,
        "links_active": links.len(),
    })))
}

pub async fn stop_chain_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let links = michi_db::get_chain_links(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    for link in &links {
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(&link.receiver_id) {
            if entry.active_session_id.is_some() {
                let _ = state.receiver_manager.stop_session(&link.receiver_id).await;
            }
        }
    }

    let update = michi_core::PlaybackChainUpdate {
        name: None,
        track_id: None,
        position_ms: None,
        playing: Some(false),
        shuffle: None,
        repeat_mode: None,
    };
    let _ = michi_db::update_chain(&state.db, &id, &update).await;

    Ok(Json(
        serde_json::json!({ "status": "stopped", "chain_id": id }),
    ))
}

pub async fn chain_volume_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let volume = body.get("volume").and_then(|v| v.as_u64()).unwrap_or(80) as i64;
    let links = michi_db::get_chain_links(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    for link in &links {
        let update = michi_core::ChainLinkUpdate {
            volume: Some(volume),
            muted: None,
            delay_ms: None,
            position: None,
        };
        let _ = michi_db::update_chain_link(&state.db, &link.id, &update).await;

        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(&link.receiver_id) {
            if entry.paired {
                let _ = state
                    .receiver_manager
                    .set_volume(&link.receiver_id, volume as u32)
                    .await;
            }
        }
    }

    Ok(Json(
        serde_json::json!({ "status": "volume_set", "volume": volume }),
    ))
}
