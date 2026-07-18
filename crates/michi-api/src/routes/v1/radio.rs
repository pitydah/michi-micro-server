use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
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

pub async fn list_radio_stations_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let stations = michi_db::list_radio_stations(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "stations": stations })))
}

#[derive(Debug, Deserialize)]
pub struct CreateRadioBody {
    pub name: String,
    pub stream_url: String,
    pub homepage: Option<String>,
    pub icon: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
}

pub async fn create_radio_station_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateRadioBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.name.trim().is_empty() || body.stream_url.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "name and stream_url are required",
        ));
    }
    if !body.stream_url.starts_with("http://") && !body.stream_url.starts_with("https://") {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION_ERROR",
            "stream_url must start with http:// or https://",
        ));
    }
    let id = Uuid::new_v4();
    let station = michi_core::RadioStation {
        id,
        name: body.name.trim().to_string(),
        stream_url: body.stream_url.trim().to_string(),
        homepage: body.homepage.map(|s| s.trim().to_string()),
        icon: body.icon.map(|s| s.trim().to_string()),
        codec: body.codec.map(|s| s.trim().to_string()),
        bitrate: body.bitrate,
        last_checked: None,
        enabled: true,
        favorite: false,
    };
    michi_db::create_radio_station(&state.db, &id, &station)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "station": station })))
}

#[derive(Debug, Deserialize)]
pub struct UpdateRadioBody {
    pub name: Option<String>,
    pub stream_url: Option<String>,
    pub homepage: Option<String>,
    pub icon: Option<String>,
    pub codec: Option<String>,
    pub bitrate: Option<u32>,
    pub enabled: Option<bool>,
    pub favorite: Option<bool>,
}

pub async fn update_radio_station_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateRadioBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let existing = michi_db::list_radio_stations(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    let old = existing.into_iter().find(|s| s.id == id).ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "radio station not found",
        )
    })?;

    let station = michi_core::RadioStation {
        id,
        name: body.name.unwrap_or(old.name),
        stream_url: body.stream_url.unwrap_or(old.stream_url),
        homepage: body.homepage.or(old.homepage),
        icon: body.icon.or(old.icon),
        codec: body.codec.or(old.codec),
        bitrate: body.bitrate.or(old.bitrate),
        last_checked: old.last_checked,
        enabled: body.enabled.unwrap_or(old.enabled),
        favorite: body.favorite.unwrap_or(old.favorite),
    };
    let updated = michi_db::update_radio_station(&state.db, &id, &station)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    if !updated {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "radio station not found",
        ));
    }
    Ok(Json(serde_json::json!({ "station": station })))
}

pub async fn delete_radio_station_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_radio_station(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    if !deleted {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "radio station not found",
        ));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn test_radio_station_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let stations = michi_db::list_radio_stations(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    let station = stations.into_iter().find(|s| s.id == id).ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "radio station not found",
        )
    })?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CLIENT_ERROR",
                &e.to_string(),
            )
        })?;

    match client.head(&station.stream_url).send().await {
        Ok(resp) => Ok(Json(serde_json::json!({
            "status": if resp.status().is_success() { "reachable" } else { "unreachable" },
            "http_status": resp.status().as_u16(),
        }))),
        Err(e) => Ok(Json(serde_json::json!({
            "status": "error",
            "message": e.to_string(),
        }))),
    }
}

pub async fn toggle_favorite_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let stations = michi_db::list_radio_stations(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    let old = stations.into_iter().find(|s| s.id == id).ok_or_else(|| {
        v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "radio station not found",
        )
    })?;

    let new_fav = !old.favorite;
    michi_db::toggle_radio_favorite(&state.db, &id, new_fav)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "favorite": new_fav })))
}
