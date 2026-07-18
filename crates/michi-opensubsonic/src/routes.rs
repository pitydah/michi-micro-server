use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

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
        .route("/rest/star", get(star_handler))
        .route("/rest/unstar", get(unstar_handler))
        .route("/rest/startScan", get(start_scan))
        .route("/rest/getScanStatus", get(get_scan_status))
        .route("/rest/setRating", get(set_rating))
        .route("/rest/getRandomSongs", get(get_random_songs))
        .route("/rest/getNowPlaying", get(get_now_playing))
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
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id = query.u.as_deref().unwrap_or("");
    let tracks = michi_db::get_artist_tracks(&state.db, id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?;

    let album_list: Vec<Value> = tracks
        .iter()
        .filter(|t| t.album.is_some())
        .map(|t| {
            json!({
                "id": t.id.to_string(),
                "name": t.album,
                "artist": t.artist,
                "songCount": 1,
            })
        })
        .collect();

    Ok(json_ok(Some(json!({
        "artist": {
            "id": id,
            "name": id,
            "albumCount": album_list.len(),
            "album": album_list,
        }
    }))))
}

async fn get_album(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id = query.u.as_deref().unwrap_or("");
    let tracks = michi_db::get_album_tracks(&state.db, id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?;

    let songs: Vec<Value> = tracks
        .iter()
        .map(|t| {
            json!({
                "id": t.id.to_string(),
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "duration": t.duration_ms.map(|d| d as i64 / 1000),
                "format": t.format.as_str(),
            })
        })
        .collect();

    Ok(json_ok(Some(json!({
        "album": {
            "id": id,
            "name": id,
            "song": songs,
        }
    }))))
}

async fn get_song(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    let id = Uuid::parse_str(id_str).map_err(|_| json_err(errors::NOT_FOUND, "invalid song id"))?;

    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?
        .ok_or_else(|| json_err(errors::NOT_FOUND, "song not found"))?;

    Ok(json_ok(Some(json!({
        "song": {
            "id": track.id.to_string(),
            "title": track.title,
            "artist": track.artist,
            "album": track.album,
            "duration": track.duration_ms.map(|d| d as i64 / 1000),
            "format": track.format.as_str(),
            "size": 0,
        }
    }))))
}

async fn search3(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let query_q = query.u.as_deref().unwrap_or("");
    if query_q.is_empty() {
        return Ok(json_ok(Some(json!({
            "searchResult3": {
                "artist": [],
                "album": [],
                "song": [],
            }
        }))));
    }

    let tracks = michi_db::search_tracks(&state.db, query_q)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?;

    let songs: Vec<Value> = tracks
        .iter()
        .map(|t| {
            json!({
                "id": t.id.to_string(),
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "duration": t.duration_ms.map(|d| d as i64 / 1000),
            })
        })
        .collect();

    Ok(json_ok(Some(json!({
        "searchResult3": {
            "artist": [],
            "album": [],
            "song": songs,
        }
    }))))
}

async fn stream(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Response, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    let id = Uuid::parse_str(id_str).map_err(|_| json_err(errors::NOT_FOUND, "invalid id"))?;

    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?
        .ok_or_else(|| json_err(errors::NOT_FOUND, "track not found"))?;

    let (path, _file) = michi_streaming::open_track_file_async(&state.music_paths, &track)
        .await
        .map_err(|_| json_err(errors::NOT_FOUND, "file not found"))?;

    let mime = track.format.mime_type();
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| json_err(errors::GENERIC, "cannot open file"))?;

    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let stream = tokio_util::io::ReaderStream::new(file);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .body(axum::body::Body::from_stream(stream))
        .unwrap())
}

async fn download(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Response, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    let id = Uuid::parse_str(id_str).map_err(|_| json_err(errors::NOT_FOUND, "invalid id"))?;

    let track = michi_db::get_track(&state.db, &id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?
        .ok_or_else(|| json_err(errors::NOT_FOUND, "track not found"))?;

    let (path, _file) = michi_streaming::open_track_file_async(&state.music_paths, &track)
        .await
        .map_err(|_| json_err(errors::NOT_FOUND, "file not found"))?;

    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| json_err(errors::GENERIC, "cannot open file"))?;

    let file_size = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let filename = format!("{}.{}", track.id, track.format.as_str());
    let stream = tokio_util::io::ReaderStream::new(file);

    Ok(Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .header(header::CONTENT_LENGTH, file_size.to_string())
        .body(axum::body::Body::from_stream(stream))
        .unwrap())
}

async fn get_cover_art(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Response, (StatusCode, Json<SubsonicResponse>)> {
    let id_str = query.u.as_deref().unwrap_or("");
    let id = Uuid::parse_str(id_str).map_err(|_| json_err(errors::NOT_FOUND, "invalid id"))?;

    // Try to find the track and extract artwork
    let track = match michi_db::get_track(&state.db, &id).await {
        Ok(Some(t)) => t,
        _ => return Err(json_err(errors::NOT_FOUND, "track not found")),
    };

    let cache_path = state.cache_path.join("artwork");
    let artwork_path = cache_path.join(id.to_string());

    if artwork_path.exists() {
        let data = tokio::fs::read(&artwork_path).await.unwrap_or_default();
        let mime = infer::get(&data)
            .map(|t| t.mime_type())
            .unwrap_or("image/jpeg");
        return Ok(([(header::CONTENT_TYPE, mime)], data).into_response());
    }

    // Try to extract from file
    let file_path = std::path::Path::new(&track.file_path);
    let path = if file_path.is_absolute() && file_path.exists() {
        file_path.to_path_buf()
    } else {
        match state.music_paths.iter().find_map(|p| {
            let full = p.join(file_path);
            if full.exists() {
                Some(full)
            } else {
                None
            }
        }) {
            Some(p) => p,
            None => return Err(json_err(errors::NOT_FOUND, "no artwork found")),
        }
    };

    if let Ok(data) = michi_metadata::extract_artwork(&path) {
        let _ = tokio::fs::create_dir_all(&cache_path).await;
        let _ = tokio::fs::write(&artwork_path, &data).await;
        let mime = infer::get(&data)
            .map(|t| t.mime_type())
            .unwrap_or("image/jpeg");
        return Ok(([(header::CONTENT_TYPE, mime)], data).into_response());
    }

    Err(json_err(errors::NOT_FOUND, "no artwork found"))
}

async fn get_lyrics(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Ok(json_ok(Some(json!({
        "lyrics": {
            "artist": query.u.as_deref().unwrap_or(""),
            "title": "",
            "value": "",
        }
    }))))
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
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    let id =
        Uuid::parse_str(id_str).map_err(|_| json_err(errors::NOT_FOUND, "invalid playlist id"))?;

    let playlist = michi_db::get_playlist(&state.db, &id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?
        .ok_or_else(|| json_err(errors::NOT_FOUND, "playlist not found"))?;

    let tracks = michi_db::get_playlist_tracks(&state.db, &id)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?;

    let entries: Vec<Value> = tracks
        .iter()
        .map(|(_, t)| {
            json!({
                "id": t.id.to_string(),
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "duration": t.duration_ms.map(|d| d as i64 / 1000),
            })
        })
        .collect();

    Ok(json_ok(Some(json!({
        "playlist": {
            "id": playlist.id.to_string(),
            "name": playlist.name,
            "songCount": playlist.track_count,
            "entry": entries,
        }
    }))))
}

async fn scrobble(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    let id = Uuid::parse_str(id_str).map_err(|_| json_err(errors::NOT_FOUND, "invalid id"))?;

    let _ = michi_db::record_play(&state.db, &id, None, &chrono::Utc::now(), None).await;

    Ok(json_ok(None))
}

async fn star_handler(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    if let Ok(id) = Uuid::parse_str(id_str) {
        let _ = michi_db::star_track(&state.db, &id, true).await;
    }
    Ok(json_ok(None))
}

async fn unstar_handler(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    if let Ok(id) = Uuid::parse_str(id_str) {
        let _ = michi_db::star_track(&state.db, &id, false).await;
    }
    Ok(json_ok(None))
}

async fn set_rating(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let id_str = query.u.as_deref().unwrap_or("");
    if let (Ok(id), Some(rating_str)) = (Uuid::parse_str(id_str), query.t.as_ref()) {
        if let Ok(rating) = rating_str.parse::<u8>() {
            let _ = michi_db::rate_track(&state.db, &id, rating).await;
        }
    }
    Ok(json_ok(None))
}

async fn get_random_songs(
    State(state): State<OsAppState>,
    Query(_query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    let tracks = michi_db::list_tracks(&state.db)
        .await
        .map_err(|e| json_err(errors::GENERIC, &format!("db error: {e}")))?;

    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    let selected: Vec<Value> = tracks
        .choose_multiple(&mut rng, 10)
        .map(|t| {
            json!({
                "id": t.id.to_string(),
                "title": t.title,
                "artist": t.artist,
                "album": t.album,
                "duration": t.duration_ms.map(|d| d as i64 / 1000),
            })
        })
        .collect();

    Ok(json_ok(Some(
        json!({ "randomSongs": { "song": selected } }),
    )))
}

async fn get_now_playing(
    State(_state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    Ok(json_ok(Some(json!({ "nowPlaying": { "entry": [] } }))))
}

async fn start_scan(
    State(state): State<OsAppState>,
    Query(query): Query<SubsonicQuery>,
) -> Result<Json<SubsonicResponse>, (StatusCode, Json<SubsonicResponse>)> {
    check_auth(&query)?;
    let music_paths = state.music_paths.clone();
    let db = state.db.clone();

    tokio::spawn(async move {
        let tracks = michi_scanner::scan_directories(&music_paths).await;
        let _ = michi_db::upsert_tracks(&db, &tracks).await;
    });

    Ok(json_ok(Some(json!({
        "scanStatus": {
            "scanning": true,
            "count": 0,
        }
    }))))
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
