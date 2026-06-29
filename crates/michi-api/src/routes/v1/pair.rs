use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::AppState;
use michi_link::{
    generate_device_token, generate_pairing_code, hash_token,
    models::{PairConfirmRequest, PairConfirmResponse, PairStartRequest, PairStartResponse, TokenRefreshRequest, TokenRefreshResponse},
    DeviceEntry,
};

pub async fn pair_start_handler(
    State(state): State<AppState>,
    Json(body): Json<PairStartRequest>,
) -> Result<Json<PairStartResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pairing_id = Uuid::new_v4();
    let code = generate_pairing_code();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    let device_type = body.device_type.unwrap_or_else(|| "unknown".into());

    let session = michi_core::PairingSessionDb {
        pairing_id,
        code: code.clone(),
        device_name: body.device_name.clone(),
        device_type,
        expires_at: expires_at.to_rfc3339(),
        confirmed: false,
    };

    michi_db::create_pairing_session(&state.db, &session)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?;

    info!("pairing session created: device={} code={}", body.device_name, code);

    Ok(Json(PairStartResponse {
        pairing_id,
        code,
        expires_at: expires_at.to_rfc3339(),
    }))
}

pub async fn pair_confirm_handler(
    State(state): State<AppState>,
    Json(body): Json<PairConfirmRequest>,
) -> Result<Json<PairConfirmResponse>, (StatusCode, Json<serde_json::Value>)> {
    let session = michi_db::get_pairing_session_by_code(&state.db, &body.code)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?
        .ok_or_else(|| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": "invalid_code",
                "message": "pairing code not found or expired"
            })))
        })?;

    if session.confirmed {
        return Err((StatusCode::CONFLICT, Json(serde_json::json!({
            "error": "already_confirmed",
            "message": "pairing already confirmed"
        }))));
    }

    let device_token = generate_device_token();
    let refresh_token = generate_device_token();
    let device_id = Uuid::new_v4();
    let token_hash = hash_token(&device_token);

    let device_type = session.device_type.clone();
    let device_name = session.device_name.clone();

    let device_entry = DeviceEntry::new(
        device_id,
        device_name.clone(),
        device_type.clone(),
        None,
        token_hash,
    );

    let core_device = michi_core::LinkDevice {
        device_id,
        alias: device_name.clone(),
        device_type: device_type.clone(),
        device_model: None,
        token_hash: hash_token(&device_token),
        permissions_json: serde_json::to_string(&device_entry.permissions).unwrap_or_default(),
        created_at: chrono::Utc::now(),
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
        revoked: false,
    };

    michi_db::create_link_device(&state.db, &core_device)
        .await
        .map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
                "error": "database_error",
                "message": e.to_string()
            })))
        })?;

    michi_db::confirm_pairing_session(&state.db, &session.pairing_id)
        .await
        .ok();

    state.token_store.store(&device_token, device_id).await;
    state.token_store.store(&refresh_token, device_id).await;

    let permissions: Vec<String> = device_entry
        .permissions
        .permissions
        .iter()
        .map(|p| format!("{:?}", p))
        .collect();

    info!("device paired: {} ({}) - {}", device_name, device_type, device_id);

    Ok(Json(PairConfirmResponse {
        device_token,
        refresh_token,
        device_id,
        alias: device_name,
        permissions,
    }))
}

pub async fn token_refresh_handler(
    State(state): State<AppState>,
    Json(body): Json<TokenRefreshRequest>,
) -> Result<Json<TokenRefreshResponse>, (StatusCode, Json<serde_json::Value>)> {
    let device_id = match state.token_store.validate(&body.refresh_token).await {
        Ok(id) => id,
        Err(_) => {
            return Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                "error": "invalid_token",
                "message": "invalid or expired refresh token"
            }))));
        }
    };

    if device_id != body.device_id {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({
            "error": "device_mismatch",
            "message": "device id does not match token"
        }))));
    }

    let new_device_token = generate_device_token();
    let new_refresh_token = generate_device_token();

    state.token_store.store(&new_device_token, device_id).await;
    state.token_store.store(&new_refresh_token, device_id).await;

    Ok(Json(TokenRefreshResponse {
        device_token: new_device_token,
        refresh_token: new_refresh_token,
    }))
}

pub async fn devices_revoke_handler(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let device_id = body.get("device_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| {
            (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid_request",
                "message": "device_id is required"
            })))
        })?;

    let revoked = michi_db::revoke_link_device(&state.db, &device_id).await.map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": "database_error",
            "message": e.to_string()
        })))
    })?;

    if !revoked {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "not_found",
            "message": "device not found"
        }))));
    }

    Ok(Json(serde_json::json!({ "status": "revoked" })))
}
