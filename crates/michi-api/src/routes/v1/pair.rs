use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
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
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<PairStartBody>,
) -> Result<Json<PairStartResponse>, (StatusCode, Json<serde_json::Value>)> {
    // Log X-Michi-Device-Id header if present (Player sends this)
    if let Some(device_id) = headers
        .get("X-Michi-Device-Id")
        .and_then(|v| v.to_str().ok())
    {
        tracing::info!("pair/start from device: {}", device_id);
    }

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
    headers: axum::http::HeaderMap,
    State(state): State<AppState>,
    Json(body): Json<PairConfirmRequest>,
) -> Result<Json<PairConfirmResponse>, (StatusCode, Json<serde_json::Value>)> {
    if let Some(device_id) = headers
        .get("X-Michi-Device-Id")
        .and_then(|v| v.to_str().ok())
    {
        tracing::info!("pair/confirm from device: {}", device_id);
    }

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

// ── QR Pairing ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct QrGenerateResponse {
    pub qr_code: Uuid,
    pub expires_at: String,
    pub svg_url: String,
}

pub async fn qr_generate_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let qr_code = Uuid::new_v4();
    let server_url = "http://localhost:".to_string() + &state.config.port().to_string();
    let expires_at = chrono::Utc::now() + chrono::Duration::minutes(5);
    sqlx::query(
        "INSERT INTO pairing_qr_codes (id, qr_code, server_url, expires_at, created_at)
         VALUES (?, ?, ?, ?, ?)"
    )
        .bind(Uuid::new_v4().to_string())
        .bind(qr_code.to_string())
        .bind(&server_url)
        .bind(expires_at.to_rfc3339())
        .bind(chrono::Utc::now().to_rfc3339())
        .execute(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    Ok(Json(serde_json::json!({
        "qr_code": qr_code,
        "expires_at": expires_at.to_rfc3339(),
        "svg_url": format!("/api/v1/pair/qr/{}/svg", qr_code),
    })))
}

pub async fn qr_svg_handler(
    State(state): State<AppState>,
    Path(qr_code): Path<Uuid>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let qr_str = qr_code.to_string();

    let row = sqlx::query_as::<_, (String, String, i64, Option<String>)>(
        "SELECT server_url, expires_at, claimed, claimed_at FROM pairing_qr_codes WHERE qr_code = ?"
    )
        .bind(&qr_str)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    let (server_url, expires_at_str, claimed, _claimed_at) = row
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "QR code not found"))?;

    if claimed != 0 {
        return Err(v1_error(StatusCode::GONE, "ALREADY_USED", "QR code has already been used"));
    }

    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at_str)
        .map_err(|_| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "PARSE_ERROR", "invalid expiry"))?;
    if expires_at < chrono::Utc::now() {
        return Err(v1_error(StatusCode::GONE, "EXPIRED", "QR code has expired"));
    }

    // Build QR content
    let payload = serde_json::json!({
        "michi": "v1",
        "url": server_url,
        "code": qr_str,
    });
    let payload_str = serde_json::to_string(&payload)
        .map_err(|_| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "JSON_ERROR", "serialization failed"))?;

    // Generate QR code
    let code = qrcode::QrCode::new(payload_str.as_bytes())
        .map_err(|_| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "QR_ERROR", "QR generation failed"))?;

    // Build QR SVG with premium styling
    let modules = code.to_vec();
    let size = code.width();
    let cell_size = 5.0;
    let qr_dim = size as f64 * cell_size;
    let padding = 40.0;
    let canvas = qr_dim + padding * 2.0;
    let corner_radius = 6.0;
    let center = padding + qr_dim / 2.0;
    let logo_size = qr_dim * 0.28;

    let mut svg = String::with_capacity(16000);
    svg.push_str(&format!(
        r#"<?xml version="1.0" standalone="yes"?><svg xmlns="http://www.w3.org/2000/svg" version="1.1" width="{c}" height="{c}" viewBox="0 0 {c} {c}">"#,
        c = canvas as u32
    ));

    // Background with rounded rect
    svg.push_str(&format!(
        r##"<rect x="0" y="0" width="{c}" height="{c}" rx="24" fill="#0D1120"/>"##,
        c = canvas as u32
    ));

    // White QR background with rounded rect
    svg.push_str(&format!(
        r##"<rect x="{p}" y="{p}" width="{d}" height="{d}" rx="{cr}" fill="#ffffff"/>"##,
        p = padding, d = qr_dim, cr = 12.0
    ));

    // Draw QR modules as circles with gradient
    svg.push_str(r#"<defs><linearGradient id="qrGrad" x1="0%" y1="0%" x2="100%" y2="100%">"#);
    svg.push_str(r##"<stop offset="0%" stop-color="#8B5CF6"/>"##);
    svg.push_str(r##"<stop offset="100%" stop-color="#6D4AFF"/>"##);
    svg.push_str("</linearGradient></defs>");

    let radius = (cell_size - 0.4) / 2.0;
    for y in 0..size {
        for x in 0..size {
            if modules[y * size + x] {
                let cx = padding + x as f64 * cell_size + cell_size / 2.0;
                let cy = padding + y as f64 * cell_size + cell_size / 2.0;
                svg.push_str(&format!(
                    r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="url(#qrGrad)"/>"##,
                    cx = cx, cy = cy, r = radius
                ));
            }
        }
    }

    // Corners: Finder patterns with rounded squares and inner circles
    let finder_positions = [(0, 0), (size - 7, 0), (0, size - 7)];
    for &(fx, fy) in &finder_positions {
        let fx = padding + fx as f64 * cell_size;
        let fy = padding + fy as f64 * cell_size;
        let f_size = 7.0 * cell_size;
        // Outer
        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="{cr}" fill="url(#qrGrad)"/>"##,
            x = fx, y = fy, s = f_size, cr = corner_radius
        ));
        // Inner
        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="{cr}" fill="#ffffff"/>"##,
            x = fx + cell_size, y = fy + cell_size, s = 5.0 * cell_size, cr = corner_radius - 1.0
        ));
        // Core
        svg.push_str(&format!(
            r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" rx="3" fill="url(#qrGrad)"/>"##,
            x = fx + 2.0 * cell_size, y = fy + 2.0 * cell_size, s = 3.0 * cell_size
        ));
    }

    // Logo circle background (white circle with subtle border)
    let white = "#ffffff";
    let dark = "#0D1120";
    svg.push_str(&format!(
        r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="{w}" stroke="url(#qrGrad)" stroke-width="3"/>"##,
        cx = center, cy = center, r = logo_size / 2.0 + 6.0, w = white
    ));

    // Logo: Michi cat silhouette
    let logo_scale = logo_size / 100.0;
    svg.push_str(&format!(
        r##"<g transform="translate({cx}, {cy}) scale({s})">
        <polygon points="-30,-35 -15,-55 0,-35" fill="url(#qrGrad)"/>
        <polygon points="30,-35 15,-55 0,-35" fill="url(#qrGrad)"/>
        <circle cx="0" cy="-10" r="25" fill="url(#qrGrad)"/>
        <ellipse cx="-10" cy="-14" rx="5" ry="6" fill="{w}"/>
        <ellipse cx="10" cy="-14" rx="5" ry="6" fill="{w}"/>
        <ellipse cx="-10" cy="-14" rx="2.5" ry="4" fill="{d}"/>
        <ellipse cx="10" cy="-14" rx="2.5" ry="4" fill="{d}"/>
        <polygon points="0,-7 -3,-3 3,-3" fill="{w}" opacity="0.8"/>
        <path d="M-6,-1 Q0,3 6,-1" fill="none" stroke="{w}" stroke-width="1.2" opacity="0.7"/>
        <path d="M-20,5 Q-25,30 -15,45 L15,45 Q25,30 20,5" fill="url(#qrGrad)"/>
        <path d="M-18,30 Q-40,20 -35,0 Q-32,-6 -28,-2" fill="none" stroke="url(#qrGrad)" stroke-width="5" stroke-linecap="round"/>
      </g>"##,
        cx = 0, cy = 0, s = logo_scale, w = white, d = dark
    ));

    svg.push_str("</svg>");

    Ok((
        [(header::CONTENT_TYPE, "image/svg+xml")],
        svg,
    ).into_response())
}

#[derive(Deserialize)]
pub struct QrClaimBody {
    pub device_name: Option<String>,
    pub device_type: Option<String>,
}

pub async fn qr_claim_handler(
    State(state): State<AppState>,
    Path(qr_code): Path<Uuid>,
    Json(body): Json<QrClaimBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let qr_str = qr_code.to_string();

    let row = sqlx::query_as::<_, (String, String, i64)>(
        "SELECT id, expires_at, claimed FROM pairing_qr_codes WHERE qr_code = ?"
    )
        .bind(&qr_str)
        .fetch_optional(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    let (db_id, expires_at_str, claimed) = row
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "QR code not found"))?;

    if claimed != 0 {
        return Err(v1_error(StatusCode::GONE, "ALREADY_USED", "QR code has already been used"));
    }

    let expires_at = chrono::DateTime::parse_from_rfc3339(&expires_at_str)
        .map_err(|_| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "PARSE_ERROR", "invalid expiry"))?;
    if expires_at < chrono::Utc::now() {
        return Err(v1_error(StatusCode::GONE, "EXPIRED", "QR code has expired"));
    }

    // Mark claimed
    sqlx::query("UPDATE pairing_qr_codes SET claimed = 1, claimed_at = ? WHERE id = ?")
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(&db_id)
        .execute(&state.db)
        .await
        .map_err(|e| v1_error(StatusCode::INTERNAL_SERVER_ERROR, "DB_ERROR", &e.to_string()))?;

    // Delete the QR code (self-destruct)
    sqlx::query("DELETE FROM pairing_qr_codes WHERE id = ?")
        .bind(&db_id)
        .execute(&state.db)
        .await
        .ok();

    // Generate device token for the claimer
    let device_name = body.device_name.unwrap_or_else(|| "QR-Paired".into());
    let device_id = Uuid::new_v4();
    let token = generate_device_token();

    state.token_store.store(
        &token,
        TokenType::Device,
        device_id,
    ).await;

    Ok(Json(serde_json::json!({
        "status": "claimed",
        "device_token": token,
        "device_id": device_id,
        "server_id": state.server_id(),
        "pairing_code": qr_str,
    })))
}
