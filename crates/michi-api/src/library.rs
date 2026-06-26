use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tracing::info;
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ScanResponse {
    pub status: String,
    pub scanned: usize,
    pub saved: usize,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[derive(Debug, Serialize)]
pub struct PurgeResponse {
    pub deleted: usize,
}

pub async fn scan_handler(
    State(state): State<AppState>,
) -> Result<Json<ScanResponse>, (StatusCode, Json<ErrorResponse>)> {
    let music_paths = &state.config.music_paths;

    if music_paths.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: "no music paths configured".to_string(),
            }),
        ));
    }

    for path in music_paths {
        if !path.exists() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("music path not found: {}", path.display()),
                }),
            ));
        }
        if !path.is_dir() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("music path is not a directory: {}", path.display()),
                }),
            ));
        }
    }

    let _ = state.tx.send(r#"{"type":"scan_start"}"#.to_string());

    info!("scanning music library at {:?}", music_paths);

    let tracks = michi_scanner::scan_directories(music_paths).await;
    let scanned = tracks.len();

    let saved = michi_db::upsert_tracks(&state.db, &tracks)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    info!("scan complete: {} tracks scanned, {} saved", scanned, saved);

    let _ = state.tx.send(format!(
        r#"{{"type":"scan_done","scanned":{},"saved":{}}}"#,
        scanned, saved
    ));

    Ok(Json(ScanResponse {
        status: "ok".to_string(),
        scanned,
        saved,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct TracksQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn tracks_handler(
    State(state): State<AppState>,
    Query(query): Query<TracksQuery>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<ErrorResponse>)> {
    let tracks = if let Some(limit) = query.limit {
        let limit = limit.clamp(1, 500);
        let offset = query.offset.unwrap_or(0).max(0);
        michi_db::list_tracks_paged(&state.db, limit, offset).await
    } else {
        michi_db::list_tracks(&state.db).await
    }
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    Ok(Json(tracks))
}

#[derive(Debug, Deserialize)]
pub struct SearchParams {
    pub q: String,
}

pub async fn search_handler(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<ErrorResponse>)> {
    if params.q.trim().is_empty() {
        return Ok(Json(Vec::new()));
    }

    let tracks = michi_db::search_tracks(&state.db, params.q.trim())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    Ok(Json(tracks))
}

pub async fn stats_handler(
    State(state): State<AppState>,
) -> Result<Json<michi_core::LibraryStats>, (StatusCode, Json<ErrorResponse>)> {
    let stats = michi_db::library_stats(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    Ok(Json(stats))
}

pub async fn track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<michi_core::Track>, (StatusCode, Json<ErrorResponse>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("track not found: {}", id),
                }),
            )
        })?;

    Ok(Json(track))
}

pub async fn delete_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = michi_db::delete_track(&state.db, &id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    if deleted {
        Ok(Json(DeleteResponse { deleted: true }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("track not found: {}", id),
            }),
        ))
    }
}

pub async fn update_track_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(update): Json<michi_core::TrackUpdate>,
) -> Result<Json<michi_core::Track>, (StatusCode, Json<ErrorResponse>)> {
    let updated = michi_db::update_track(&state.db, &id, &update)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    if !updated {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("track not found: {}", id),
            }),
        ));
    }

    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: "track lost after update".to_string(),
                }),
            )
        })?;

    Ok(Json(track))
}

pub async fn delete_all_tracks_handler(
    State(state): State<AppState>,
) -> Result<Json<PurgeResponse>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = michi_db::delete_all_tracks(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;

    let _ = state.tx.send(r#"{"type":"library_updated"}"#.to_string());

    Ok(Json(PurgeResponse {
        deleted: deleted as usize,
    }))
}

pub async fn albums_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<michi_core::AlbumSummary>>, (StatusCode, Json<ErrorResponse>)> {
    let albums = michi_db::list_albums(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;
    Ok(Json(albums))
}

pub async fn artists_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<michi_core::ArtistSummary>>, (StatusCode, Json<ErrorResponse>)> {
    let artists = michi_db::list_artists(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;
    Ok(Json(artists))
}

pub async fn album_tracks_handler(
    State(state): State<AppState>,
    Path(album): Path<String>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<ErrorResponse>)> {
    let tracks = michi_db::get_album_tracks(&state.db, &album)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    Ok(Json(tracks))
}

pub async fn artist_tracks_handler(
    State(state): State<AppState>,
    Path(artist): Path<String>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<ErrorResponse>)> {
    let tracks = michi_db::get_artist_tracks(&state.db, &artist)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    Ok(Json(tracks))
}

pub async fn artwork_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("track not found: {}", id),
                }),
            )
        })?;

    let cache_path = state.config.cache_path.join("artwork");
    let artwork_path = cache_path.join(id.to_string());

    if artwork_path.exists() {
        let data = tokio::fs::read(&artwork_path).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("failed to read artwork: {}", e),
                }),
            )
        })?;
        let mime = infer::get(&data)
            .map(|t| t.mime_type())
            .unwrap_or("image/jpeg");
        return Ok(artwork_response(mime, data));
    }

    let path = std::path::Path::new(&track.file_path);
    if path.is_absolute() && path.exists() {
        let result = extract_and_cache(path, &cache_path, &id).await;
        if let Ok(resp) = result {
            return Ok(resp);
        }
    } else {
        for music_path in &state.config.music_paths {
            let full = music_path.join(path);
            if full.exists() {
                let result = extract_and_cache(&full, &cache_path, &id).await;
                if let Ok(resp) = result {
                    return Ok(resp);
                }
            }
        }
    }

    Err((
        StatusCode::NOT_FOUND,
        Json(ErrorResponse {
            status: "error".to_string(),
            message: "no artwork found".to_string(),
        }),
    ))
}

fn artwork_response(mime: &str, data: Vec<u8>) -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    headers.insert(header::CONTENT_LENGTH, data.len().into());
    (headers, data).into_response()
}

async fn extract_and_cache(
    path: &std::path::Path,
    cache_path: &std::path::Path,
    id: &Uuid,
) -> Result<Response, ()> {
    match michi_metadata::extract_artwork(path) {
        Ok(data) => {
            tokio::fs::create_dir_all(cache_path).await.ok();
            let artwork_path = cache_path.join(id.to_string());
            let _ = tokio::fs::write(&artwork_path, &data).await;
            let mime = infer::get(&data)
                .map(|t| t.mime_type())
                .unwrap_or("image/jpeg");
            Ok(artwork_response(mime, data))
        }
        Err(_) => Err(()),
    }
}

pub async fn playlists_handler(
    State(state): State<AppState>,
) -> Result<Json<Vec<michi_core::Playlist>>, (StatusCode, Json<ErrorResponse>)> {
    let playlists = michi_db::list_playlists(&state.db).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("database error: {}", e),
            }),
        )
    })?;
    Ok(Json(playlists))
}

pub async fn create_playlist_handler(
    State(state): State<AppState>,
    Json(input): Json<michi_core::PlaylistCreate>,
) -> Result<Json<michi_core::Playlist>, (StatusCode, Json<ErrorResponse>)> {
    if input.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: "playlist name is required".to_string(),
            }),
        ));
    }
    let playlist = michi_db::create_playlist(&state.db, &input)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(playlist))
}

pub async fn get_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<michi_core::Playlist>, (StatusCode, Json<ErrorResponse>)> {
    let playlist = michi_db::get_playlist(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("playlist not found: {}", id),
                }),
            )
        })?;
    Ok(Json(playlist))
}

pub async fn delete_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<DeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = michi_db::delete_playlist(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    if deleted {
        let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
        Ok(Json(DeleteResponse { deleted: true }))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("playlist not found: {}", id),
            }),
        ))
    }
}

pub async fn add_playlist_track_handler(
    State(state): State<AppState>,
    Path((playlist_id, track_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<michi_core::PlaylistTrack>, (StatusCode, Json<ErrorResponse>)> {
    let pt = michi_db::add_track_to_playlist(&state.db, &playlist_id, &track_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(pt))
}

pub async fn remove_playlist_track_handler(
    State(state): State<AppState>,
    Path((playlist_id, track_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<DeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    let deleted = michi_db::remove_track_from_playlist(&state.db, &playlist_id, &track_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(DeleteResponse { deleted }))
}

#[derive(Debug, Deserialize)]
pub struct SetPlaybackState {
    pub track_id: Option<Uuid>,
    pub position_ms: u64,
    pub playing: bool,
    pub volume: Option<f64>,
}

pub async fn get_playback_state_handler(
    State(state): State<AppState>,
) -> Result<Json<michi_sync::PlaybackState>, (StatusCode, Json<ErrorResponse>)> {
    let current = state.playback_state.read().await;
    Ok(Json(current.clone()))
}

pub async fn set_playback_state_handler(
    State(state): State<AppState>,
    Json(input): Json<SetPlaybackState>,
) -> Result<Json<michi_sync::PlaybackState>, (StatusCode, Json<ErrorResponse>)> {
    let new_state = michi_sync::PlaybackState {
        track_id: input.track_id,
        position_ms: input.position_ms,
        playing: input.playing,
        volume: input.volume.unwrap_or(0.8),
        updated_at: Utc::now(),
    };

    {
        let mut current = state.playback_state.write().await;
        *current = new_state.clone();
    }

    // Broadcast to sync peers
    let _ = state.sync_tx.send(new_state.clone().into());

    Ok(Json(new_state))
}

pub async fn export_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<ErrorResponse>)> {
    let tracks = michi_db::get_playlist_tracks(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    let entries: Vec<michi_m3u::M3uEntry> = tracks
        .into_iter()
        .map(|(_, t)| michi_m3u::M3uEntry {
            duration: t.duration_ms,
            title: t.title.clone(),
            path: t.file_path.clone(),
        })
        .collect();

    let m3u_content = michi_m3u::serialize_m3u(&entries);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "audio/x-mpegurl".parse().unwrap());
    headers.insert(
        header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"playlist-{}.m3u\"", id)
            .parse()
            .unwrap(),
    );

    Ok((headers, m3u_content).into_response())
}

#[derive(Debug, Deserialize)]
pub struct ImportPlaylistInput {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct ImportPlaylistResponse {
    pub playlist: michi_core::Playlist,
    pub matched: usize,
    pub total: usize,
}

pub async fn import_playlist_handler(
    State(state): State<AppState>,
    Json(input): Json<ImportPlaylistInput>,
) -> Result<Json<ImportPlaylistResponse>, (StatusCode, Json<ErrorResponse>)> {
    if input.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: "playlist name is required".to_string(),
            }),
        ));
    }

    let entries = michi_m3u::parse_m3u(&input.content).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                status: "error".to_string(),
                message: format!("invalid M3U file: {}", e),
            }),
        )
    })?;

    let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let matched_tracks = michi_db::find_tracks_by_paths(&state.db, &paths)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    let create = michi_core::PlaylistCreate {
        name: input.name.trim().to_string(),
        description: Some(format!(
            "Imported from M3U ({} of {} tracks matched)",
            matched_tracks.len(),
            entries.len()
        )),
    };

    let playlist = michi_db::create_playlist(&state.db, &create)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;

    for track in &matched_tracks {
        let _ = michi_db::add_track_to_playlist(&state.db, &playlist.id, &track.id).await;
    }

    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());

    Ok(Json(ImportPlaylistResponse {
        matched: matched_tracks.len(),
        total: entries.len(),
        playlist,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ReorderPlaylistInput {
    pub track_ids: Vec<Uuid>,
}

pub async fn reorder_playlist_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(input): Json<ReorderPlaylistInput>,
) -> Result<Json<DeleteResponse>, (StatusCode, Json<ErrorResponse>)> {
    michi_db::reorder_playlist_tracks(&state.db, &id, &input.track_ids)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    let _ = state.tx.send(r#"{"type":"playlist_updated"}"#.to_string());
    Ok(Json(DeleteResponse { deleted: true }))
}

pub async fn get_playlist_tracks_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<michi_core::Track>>, (StatusCode, Json<ErrorResponse>)> {
    let tracks = michi_db::get_playlist_tracks(&state.db, &id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    status: "error".to_string(),
                    message: format!("database error: {}", e),
                }),
            )
        })?;
    Ok(Json(tracks.into_iter().map(|(_, t)| t).collect()))
}
