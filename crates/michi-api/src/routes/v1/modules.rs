use crate::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Serialize, Deserialize)]
pub struct ModuleDescriptor {
    pub name: String,
    pub enabled: bool,
    pub description: String,
}

fn builtin_modules() -> Vec<ModuleDescriptor> {
    vec![
        ModuleDescriptor {
            name: "scan".into(),
            enabled: true,
            description: "Music library scanning".into(),
        },
        ModuleDescriptor {
            name: "sync".into(),
            enabled: true,
            description: "Peer synchronization".into(),
        },
        ModuleDescriptor {
            name: "stream".into(),
            enabled: true,
            description: "Audio streaming".into(),
        },
        ModuleDescriptor {
            name: "playback".into(),
            enabled: true,
            description: "Playback tracking".into(),
        },
        ModuleDescriptor {
            name: "backup".into(),
            enabled: true,
            description: "Backup and restore".into(),
        },
        ModuleDescriptor {
            name: "webhook".into(),
            enabled: true,
            description: "Webhook notifications".into(),
        },
        ModuleDescriptor {
            name: "homeassistant".into(),
            enabled: true,
            description: "Home Assistant integration".into(),
        },
    ]
}

pub async fn modules_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let disabled = state.disabled_modules.read().await;
    let mut modules = builtin_modules();
    for m in &mut modules {
        m.enabled = !disabled.contains(&m.name);
    }
    Json(serde_json::json!({ "modules": modules }))
}

#[derive(Debug, Deserialize)]
pub struct ToggleModuleBody {
    pub name: String,
    pub enabled: bool,
}

pub async fn toggle_module_handler(
    State(state): State<AppState>,
    Json(body): Json<ToggleModuleBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    // Validate module name
    if !builtin_modules().iter().any(|m| m.name == body.name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": {"code": "UNKNOWN_MODULE", "message": format!("unknown module: {}", body.name)}}),
            ),
        ));
    }

    if body.enabled {
        state.disabled_modules.write().await.remove(&body.name);
        let mut tokens = state.module_tokens.write().await;
        if tokens
            .get(&body.name)
            .map(|token| token.is_cancelled())
            .unwrap_or(true)
        {
            tokens.insert(
                body.name.clone(),
                tokio_util::sync::CancellationToken::new(),
            );
        }
        tracing::info!("module '{}' enabled", body.name);
    } else {
        state
            .disabled_modules
            .write()
            .await
            .insert(body.name.clone());
        if let Some(token) = state.module_tokens.read().await.get(&body.name).cloned() {
            token.cancel();
            tracing::info!("module '{}' disabled, tasks cancelled", body.name);
        }
    }

    Ok(Json(
        serde_json::json!({ "module": body.name, "enabled": body.enabled }),
    ))
}

// ── Self-Test ──────────────────────────────────────────────────

pub async fn self_test_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut results = Vec::new();

    let db_ok = sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    results.push(serde_json::json!({
        "name": "database",
        "status": if db_ok { "passed" } else { "failed" },
    }));

    let config_ok = state.config.config_path.exists();
    results.push(serde_json::json!({
        "name": "config_path",
        "status": if config_ok { "passed" } else { "warning" },
        "info": state.config.config_path.display().to_string(),
    }));

    let cache_ok = state.config.cache_path.exists();
    results.push(serde_json::json!({
        "name": "cache_path",
        "status": if cache_ok { "passed" } else { "warning" },
        "info": state.config.cache_path.display().to_string(),
    }));

    for p in &state.config.music_paths {
        let exists = p.exists();
        let readable = p.is_dir();
        results.push(serde_json::json!({
            "name": "music_path",
            "status": if exists && readable { "passed" } else { "warning" },
            "info": p.display().to_string(),
        }));
    }

    if state.config.auth_enabled {
        results.push(serde_json::json!({
            "name": "admin_token_configured",
            "status": "passed",
            "info": "authentication is enabled",
        }));
    }

    Json(serde_json::json!({
        "status": if results.iter().all(|r| r["status"] == "passed") { "passed" } else { "warning" },
        "checks": results,
    }))
}

// ── Capabilities Manifest ──────────────────────────────────────

pub async fn capabilities_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let caps = crate::server_caps::ServerCapabilities::from_state(&state).await;
    Json(serde_json::to_value(caps).unwrap_or_else(|_| serde_json::json!({})))
}

// ── Change Journal ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ChangeEntry {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub diff: Option<serde_json::Value>,
    pub created_at: String,
}

pub async fn change_journal_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let rows = sqlx::query_as::<_, (String, String, String, String, Option<String>, String)>(
        "SELECT id, entity_type, entity_id, action, diff_json, created_at
         FROM change_journal ORDER BY created_at DESC LIMIT 100",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|e| {
        let s = StatusCode::INTERNAL_SERVER_ERROR;
        (
            s,
            Json(serde_json::json!({"error": {"code": "DB_ERROR", "message": e.to_string()}})),
        )
    })?;

    let entries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(id, et, eid, action, diff, created)| {
            serde_json::json!({
                "id": id,
                "entity_type": et,
                "entity_id": eid,
                "action": action,
                "diff": diff.and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok()),
                "created_at": created,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "entries": entries })))
}

// ── LAN / Remote Policy ────────────────────────────────────────

const LAN_IP_RANGES: &[&str] = &[
    "10.", "192.168.", "172.16.", "172.17.", "172.18.", "172.19.", "172.20.", "172.21.", "172.22.",
    "172.23.", "172.24.", "172.25.", "172.26.", "172.27.", "172.28.", "172.29.", "172.30.",
    "172.31.", "127.", "::1", "fd",
];

fn is_lan_ip(ip: &str) -> bool {
    LAN_IP_RANGES.iter().any(|r| ip.starts_with(r))
}

#[derive(Debug, Deserialize)]
pub struct PolicyQuery {
    pub client_ip: Option<String>,
    pub client_capabilities: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct PolicyResult {
    pub profile: String,
    pub max_bitrate: Option<u32>,
    pub allow_sync: bool,
    pub allow_stream: bool,
}

pub async fn policy_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let max_remote_bitrate: u32 = state.config.max_remote_bitrate;
    let remote_sync: bool = state.config.remote_sync;
    let allow_stream = !state.disabled_modules.read().await.contains("stream");

    let profile = "remote";
    let max_bitrate = if max_remote_bitrate > 0 {
        Some(max_remote_bitrate)
    } else {
        Some(128_000)
    };
    let allow_sync = remote_sync;

    Json(serde_json::json!({
        "profile": profile,
        "max_bitrate": max_bitrate,
        "allow_sync": allow_sync,
        "allow_stream": allow_stream,
    }))
}

pub async fn lan_policy_handler(
    State(state): State<AppState>,
    connect_info: Option<axum::extract::ConnectInfo<std::net::SocketAddr>>,
    headers: axum::http::HeaderMap,
    Json(_query): Json<PolicyQuery>,
) -> Json<serde_json::Value> {
    let resolved_ip_str = crate::extract_client_ip(connect_info, &headers, &state.config);
    let is_lan = is_lan_ip(&resolved_ip_str);
    let profile = if is_lan { "lan" } else { "remote" };
    let max_remote_bitrate: u32 = state.config.max_remote_bitrate;
    let max_bitrate = if is_lan {
        None
    } else if max_remote_bitrate > 0 {
        Some(max_remote_bitrate)
    } else {
        Some(128_000)
    };
    let allow_sync = if is_lan {
        true
    } else {
        state.config.remote_sync
    };
    let allow_stream = !state.disabled_modules.read().await.contains("stream");

    Json(serde_json::json!({
        "profile": profile,
        "max_bitrate": max_bitrate,
        "allow_sync": allow_sync,
        "allow_stream": allow_stream,
        "client_ip": resolved_ip_str,
    }))
}

// ── Direct Stream Handoff ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct HandoffOffer {
    pub target_peer: Option<String>,
    pub track_id: String,
    pub session_id: Option<String>,
}

use std::sync::LazyLock;

type HandoffEntry = (String, String, u64, std::time::Instant);
type HandoffTokenMap = Arc<RwLock<HashMap<String, HandoffEntry>>>;

static HANDOFF_TOKENS: LazyLock<HandoffTokenMap> =
    LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

pub async fn handoff_handler(Json(body): Json<HandoffOffer>) -> Json<serde_json::Value> {
    let handoff_token = uuid::Uuid::new_v4().to_string();
    HANDOFF_TOKENS.write().await.insert(
        handoff_token.clone(),
        (
            body.track_id.clone(),
            body.target_peer.unwrap_or_default(),
            body.session_id.clone().unwrap_or_default().len() as u64,
            std::time::Instant::now() + std::time::Duration::from_secs(30),
        ),
    );
    Json(serde_json::json!({
        "handoff_token": handoff_token,
        "track_id": body.track_id,
        "ttl_seconds": 30,
        "endpoint": "/api/v1/stream/handoff",
    }))
}

// ── ETag ───────────────────────────────────────────────────────

lazy_static::lazy_static! {
    static ref ETAG_STORE: Arc<RwLock<HashMap<String, String>>> =
        Arc::new(RwLock::new(HashMap::new()));
}

pub async fn set_resource_etag(resource: &str) -> String {
    let etag = format!("\"{}\"", uuid::Uuid::new_v4());
    ETAG_STORE
        .write()
        .await
        .insert(resource.to_string(), etag.clone());
    etag
}

pub async fn get_resource_etag(resource: &str) -> Option<String> {
    ETAG_STORE.read().await.get(resource).cloned()
}

pub async fn check_etag(resource: &str, if_none_match: &str) -> bool {
    if let Some(current) = get_resource_etag(resource).await {
        if_none_match == current || if_none_match == "*"
    } else {
        false
    }
}
