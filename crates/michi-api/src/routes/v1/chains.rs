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
            &format!("chain not found: {id}"),
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
            &format!("chain not found: {id}"),
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
            &format!("chain not found: {id}"),
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
            &format!("link not found: {link_id}"),
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
            &format!("link not found: {link_id}"),
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
            &format!("chain not found: {id}"),
        )
    })?;

    let configured_links = links.len();
    if configured_links == 0 {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "NO_OUTPUTS",
            "chain has no configured links",
        ));
    }

    // Start receiver sessions for each link and record per-link outcome
    let mut link_results = Vec::new();
    let mut active_sessions_count = 0usize;

    for link in &links {
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(&link.receiver_id) {
            if entry.paired {
                let session_res = if entry.active_session_id.is_none() {
                    state
                        .receiver_manager
                        .start_session(
                            &link.receiver_id,
                            &id.to_string(),
                            "pcm_s16le",
                            48000,
                            16,
                            2,
                            0,
                            200,
                            link.volume as u32,
                        )
                        .await
                        .map(|_| ())
                } else {
                    Ok(())
                };

                match session_res {
                    Ok(_) => {
                        active_sessions_count += 1;
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
                        link_results.push(serde_json::json!({
                            "receiver_id": link.receiver_id,
                            "status": "active",
                        }));
                    }
                    Err(e) => {
                        link_results.push(serde_json::json!({
                            "receiver_id": link.receiver_id,
                            "status": "failed",
                            "error": e.to_string(),
                        }));
                    }
                }
            } else {
                link_results.push(serde_json::json!({
                    "receiver_id": link.receiver_id,
                    "status": "failed",
                    "error": "receiver not paired",
                }));
            }
        } else {
            link_results.push(serde_json::json!({
                "receiver_id": link.receiver_id,
                "status": "failed",
                "error": "receiver not found in registry",
            }));
        }
    }

    if active_sessions_count == 0 {
        return Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "PLAYBACK_FAILED",
            &format!("failed to start audio output on any of {configured_links} configured links"),
        ));
    }

    // Update canonical playback state only when at least one output is active
    {
        let mut ps = state.playback_state.write().await;
        ps.track_id = chain.track_id;
        ps.position_ms = chain.position_ms;
        ps.playing = true;
        ps.updated_at = chrono::Utc::now();
        ps.device_id = Some("chain".into());
    }

    let status = if active_sessions_count == configured_links {
        "playing"
    } else {
        "partial"
    };

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
        "status": status,
        "chain_id": id,
        "configured_links": configured_links,
        "active_links": active_sessions_count,
        "failed_links": configured_links - active_sessions_count,
        "links": link_results,
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

    let mut link_results = Vec::new();
    let mut stopped_count = 0usize;
    let mut failed_count = 0usize;

    for link in &links {
        let reg = state.receiver_manager.registry().await;
        let reg_read = reg.read().await;
        if let Some(entry) = reg_read.get(&link.receiver_id) {
            if entry.active_session_id.is_some() {
                drop(reg_read);
                drop(reg);
                match state.receiver_manager.stop_session(&link.receiver_id).await {
                    Ok(_) => {
                        stopped_count += 1;
                        link_results.push(serde_json::json!({
                            "receiver_id": link.receiver_id,
                            "status": "stopped",
                        }));
                    }
                    Err(e) => {
                        failed_count += 1;
                        link_results.push(serde_json::json!({
                            "receiver_id": link.receiver_id,
                            "status": "failed",
                            "error": e.to_string(),
                        }));
                    }
                }
            } else {
                stopped_count += 1;
                link_results.push(serde_json::json!({
                    "receiver_id": link.receiver_id,
                    "status": "already_inactive",
                }));
            }
        }
    }

    let status = if failed_count == 0 {
        "stopped"
    } else if stopped_count > 0 {
        "partial"
    } else {
        "failed"
    };

    if failed_count == 0 || stopped_count > 0 {
        let update = michi_core::PlaybackChainUpdate {
            name: None,
            track_id: None,
            position_ms: None,
            playing: Some(false),
            shuffle: None,
            repeat_mode: None,
        };
        let _ = michi_db::update_chain(&state.db, &id, &update).await;
    }

    Ok(Json(serde_json::json!({
        "status": status,
        "chain_id": id,
        "stopped_count": stopped_count,
        "failed_count": failed_count,
        "links": link_results,
    })))
}

pub async fn chain_volume_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let volume_opt = body.get("volume").and_then(|v| v.as_i64());
    let volume = match volume_opt {
        Some(v) if (0..=100).contains(&v) => v,
        _ => {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "volume must be an integer between 0 and 100",
            ));
        }
    };

    let links = michi_db::get_chain_links(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;

    let mut link_results = Vec::new();
    let mut success_count = 0usize;
    let mut failed_count = 0usize;

    for link in &links {
        let update = michi_core::ChainLinkUpdate {
            volume: Some(volume),
            muted: None,
            delay_ms: None,
            position: None,
        };
        let db_res = michi_db::update_chain_link(&state.db, &link.id, &update).await;

        let hw_res = {
            let reg = state.receiver_manager.registry().await;
            let reg_read = reg.read().await;
            if let Some(entry) = reg_read.get(&link.receiver_id) {
                if entry.paired {
                    drop(reg_read);
                    drop(reg);
                    state
                        .receiver_manager
                        .set_volume(&link.receiver_id, volume as u32)
                        .await
                        .map_err(|e| e.to_string())
                } else {
                    Err("receiver not paired".into())
                }
            } else {
                Err("receiver not in registry".into())
            }
        };

        match (db_res, hw_res) {
            (Ok(true), Ok(_)) => {
                success_count += 1;
                link_results.push(serde_json::json!({
                    "receiver_id": link.receiver_id,
                    "status": "updated",
                    "effective_volume": volume,
                }));
            }
            (Ok(true), Err(e)) => {
                failed_count += 1;
                link_results.push(serde_json::json!({
                    "receiver_id": link.receiver_id,
                    "status": "hardware_failed",
                    "error": e,
                }));
            }
            (Err(e), _) => {
                failed_count += 1;
                link_results.push(serde_json::json!({
                    "receiver_id": link.receiver_id,
                    "status": "db_failed",
                    "error": e.to_string(),
                }));
            }
            (Ok(false), _) => {
                failed_count += 1;
                link_results.push(serde_json::json!({
                    "receiver_id": link.receiver_id,
                    "status": "link_not_found",
                }));
            }
        }
    }

    let status = if links.is_empty() {
        "no_links"
    } else if failed_count == 0 {
        "success"
    } else if success_count > 0 {
        "partial"
    } else {
        "failed"
    };

    Ok(Json(serde_json::json!({
        "status": status,
        "requested_volume": volume,
        "links": link_results,
    })))
}
