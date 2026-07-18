use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};

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

pub async fn artist_insights_handler(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let decoded = urlencoding::decode(&name).map_err(|_| {
        v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ARTIST_NAME",
            "url decode failed",
        )
    })?;
    let artist = decoded.to_string();

    let track_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE artist = ?")
        .bind(&artist)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let album_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(DISTINCT album) FROM tracks WHERE artist = ? AND album IS NOT NULL AND album != ''"
    )
        .bind(&artist)
        .fetch_one(&state.db).await.unwrap_or(0);

    let lossless_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE artist = ? AND format IN ('FLAC', 'ALAC', 'WAV', 'AIFF', 'DSF', 'DFF')"
    )
        .bind(&artist)
        .fetch_one(&state.db).await.unwrap_or(0);

    let hi_res_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE artist = ? AND bit_depth > 16 AND bit_depth IS NOT NULL",
    )
    .bind(&artist)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let earliest_year: Option<i32> =
        sqlx::query_scalar("SELECT MIN(year) FROM tracks WHERE artist = ? AND year IS NOT NULL")
            .bind(&artist)
            .fetch_one(&state.db)
            .await
            .unwrap_or(None);

    let latest_year: Option<i32> =
        sqlx::query_scalar("SELECT MAX(year) FROM tracks WHERE artist = ? AND year IS NOT NULL")
            .bind(&artist)
            .fetch_one(&state.db)
            .await
            .unwrap_or(None);

    let total_duration_ms: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE artist = ?")
            .bind(&artist)
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

    // Top tracks (most played from history)
    let top_tracks: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT t.title, t.id, COUNT(h.track_id) as plays
         FROM tracks t LEFT JOIN play_history h ON t.id = h.track_id
         WHERE t.artist = ?
         GROUP BY t.id ORDER BY plays DESC LIMIT 10",
    )
    .bind(&artist)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    // Genres
    let genres: Vec<(String, i64)> = sqlx::query_as(
        "SELECT genre, COUNT(*) as cnt FROM tracks WHERE artist = ? AND genre IS NOT NULL AND genre != '' GROUP BY genre ORDER BY cnt DESC"
    )
        .bind(&artist)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let active_years = match (earliest_year, latest_year) {
        (Some(e), Some(l)) if e != l => format!("{}–{}", e, l),
        (Some(e), _) => format!("{}", e),
        _ => "Unknown".to_string(),
    };

    let lossless_pct = if track_count > 0 {
        (lossless_count as f64 / track_count as f64 * 100.0).round() as i64
    } else {
        0
    };

    Ok(Json(serde_json::json!({
        "artist": artist,
        "track_count": track_count,
        "album_count": album_count,
        "lossless_count": lossless_count,
        "hi_res_count": hi_res_count,
        "lossless_percentage": lossless_pct,
        "total_duration_ms": total_duration_ms,
        "active_years": active_years,
        "earliest_year": earliest_year,
        "latest_year": latest_year,
        "genres": genres.into_iter().map(|(g, c)| serde_json::json!({"genre": g, "count": c})).collect::<Vec<_>>(),
        "top_tracks": top_tracks.into_iter().map(|(title, id, plays)| serde_json::json!({"title": title, "id": id, "plays": plays})).collect::<Vec<_>>(),
    })))
}

pub async fn album_health_handler(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let decoded = urlencoding::decode(&key).map_err(|_| {
        v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_ALBUM_KEY",
            "url decode failed",
        )
    })?;
    let parts: Vec<&str> = decoded.splitn(2, "|||").collect();
    if parts.len() != 2 {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "INVALID_FORMAT",
            "use artist|||album format",
        ));
    }
    let album_artist = parts[0];
    let album = parts[1];

    let track_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM tracks WHERE album = ? AND album_artist = ?")
            .bind(album)
            .bind(album_artist)
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

    let missing_cover: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE album = ? AND album_artist = ? AND artwork_id IS NULL",
    )
    .bind(album)
    .bind(album_artist)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let lossless_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE album = ? AND album_artist = ? AND format IN ('FLAC', 'ALAC', 'WAV', 'AIFF', 'DSF', 'DFF')"
    )
        .bind(album).bind(album_artist)
        .fetch_one(&state.db).await.unwrap_or(0);

    let hi_res_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE album = ? AND album_artist = ? AND bit_depth > 16 AND bit_depth IS NOT NULL"
    )
        .bind(album).bind(album_artist)
        .fetch_one(&state.db).await.unwrap_or(0);

    let total_duration_ms: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(duration_ms), 0) FROM tracks WHERE album = ? AND album_artist = ?",
    )
    .bind(album)
    .bind(album_artist)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let missing_year: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM tracks WHERE album = ? AND album_artist = ? AND year IS NULL",
    )
    .bind(album)
    .bind(album_artist)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // Format breakdown
    let formats: Vec<(String, i64)> = sqlx::query_as(
        "SELECT format, COUNT(*) as cnt FROM tracks WHERE album = ? AND album_artist = ? GROUP BY format ORDER BY cnt DESC"
    )
        .bind(album).bind(album_artist)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    let lossless_pct = if track_count > 0 {
        (lossless_count as f64 / track_count as f64 * 100.0).round() as i64
    } else {
        0
    };

    let issues: Vec<String> = {
        let mut v = Vec::new();
        if missing_cover > 0 {
            v.push(format!("{} tracks missing cover art", missing_cover));
        }
        if missing_year > 0 {
            v.push(format!("{} tracks missing year", missing_year));
        }
        v
    };

    Ok(Json(serde_json::json!({
        "album": album,
        "artist": album_artist,
        "track_count": track_count,
        "lossless_count": lossless_count,
        "hi_res_count": hi_res_count,
        "lossless_percentage": lossless_pct,
        "total_duration_ms": total_duration_ms,
        "missing_cover": missing_cover,
        "missing_year": missing_year,
        "formats": formats.into_iter().map(|(f, c)| serde_json::json!({"format": f, "count": c})).collect::<Vec<_>>(),
        "issues": issues,
        "health_score": if issues.is_empty() { 100 } else { (100.0 - (issues.len() as f64 / 3.0 * 100.0)).round() as i64 },
    })))
}
