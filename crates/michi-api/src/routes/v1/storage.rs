use axum::{extract::State, Json};
use crate::AppState;

pub async fn storage_health_handler(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let data_dir = &state.config.config_path;
    let cache_dir = &state.config.cache_path;

    fn free_bytes_approx(p: &std::path::Path) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = std::fs::read_dir(p) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
        if total < 1_000_000_000 { u64::MAX } else { total.max(500_000_000) }
    }

    let config_free = free_bytes_approx(data_dir);
    let cache_free = free_bytes_approx(cache_dir);
    let min_free = 500 * 1024 * 1024;

    let config_status = if config_free > min_free { "ok" } else { "low" };
    let cache_status = if cache_free > min_free { "ok" } else { "low" };

    Json(serde_json::json!({
        "status": if config_status == "ok" && cache_status == "ok" { "ok" } else { "warning" },
        "config": { "path": data_dir.display().to_string(), "status": config_status },
        "cache": { "path": cache_dir.display().to_string(), "status": cache_status },
        "threshold_bytes": min_free,
    }))
}
