use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use crate::AppState;

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
    State(state): State<AppState>,
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
    Json(body): Json<FixPermsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let target = body.path.unwrap_or_else(|| "/music".to_string());
    let path = std::path::Path::new(&target);
    if !path.exists() {
        return Err(v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "path does not exist"));
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
                Ok(Json(serde_json::json!({ "status": "error", "message": stderr })))
            }
        }
        Err(e) => Ok(Json(serde_json::json!({ "status": "error", "message": e.to_string() }))),
    }
}
