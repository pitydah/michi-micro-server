use axum::{
    extract::{Query, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};

use crate::auth::check_auth;
use crate::errors;
use crate::models::{json_err, json_ok, SubsonicQuery, SubsonicResponse};

#[derive(Clone)]
pub struct OsAppState {
    pub db: sqlx::SqlitePool,
    pub music_paths: Vec<std::path::PathBuf>,
    pub cache_path: std::path::PathBuf,
}

pub fn router(state: OsAppState) -> Router {
    Router::new()
        .route("/rest/ping", get(ping))
        .route("/rest/getLicense", get(get_license))
        .route("/rest/getMusicFolders", get(get_music_folders))
        .route("/rest/getArtists", get(get_artists))
        .route("/rest/getArtist", get(get_artist))
        .route("/rest/getAlbum", get(get_album))
        .route("/rest/getSong", get(get_song))
        .route("/rest/search3", get(search3))
        .route("/rest/stream", get(stream))
        .route("/rest/download", get(download))
        .route("/rest/getCoverArt", get(get_cover_art))
        .route("/rest/getLyrics", get(get_lyrics))
        .route("/rest/getPlaylists", get(get_playlists))
        .route("/rest/getPlaylist", get(get_playlist))
        .route("/rest/scrobble", get(scrobble))
        .route("/rest/star", get(star))
        .route("/rest/unstar", get(unstar))
        .route("/rest/startScan", get(start_scan))
        .route("/rest/getScanStatus", get(get_scan_status))
        .with_state(state)
}

async fn ping(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Ok(json_ok(None))
}

async fn get_license(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Ok(json_ok(Some(json!({
        "license": {
            "valid": true,
            "email": "",
            "licenseExpires": "",
            "trialExpires": "",
            "isOffline": false
        }
    }))))
}

async fn get_music_folders(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let folders: Vec<Value> = state
        .music_paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            json!({
                "id": i + 1,
                "name": p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default()
            })
        })
        .collect();
    Ok(json_ok(Some(
        json!({ "musicFolders": { "musicFolder": folders } }),
    )))
}

async fn get_artists(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    match michi_db::list_artists(&state.db).await {
        Ok(artists) => {
            let indexes = build_artist_index(&artists);
            Ok(json_ok(Some(json!({ "artists": { "index": indexes } }))))
        }
        Err(e) => Err(json_err(errors::GENERIC, &format!("db error: {e}"))),
    }
}

fn build_artist_index(artists: &[michi_core::ArtistSummary]) -> Vec<Value> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for a in artists {
        let name = a.artist.as_deref().unwrap_or("Unknown");
        let first = name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "#".to_string());
        let entry = json!({
            "id": name,
            "name": name,
            "albumCount": a.track_count,
        });
        map.entry(first).or_default().push(entry);
    }
    map.into_iter()
        .map(|(key, entries)| json!({ "name": key, "artist": entries }))
        .collect()
}

async fn get_artist(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "getArtist not yet implemented",
    ))
}

async fn get_album(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "getAlbum not yet implemented",
    ))
}

async fn get_song(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "getSong not yet implemented",
    ))
}

async fn search3(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "search3 not yet implemented",
    ))
}

async fn stream(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "stream not yet implemented",
    ))
}

async fn download(
    State(_state): State<OsAppState>,
    Query(_query): Query<SubsonicQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<SubsonicResponse>)> {
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "download not yet implemented",
    ))
}

async fn get_cover_art(
    State(_state): State<OsAppState>,
    Query(_query): Query<SubsonicQuery>,
) -> Result<axum::response::Response, (StatusCode, Json<SubsonicResponse>)> {
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "getCoverArt not yet implemented",
    ))
}

async fn get_lyrics(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "getLyrics not yet implemented",
    ))
}

async fn get_playlists(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    match michi_db::list_playlists(&state.db, None).await {
        Ok(playlists) => {
            let items: Vec<Value> = playlists
                .iter()
                .map(|p| {
                    json!({
                        "id": p.id.to_string(),
                        "name": p.name,
                        "songCount": p.track_count,
                        "created": p.created_at.to_rfc3339(),
                    })
                })
                .collect();
            Ok(json_ok(Some(json!({ "playlists": { "playlist": items } }))))
        }
        Err(e) => Err(json_err(errors::GENERIC, &format!("db error: {e}"))),
    }
}

async fn get_playlist(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "getPlaylist not yet implemented",
    ))
}

async fn scrobble(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "scrobble not yet implemented",
    ))
}

async fn star(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "star not yet implemented",
    ))
}

async fn unstar(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "unstar not yet implemented",
    ))
}

async fn start_scan(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Err(json_err(
        errors::NOT_IMPLEMENTED,
        "startScan not yet implemented",
    ))
}

async fn get_scan_status(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let count = michi_db::count_tracks(&state.db).await.unwrap_or(0);
    Ok(json_ok(Some(json!({
        "scanStatus": {
            "scanning": false,
            "count": count,
            "folderCount": state.music_paths.len() as i64,
        }
    }))))
}
