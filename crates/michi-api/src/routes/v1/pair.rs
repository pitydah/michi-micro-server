use axum::{extract::State, http::StatusCode, Json};
use serde::Deserialize;
use uuid::Uuid;

use crate::AppState;
use michi_link::{
    generate_device_token, hash_token,
    models::{
        PairConfirmRequest, PairConfirmResponse, PairStartResponse, TokenRefreshRequest,
        TokenRefreshResponse,
    },
    DeviceEntry, TokenType,
};

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

fn v1_internal_error(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    v1_error(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", msg)
}

#[derive(Debug, Deserialize)]
pub struct PairStartBody {
    pub device_name: Option<String>,
    pub alias: Option<String>,
    pub device_type: Option<String>,
    pub device_model: Option<String>,
    pub client_device_id: Option<String>,
}

pub async fn link_pair_start(
    State(state): State<AppState>,
    Json(body): Json<PairStartBody>,
) -> Result<Json<PairStartResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pairing_id = Uuid::new_v4();
    let code = michi_link::generate_pairing_code();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    let device_name = body
        .alias
        .as_deref()
        .or(body.device_name.as_deref())
        .unwrap_or("unknown")
        .to_string();
    let device_type = body.device_type.unwrap_or_else(|| "unknown".into());

    let session = michi_core::PairingSessionDb {
        pairing_id,
        code: code.clone(),
        device_name,
        device_type,
        expires_at: expires_at.to_rfc3339(),
        confirmed: false,
    };

    michi_db::create_pairing_session(&state.db, &session)
        .await
        .map_err(|e| v1_internal_error(&e.to_string()))?;

    Ok(Json(PairStartResponse {
        pairing_id,
        code,
        expires_at: expires_at.to_rfc3339(),
    }))
}

pub async fn link_pair_confirm(
    State(state): State<AppState>,
    Json(body): Json<PairConfirmRequest>,
) -> Result<Json<PairConfirmResponse>, (StatusCode, Json<serde_json::Value>)> {
    let session = michi_db::get_pairing_session_by_code(&state.db, &body.code)
        .await
        .map_err(|e| v1_internal_error(&e.to_string()))?
        .ok_or_else(|| {
            v1_error(
                StatusCode::NOT_FOUND,
                "INVALID_CODE",
                "pairing code not found or expired",
            )
        })?;

    if session.confirmed {
        return Err(v1_error(
            StatusCode::CONFLICT,
            "ALREADY_CONFIRMED",
            "pairing already confirmed",
        ));
    }

    let device_token = generate_device_token();
    let refresh_token = generate_device_token();
    let device_id = Uuid::new_v4();
    let token_hash = hash_token(&device_token);

    let device_entry = DeviceEntry::new(
        device_id,
        session.device_name.clone(),
        session.device_type.clone(),
        None,
        token_hash,
    );

    let core_device = michi_core::LinkDevice {
        device_id,
        alias: session.device_name.clone(),
        device_type: session.device_type.clone(),
        device_model: None,
        token_hash: hash_token(&device_token),
        permissions_json: serde_json::to_string(&device_entry.permissions).unwrap_or_default(),
        created_at: chrono::Utc::now(),
        last_seen: Some(chrono::Utc::now().to_rfc3339()),
        revoked: false,
    };

    michi_db::create_link_device(&state.db, &core_device)
        .await
        .map_err(|e| v1_internal_error(&e.to_string()))?;

    michi_db::confirm_pairing_session(&state.db, &session.pairing_id)
        .await
        .ok();

    state
        .token_store
        .store(&device_token, TokenType::Device, device_id)
        .await;
    state
        .token_store
        .store(&refresh_token, TokenType::Refresh, device_id)
        .await;

    let permissions = device_entry.permissions.to_canonical_strings();

    Ok(Json(PairConfirmResponse {
        device_token,
        refresh_token,
        device_id,
        alias: session.device_name,
        permissions,
    }))
}

pub async fn link_token_refresh(
    State(state): State<AppState>,
    Json(body): Json<TokenRefreshRequest>,
) -> Result<Json<TokenRefreshResponse>, (StatusCode, Json<serde_json::Value>)> {
    let device_id = match state
        .token_store
        .validate(&body.refresh_token, TokenType::Refresh)
        .await
    {
        Ok(id) => id,
        Err(_) => {
            return Err(v1_error(
                StatusCode::UNAUTHORIZED,
                "INVALID_TOKEN",
                "invalid or expired refresh token",
            ));
        }
    };

    let req_device_id = body.device_id.or_else(|| {
        body.client_device_id
            .as_ref()
            .and_then(|s| Uuid::parse_str(s).ok())
    });

    if let Some(req_id) = req_device_id {
        if req_id != device_id {
            return Err(v1_error(
                StatusCode::FORBIDDEN,
                "DEVICE_MISMATCH",
                "device id does not match token",
            ));
        }
    }

    let new_device_token = generate_device_token();
    let new_refresh_token = generate_device_token();

    state
        .token_store
        .store(&new_device_token, TokenType::Device, device_id)
        .await;
    state
        .token_store
        .store(&new_refresh_token, TokenType::Refresh, device_id)
        .await;

    Ok(Json(TokenRefreshResponse {
        device_token: new_device_token,
        refresh_token: new_refresh_token,
    }))
}

pub async fn link_devices_revoke(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let device_id = body
        .get("device_id")
        .and_then(|v| v.as_str())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or_else(|| {
            v1_error(
                StatusCode::BAD_REQUEST,
                "INVALID_REQUEST",
                "device_id is required",
            )
        })?;

    let revoked = michi_db::revoke_link_device(&state.db, &device_id)
        .await
        .map_err(|e| v1_internal_error(&e.to_string()))?;

    if !revoked {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "device not found",
        ));
    }

    state.token_store.revoke_all_by_device(device_id).await;

    Ok(Json(serde_json::json!({ "status": "revoked" })))
}
