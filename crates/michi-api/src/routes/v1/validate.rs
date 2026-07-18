use axum::{extract::State, Json};
use crate::AppState;

pub async fn config_validate_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let cfg = &state.config;
    let mut errors: Vec<serde_json::Value> = Vec::new();
    let mut warnings: Vec<serde_json::Value> = Vec::new();

    // Validate port
    if cfg.port() == 0 || cfg.port() > 65535 {
        errors.push(serde_json::json!({"code": "INVALID_PORT", "message": "Port must be 1-65535"}));
    }

    // Validate music paths
    for p in &cfg.music_paths {
        if !p.exists() {
            warnings.push(serde_json::json!({
                "code": "MUSIC_PATH_MISSING",
                "path": p.display().to_string(),
                "message": "Music path does not exist"
            }));
        }
        if !p.is_dir() {
            errors.push(serde_json::json!({
                "code": "MUSIC_PATH_NOT_DIR",
                "path": p.display().to_string(),
                "message": "Music path is not a directory"
            }));
        }
    }

    // Validate config and cache directories
    for (name, path) in [("config", &cfg.config_path), ("cache", &cfg.cache_path)] {
        if !path.exists() {
            warnings.push(serde_json::json!({
                "code": format!("{}_PATH_MISSING", name.to_uppercase()),
                "path": path.display().to_string(),
                "message": format!("{} directory does not exist", name)
            }));
        }
    }

    // Validate database URL
    if !cfg.database_url.starts_with("sqlite://") {
        errors.push(serde_json::json!({
            "code": "INVALID_DATABASE_URL",
            "message": "Database URL must start with sqlite://"
        }));
    }

    // Validate CORS if set
    if let Some(ref origin) = cfg.cors_origin {
        if !origin.starts_with("http://") && !origin.starts_with("https://") {
            warnings.push(serde_json::json!({
                "code": "CORS_NOT_HTTP",
                "message": "CORS origin should start with http:// or https://"
            }));
        }
    }

    Json(serde_json::json!({
        "valid": errors.is_empty(),
        "errors": errors,
        "warnings": warnings,
    }))
}
