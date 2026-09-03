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
    pub desired_state: String,
    pub actual_state: String,
    pub health: String,
    pub generation: u64,
    pub last_error: Option<String>,
}

fn builtin_modules() -> Vec<ModuleDescriptor> {
    vec![
        ModuleDescriptor {
            name: "scan".into(),
            enabled: true,
            description: "Music library scanning and filesystem watcher".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
        ModuleDescriptor {
            name: "sync".into(),
            enabled: true,
            description: "Peer synchronization and background sync workers".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
        ModuleDescriptor {
            name: "stream".into(),
            enabled: true,
            description: "Audio streaming and transcode pipeline".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
        ModuleDescriptor {
            name: "playback".into(),
            enabled: true,
            description: "Server playback engine and tracking".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
        ModuleDescriptor {
            name: "backup".into(),
            enabled: true,
            description: "Automatic backup scheduler and retention".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
        ModuleDescriptor {
            name: "webhook".into(),
            enabled: true,
            description: "Webhook dispatch notifications".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
        ModuleDescriptor {
            name: "homeassistant".into(),
            enabled: true,
            description: "Home Assistant MQTT discovery and entity synchronization".into(),
            desired_state: "enabled".into(),
            actual_state: "active".into(),
            health: "healthy".into(),
            generation: 1,
            last_error: None,
        },
    ]
}

pub async fn modules_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let disabled = state.disabled_modules.read().await;
    let runtime = state.module_runtime_info.read().await;
    let mut modules = builtin_modules();
    for m in &mut modules {
        let is_disabled = disabled.contains(&m.name);
        m.enabled = !is_disabled;
        m.desired_state = if is_disabled { "disabled" } else { "enabled" }.to_string();
        if let Some(r) = runtime.get(&m.name) {
            m.actual_state = r.actual_state.clone();
            m.health = r.health.clone();
            m.generation = r.generation;
            m.last_error = r.last_error.clone();
        } else {
            m.actual_state = if is_disabled { "disabled" } else { "active" }.to_string();
            m.health = if is_disabled { "disabled" } else { "healthy" }.to_string();
            m.generation = 1;
            m.last_error = None;
        }
    }
    Json(serde_json::json!({ "modules": modules }))
}

#[derive(Debug, Deserialize)]
pub struct ToggleModuleBody {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub struct ToggleModulePathBody {
    #[serde(default)]
    pub name: Option<String>,
    pub enabled: bool,
}

pub async fn toggle_module_path_handler(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<ToggleModulePathBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    toggle_module_handler(
        State(state),
        Json(ToggleModuleBody {
            name,
            enabled: body.enabled,
        }),
    )
    .await
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

    // Per-module transition lock: serializes transitions for this module without blocking unrelated modules or holding global locks during I/O
    let module_lock = state.get_module_transition_lock(&body.name).await;
    let _guard = module_lock.lock().await;

    let is_currently_disabled = state.disabled_modules.read().await.contains(&body.name);

    if body.enabled {
        // Idempotency: If already enabled (ON -> ON), it is a safe NO-OP
        if !is_currently_disabled {
            tracing::debug!("module '{}' is already enabled (no-op)", body.name);
            let r = state.module_runtime_info.read().await;
            let (act, hlth, gen, err) = r
                .get(&body.name)
                .map(|info| {
                    (
                        info.actual_state.clone(),
                        info.health.clone(),
                        info.generation,
                        info.last_error.clone(),
                    )
                })
                .unwrap_or_else(|| ("active".into(), "healthy".into(), 1, None));
            return Ok(Json(serde_json::json!({
                "module": body.name,
                "enabled": true,
                "desired_state": "enabled",
                "actual_state": act,
                "health": hlth,
                "generation": gen,
                "last_error": err,
            })));
        }

        // Transition OFF -> ON: remove from disabled_modules and create fresh token
        state.disabled_modules.write().await.remove(&body.name);
        let new_token = tokio_util::sync::CancellationToken::new();
        state
            .module_tokens
            .write()
            .await
            .insert(body.name.clone(), new_token.clone());

        // Update runtime tracking
        {
            let mut runtime = state.module_runtime_info.write().await;
            let entry =
                runtime
                    .entry(body.name.clone())
                    .or_insert_with(|| crate::ModuleRuntimeInfo {
                        generation: 0,
                        actual_state: "active".into(),
                        health: "healthy".into(),
                        last_error: None,
                    });
            entry.generation += 1;
            if body.name == "homeassistant" && std::env::var("MICHI_MQTT_HOST").is_err() {
                entry.actual_state = "idle".into();
                entry.health = "disabled".into();
                entry.last_error = Some("MICHI_MQTT_HOST not set".into());
            } else {
                entry.actual_state = "active".into();
                entry.health = "healthy".into();
                entry.last_error = None;
            }
        }

        // Dynamic worker restart on OFF -> ON
        if body.name == "homeassistant" && std::env::var("MICHI_MQTT_HOST").is_ok() {
            let ha_config = state.config.clone();
            let ha_engine = state.playback_engine.clone();
            let ha_db = state.db.clone();
            let ha_dm = state.disabled_modules.clone();
            let ha_cancel = new_token;
            let handle = tokio::spawn(async move {
                tokio::select! {
                    _ = ha_cancel.cancelled() => {
                        tracing::info!("homeassistant module cancelled, HA stopped");
                    }
                    _ = async {
                        if ha_dm.read().await.contains("homeassistant") {
                            return;
                        }
                        michi_homeassistant::run(ha_config, ha_engine, ha_db).await;
                    } => {}
                }
            });
            state.track_task(handle);
            tracing::info!("homeassistant worker spawned on module enable");
        } else if body.name == "sync" {
            crate::start_sync_peers(&state, new_token.clone());
            tracing::info!("sync peers worker started on module enable");
        }
        tracing::info!("module '{}' enabled", body.name);
    } else {
        // Idempotency: If already disabled (OFF -> OFF), it is a safe NO-OP
        if is_currently_disabled {
            tracing::debug!("module '{}' is already disabled (no-op)", body.name);
            let r = state.module_runtime_info.read().await;
            let (act, hlth, gen, err) = r
                .get(&body.name)
                .map(|info| {
                    (
                        info.actual_state.clone(),
                        info.health.clone(),
                        info.generation,
                        info.last_error.clone(),
                    )
                })
                .unwrap_or_else(|| ("disabled".into(), "disabled".into(), 1, None));
            return Ok(Json(serde_json::json!({
                "module": body.name,
                "enabled": false,
                "desired_state": "disabled",
                "actual_state": act,
                "health": hlth,
                "generation": gen,
                "last_error": err,
            })));
        }

        // Transition ON -> OFF: add to disabled_modules and cancel existing token
        state
            .disabled_modules
            .write()
            .await
            .insert(body.name.clone());
        if let Some(token) = state.module_tokens.read().await.get(&body.name) {
            token.cancel();
            tracing::info!("module '{}' disabled, tasks cancelled", body.name);
        }

        // Update runtime tracking on OFF
        {
            let mut runtime = state.module_runtime_info.write().await;
            let entry =
                runtime
                    .entry(body.name.clone())
                    .or_insert_with(|| crate::ModuleRuntimeInfo {
                        generation: 1,
                        actual_state: "disabled".into(),
                        health: "disabled".into(),
                        last_error: None,
                    });
            entry.actual_state = "disabled".into();
            entry.health = "disabled".into();
            entry.last_error = None;
        }

        // Stop and neutralize PlaybackEngine when Playback module is toggled OFF
        if body.name == "playback" {
            let mut stop_err = None;
            if let Err(e) = state.playback_engine.stop().await {
                tracing::error!("failed to stop playback engine when disabling module: {e}");
                stop_err = Some(e.to_string());
            }
            if let Err(e) = state.playback_engine.set_queue(Vec::new(), 0, None).await {
                tracing::error!("failed to clear engine queue when disabling module: {e}");
                if stop_err.is_none() {
                    stop_err = Some(e.to_string());
                }
            }
            *state.playback_output_selection.write().await = None;
            if let Some(err) = stop_err {
                let mut runtime = state.module_runtime_info.write().await;
                if let Some(entry) = runtime.get_mut("playback") {
                    entry.actual_state = "degraded".into();
                    entry.health = "degraded".into();
                    entry.last_error = Some(err);
                }
            }
            tracing::info!("playback module disabled: playback engine stopped and neutralized");
        }
    }

    let (actual_state, health, generation, last_error) = {
        let r = state.module_runtime_info.read().await;
        if let Some(info) = r.get(&body.name) {
            (
                info.actual_state.clone(),
                info.health.clone(),
                info.generation,
                info.last_error.clone(),
            )
        } else {
            (
                if body.enabled {
                    "active".into()
                } else {
                    "disabled".into()
                },
                if body.enabled {
                    "healthy".into()
                } else {
                    "disabled".into()
                },
                1,
                None,
            )
        }
    };

    Ok(Json(serde_json::json!({
        "module": body.name,
        "enabled": body.enabled,
        "desired_state": if body.enabled { "enabled" } else { "disabled" },
        "actual_state": actual_state,
        "health": health,
        "generation": generation,
        "last_error": last_error,
    })))
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
    let sync_module_enabled = !state.disabled_modules.read().await.contains("sync");
    let allow_stream = !state.disabled_modules.read().await.contains("stream");

    let profile = "remote";
    let max_bitrate = if max_remote_bitrate > 0 {
        Some(max_remote_bitrate)
    } else {
        Some(128_000)
    };
    let allow_sync = sync_module_enabled && remote_sync;

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
    let sync_module_enabled = !state.disabled_modules.read().await.contains("sync");
    let allow_sync = sync_module_enabled && (is_lan || state.config.remote_sync);
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
