use crate::AppState;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;
use uuid::Uuid;

fn v1_error(s: StatusCode, c: &str, m: &str) -> (StatusCode, Json<serde_json::Value>) {
    (s, Json(serde_json::json!({"error":{"code":c,"message":m}})))
}

#[derive(Deserialize)]
pub struct AddSourceBody {
    pub url: String,
}

pub async fn add_source_handler(
    State(state): State<AppState>,
    Json(body): Json<AddSourceBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if body.url.trim().is_empty() {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "VALIDATION",
            "url is required",
        ));
    }

    let info = michi_ingest::sniff_stream(&body.url)
        .await
        .map_err(|e| v1_error(StatusCode::BAD_REQUEST, "SNIFF_ERROR", &e))?;

    if matches!(info.stream_type, michi_ingest::StreamType::Unknown) {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "UNKNOWN_STREAM",
            "could not determine stream type",
        ));
    }

    let source = michi_core::StreamSource {
        id: Uuid::new_v4(),
        url: info.url,
        stream_type: format!("{:?}", info.stream_type).to_lowercase(),
        name: info.name,
        genre: info.genre,
        description: info.description,
        logo_url: info.logo_url,
        codec: info.codec,
        enabled: true,
    };

    michi_db::add_stream_source(&state.db, &source)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;

    // If podcast, fetch and store episodes
    let mut episodes_imported = 0usize;
    let mut episodes_failed = 0usize;
    let mut feed_status = "not_applicable";

    if source.stream_type == "podcast" {
        match reqwest::get(&source.url).await {
            Ok(body) => match body.text().await {
                Ok(text) => {
                    let episodes = michi_ingest::parse_rss_episodes(&text);
                    if episodes.is_empty() {
                        feed_status = "empty_feed";
                    } else {
                        feed_status = "success";
                        for ep in episodes {
                            let db_ep = michi_core::PodcastEpisodeDb {
                                id: Uuid::new_v4(),
                                source_id: source.id,
                                title: ep.title,
                                audio_url: ep.audio_url,
                                pub_date: Some(ep.pub_date),
                                duration_secs: ep.duration_secs,
                                played: false,
                                position_ms: 0,
                            };
                            match michi_db::upsert_podcast_episode(&state.db, &db_ep).await {
                                Ok(_) => episodes_imported += 1,
                                Err(_) => episodes_failed += 1,
                            }
                        }
                    }
                }
                Err(_) => {
                    feed_status = "read_failed";
                }
            },
            Err(_) => {
                feed_status = "fetch_failed";
            }
        }
    }

    Ok(Json(serde_json::json!({
        "source": source,
        "feed_status": feed_status,
        "episodes_imported": episodes_imported,
        "episodes_failed": episodes_failed,
    })))
}

pub async fn list_sources_handler(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let sources = michi_db::list_stream_sources(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "sources": sources })))
}

pub async fn delete_source_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let deleted = michi_db::delete_stream_source(&state.db, &id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;
    if !deleted {
        return Err(v1_error(
            StatusCode::NOT_FOUND,
            "NOT_FOUND",
            "source not found",
        ));
    }
    Ok(Json(serde_json::json!({ "status": "deleted" })))
}

pub async fn get_episodes_handler(
    State(state): State<AppState>,
    Path(source_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let episodes = michi_db::list_podcast_episodes(&state.db, &source_id)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;
    Ok(Json(serde_json::json!({ "episodes": episodes })))
}

#[derive(Deserialize)]
pub struct UpdateEpisodeBody {
    pub position_ms: Option<u64>,
    pub played: Option<bool>,
}

pub async fn update_episode_handler(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateEpisodeBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    michi_db::update_episode_progress(
        &state.db,
        &id,
        body.position_ms.unwrap_or(0),
        body.played.unwrap_or(false),
    )
    .await
    .map_err(|e| {
        v1_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;
    Ok(Json(serde_json::json!({ "status": "updated" })))
}

// ── Proxy Stream ─────────────────────────────────────────────────

pub async fn proxy_stream_handler(
    Path(source_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let sources = michi_db::list_stream_sources(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;

    let source = sources
        .into_iter()
        .find(|s| s.id == source_id)
        .ok_or_else(|| v1_error(StatusCode::NOT_FOUND, "NOT_FOUND", "source not found"))?;

    if !source.enabled {
        return Err(v1_error(
            StatusCode::BAD_REQUEST,
            "DISABLED",
            "source is disabled",
        ));
    }

    ssrf_proxy_fetch(&source.url).await
}

/// Proxy a podcast episode audio URL
pub async fn proxy_episode_handler(
    Path(episode_id): Path<Uuid>,
    State(state): State<AppState>,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    let sources = michi_db::list_stream_sources(&state.db)
        .await
        .map_err(|e| {
            v1_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?;

    for source in &sources {
        if let Ok(eps) = michi_db::list_podcast_episodes(&state.db, &source.id).await {
            if let Some(ep) = eps.into_iter().find(|e| e.id == episode_id) {
                return ssrf_proxy_fetch(&ep.audio_url).await;
            }
        }
    }

    Err(v1_error(
        StatusCode::NOT_FOUND,
        "NOT_FOUND",
        "episode not found",
    ))
}

/// Fetch a URL with full SSRF protection via a manual DNS-pinned redirect loop.
///
/// Safety guarantees:
/// - Each hop calls `validate_url_and_resolve()` which resolves the hostname and checks
///   all returned IPs against the private/reserved range blocklist.
/// - The resolved, validated socket address is pinned into the `reqwest::Client` via
///   `.resolve(host, validated_addr)` so reqwest performs **no** OS-level DNS lookup.
/// - Automatic redirects are disabled (`Policy::none()`). The `Location` header from
///   any 3xx response is extracted and re-validated before following — each hop gets
///   its own fresh client with its own pinned address.
/// - This closes the DNS rebinding TOCTOU: validate-then-pin is atomic for each hop.
async fn ssrf_proxy_fetch(
    initial_url: &str,
) -> Result<axum::response::Response, (StatusCode, Json<serde_json::Value>)> {
    const MAX_HOPS: usize = 5;

    let mut current_url = initial_url.to_string();

    for hop in 0..MAX_HOPS {
        // Step 1: resolve + validate (DNS #1 per hop — this is the only lookup).
        let (parsed_url, addrs) =
            michi_ingest::validate_url_and_resolve(&current_url).map_err(|e| {
                v1_error(
                    StatusCode::BAD_REQUEST,
                    "SSRF_BLOCKED",
                    &format!("hop {hop}: {e}"),
                )
            })?;

        let host = parsed_url
            .host_str()
            .ok_or_else(|| v1_error(StatusCode::BAD_REQUEST, "SSRF_BLOCKED", "URL has no host"))?
            .to_string();

        let first_addr = *addrs.first().ok_or_else(|| {
            v1_error(
                StatusCode::BAD_REQUEST,
                "SSRF_BLOCKED",
                "DNS resolution returned no addresses",
            )
        })?;

        // Step 2: build a per-hop client with the validated IP pinned.
        // `redirect(Policy::none())` ensures reqwest never follows redirects automatically
        // and therefore never performs a second OS-level DNS lookup for any hostname.
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .connect_timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, first_addr) // pin: reqwest will connect to this exact addr
            .build()
            .map_err(|e| {
                v1_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "CLIENT_ERROR",
                    &e.to_string(),
                )
            })?;

        // Step 3: send — reqwest uses the pinned address, no second DNS lookup.
        let resp = client.get(&current_url).send().await.map_err(|e| {
            v1_error(
                StatusCode::BAD_GATEWAY,
                "PROXY_ERROR",
                &format!("hop {hop}: {e}"),
            )
        })?;

        let status = resp.status();

        // Step 4: handle redirects manually.
        if status.is_redirection() {
            let location = resp
                .headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    v1_error(
                        StatusCode::BAD_GATEWAY,
                        "REDIRECT_ERROR",
                        "redirect response has no Location header",
                    )
                })?;

            // Resolve relative Location against the current URL.
            let next = parsed_url.join(&location).map_err(|e| {
                v1_error(
                    StatusCode::BAD_GATEWAY,
                    "REDIRECT_ERROR",
                    &format!("invalid Location: {e}"),
                )
            })?;

            current_url = next.to_string();
            continue; // loop — next hop will validate + pin the new hostname
        }

        // Step 5: on 2xx, stream body.
        if status.is_success() {
            let headers = resp.headers().clone();
            let stream = resp.bytes_stream();

            let mut response = axum::response::Response::builder().status(status);

            // Reject HTML content (SSRF content injection prevention).
            let content_type = headers
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("");
            if content_type.contains("text/html") || content_type.contains("application/xhtml") {
                return Err(v1_error(
                    StatusCode::BAD_GATEWAY,
                    "PROXY_BLOCKED",
                    "stream returned HTML, possible SSRF redirect",
                ));
            }

            if !content_type.is_empty() {
                response = response.header(header::CONTENT_TYPE, content_type);
            }
            response = response
                .header("Access-Control-Allow-Origin", "*")
                .header("Access-Control-Allow-Methods", "GET, OPTIONS")
                .header("Access-Control-Allow-Headers", "Range, Content-Type");

            return Ok(response
                .body(Body::from_stream(
                    stream.map(|chunk| chunk.map_err(std::io::Error::other)),
                ))
                .unwrap());
        }

        // Any other status (4xx, 5xx) → propagate as error.
        return Err(v1_error(
            StatusCode::BAD_GATEWAY,
            "PROXY_ERROR",
            &format!("upstream returned status {status} at hop {hop}"),
        ));
    }

    Err(v1_error(
        StatusCode::BAD_GATEWAY,
        "TOO_MANY_REDIRECTS",
        &format!("exceeded {MAX_HOPS} redirect hops"),
    ))
}
