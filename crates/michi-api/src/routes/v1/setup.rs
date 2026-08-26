use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

pub async fn setup_status_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let status = michi_onboard::check_setup_status(&state.db).await;
    Ok(Json(serde_json::json!(status)))
}

pub async fn setup_scan_handler(
    State(_state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let paths = michi_onboard::discover_music_paths_wrapper();
    let (files, bytes) = michi_onboard::scan_music_stats(&paths).await;
    let size_mb = bytes / (1024 * 1024);
    Ok(Json(serde_json::json!({
        "paths": paths,
        "files_found": files,
        "total_size_mb": size_mb,
        "status": if files > 0 { "ready" } else { "empty" },
    })))
}

#[derive(Deserialize)]
pub struct FixPermsBody {
    pub path: Option<String>,
}

pub async fn setup_fix_perms_handler(
    State(state): State<AppState>,
    Json(body): Json<FixPermsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target = match body.path {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => {
            return Err(v1_error(
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                "path parameter is required",
            ));
        }
    };
    let path = std::path::Path::new(&target);
    if !path.exists() {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "path does not exist",
        ));
    }

    let canonical_target = match path.canonicalize() {
        Ok(c) => c,
        Err(_) => path.to_path_buf(),
    };

    // Strictly forbid /music and all configured music library paths
    if target == "/music" || target.starts_with("/music/") {
        return Err(v1_error(
            StatusCode::FORBIDDEN,
            "UNSAFE_TARGET",
            "permission fix is strictly forbidden on music library paths",
        ));
    }

    for mp in &state.config.music_paths {
        let canonical_mp = mp.canonicalize().unwrap_or_else(|_| mp.clone());
        if canonical_target == canonical_mp || canonical_target.starts_with(&canonical_mp) {
            return Err(v1_error(
                StatusCode::FORBIDDEN,
                "UNSAFE_TARGET",
                "permission fix is strictly forbidden on music library paths",
            ));
        }
    }

    // Only allow config_path or cache_path (or subdirectories thereof)
    let is_allowed_config = state
        .config
        .config_path
        .canonicalize()
        .map(|p| canonical_target.starts_with(&p) || canonical_target == p)
        .unwrap_or(false);
    let is_allowed_cache = state
        .config
        .cache_path
        .canonicalize()
        .map(|p| canonical_target.starts_with(&p) || canonical_target == p)
        .unwrap_or(false);

    if !is_allowed_config && !is_allowed_cache {
        return Err(v1_error(
            StatusCode::FORBIDDEN,
            "UNSAFE_TARGET",
            "permission fix is only allowed on application config and cache directories",
        ));
    }

    // In container: chown -R 1000:1000 (safe, no symlink follow)
    let result = std::process::Command::new("chown")
        .arg("-R")
        .arg("1000:1000")
        .arg(&target)
        .output();
    match result {
        Ok(output) => {
            if output.status.success() {
                Ok(Json(serde_json::json!({ "status": "ok", "path": target })))
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Ok(Json(
                    serde_json::json!({ "status": "error", "message": stderr }),
                ))
            }
        }
        Err(e) => Ok(Json(
            serde_json::json!({ "status": "error", "message": e.to_string() }),
        )),
    }
}
