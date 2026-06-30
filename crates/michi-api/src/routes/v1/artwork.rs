use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
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

pub async fn artwork_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let track = michi_db::get_track(&state.db, &id)
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
                &format!("track not found: {}", id),
            )
        })?;

    let cache_path = state.config.cache_path.join("artwork");
    let artwork_path = cache_path.join(id.to_string());

    if artwork_path.exists() {
        let data = tokio::fs::read(&artwork_path).await.map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "IO_ERROR",
                &format!("failed to read artwork: {}", e),
            )
        })?;
        let mime = infer::get(&data)
            .map(|t| t.mime_type())
            .unwrap_or("image/jpeg");
        return Ok(([(axum::http::header::CONTENT_TYPE, mime)], data).into_response());
    }

    let path = std::path::Path::new(&track.file_path);
    if path.is_absolute() && path.exists() {
        if let Ok(resp) = extract_and_cache(path, &cache_path, &id).await {
            return Ok(resp);
        }
    } else {
        for music_path in &state.config.music_paths {
            let full = music_path.join(path);
            if full.exists() {
                if let Ok(resp) = extract_and_cache(&full, &cache_path, &id).await {
                    return Ok(resp);
                }
            }
        }
    }

    Err(v1_error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "no artwork found",
    ))
}

async fn extract_and_cache(
    path: &std::path::Path,
    cache_path: &std::path::Path,
    id: &Uuid,
) -> Result<Response, ()> {
    match michi_metadata::extract_artwork(path) {
        Ok(data) => {
            tokio::fs::create_dir_all(cache_path).await.ok();
            let _ = tokio::fs::write(&cache_path.join(id.to_string()), &data).await;
            let mime = infer::get(&data)
                .map(|t| t.mime_type())
                .unwrap_or("image/jpeg");
            Ok(([(axum::http::header::CONTENT_TYPE, mime)], data).into_response())
        }
        Err(_) => Err(()),
    }
}
