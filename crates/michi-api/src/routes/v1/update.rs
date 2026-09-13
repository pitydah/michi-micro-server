use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::AppState;

const GITHUB_RELEASES_URL: &str =
    "https://api.github.com/repos/pitydah/michi-micro-server/releases";
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(5);
const CACHE_TTL_STATUS: Duration = Duration::from_secs(6 * 3600); // 6 hours
const MIN_CHECK_INTERVAL: Duration = Duration::from_secs(30); // Prevent spamming GitHub

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub html_url: String,
    pub body: Option<String>,
    pub prerelease: bool,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub channel: String,
    pub release_url: Option<String>,
    pub release_name: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub deployment_platform: String,
    pub instructions: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    pub channel: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateCheckBody {
    pub channel: Option<String>,
}

struct CachedReleases {
    checked_at: Instant,
    checked_at_iso: String,
    releases: Vec<GitHubRelease>,
}

lazy_static::lazy_static! {
    static ref RELEASES_CACHE: Arc<RwLock<Option<CachedReleases>>> = Arc::new(RwLock::new(None));
}

async fn fetch_github_releases() -> Result<Vec<GitHubRelease>, String> {
    let client = reqwest::Client::builder()
        .timeout(UPSTREAM_TIMEOUT)
        .user_agent("michi-micro-server")
        .build()
        .map_err(|e| format!("failed to build http client: {e}"))?;

    let res = client
        .get(GITHUB_RELEASES_URL)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("failed to fetch releases from GitHub: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("GitHub API returned status: {}", res.status()));
    }

    let releases = res
        .json::<Vec<GitHubRelease>>()
        .await
        .map_err(|e| format!("failed to parse GitHub releases response: {e}"))?;

    Ok(releases)
}

fn platform_instructions(platform: &str) -> String {
    match platform.to_lowercase().as_str() {
        "zimaos" => "To update on ZimaOS, open App Store -> Installed Apps -> Michi Micro Server, and click Update or reinstall to pull the latest container image.".to_string(),
        "casaos" => "To update on CasaOS, open CasaOS App settings for Michi Micro Server and click Check for updates or Reinstall.".to_string(),
        "docker" => "To update your Docker container, run: docker compose pull && docker compose up -d".to_string(),
        _ => "Visit the GitHub release page to download or pull the latest container image.".to_string(),
    }
}

fn compute_update_info(
    current_version_str: &str,
    platform: &str,
    channel_param: Option<&str>,
    cached_iso: Option<String>,
    releases: &[GitHubRelease],
) -> UpdateInfo {
    let raw_cur = current_version_str
        .trim()
        .strip_prefix('v')
        .unwrap_or(current_version_str.trim());
    let current_semver = semver::Version::parse(raw_cur).ok();

    // Default channel: if current is prerelease, preview; else stable
    let default_channel = if let Some(ref cv) = current_semver {
        if !cv.pre.is_empty() {
            "preview"
        } else {
            "stable"
        }
    } else {
        "stable"
    };

    let channel = channel_param
        .map(|c| c.trim().to_lowercase())
        .filter(|c| c == "stable" || c == "preview")
        .unwrap_or_else(|| default_channel.to_string());

    let mut candidates: Vec<(semver::Version, &GitHubRelease)> = Vec::new();

    for rel in releases {
        if channel == "stable" && rel.prerelease {
            continue;
        }
        let tag = rel
            .tag_name
            .trim()
            .strip_prefix('v')
            .unwrap_or(rel.tag_name.trim());
        if let Ok(ver) = semver::Version::parse(tag) {
            if channel == "stable" && !ver.pre.is_empty() {
                continue;
            }
            candidates.push((ver, rel));
        }
    }

    candidates.sort_by(|a, b| b.0.cmp(&a.0));

    let (latest_version, update_available, release_url, release_name, release_notes, published_at) =
        if let Some((latest_semver, rel)) = candidates.first() {
            let is_newer = if let Some(ref cur) = current_semver {
                latest_semver > cur
            } else {
                false
            };
            (
                Some(latest_semver.to_string()),
                is_newer,
                Some(rel.html_url.clone()),
                rel.name.clone(),
                rel.body.clone(),
                rel.published_at.clone(),
            )
        } else {
            (None, false, None, None, None, None)
        };

    UpdateInfo {
        current_version: current_version_str.to_string(),
        latest_version,
        update_available,
        channel,
        release_url,
        release_name,
        release_notes,
        published_at,
        last_checked_at: cached_iso,
        deployment_platform: platform.to_string(),
        instructions: platform_instructions(platform),
    }
}

pub async fn update_status_handler(
    State(state): State<AppState>,
    Query(query): Query<UpdateQuery>,
) -> impl IntoResponse {
    let now = Instant::now();
    let current_version = state.config.version();
    let platform = &state.config.deployment_platform;

    // Check if cache is fresh
    {
        let cache_read = RELEASES_CACHE.read().await;
        if let Some(ref entry) = *cache_read {
            if now.duration_since(entry.checked_at) < CACHE_TTL_STATUS {
                let info = compute_update_info(
                    current_version,
                    platform,
                    query.channel.as_deref(),
                    Some(entry.checked_at_iso.clone()),
                    &entry.releases,
                );
                return (StatusCode::OK, Json(info)).into_response();
            }
        }
    }

    // Attempt fetch
    let fetch_result = fetch_github_releases().await;
    let mut cache_write = RELEASES_CACHE.write().await;

    match fetch_result {
        Ok(releases) => {
            let iso = Utc::now().to_rfc3339();
            *cache_write = Some(CachedReleases {
                checked_at: now,
                checked_at_iso: iso.clone(),
                releases: releases.clone(),
            });
            let info = compute_update_info(
                current_version,
                platform,
                query.channel.as_deref(),
                Some(iso),
                &releases,
            );
            (StatusCode::OK, Json(info)).into_response()
        }
        Err(err) => {
            tracing::warn!("failed to query upstream github releases: {err}");
            // Fallback to existing cache if available
            if let Some(ref entry) = *cache_write {
                let info = compute_update_info(
                    current_version,
                    platform,
                    query.channel.as_deref(),
                    Some(entry.checked_at_iso.clone()),
                    &entry.releases,
                );
                (StatusCode::OK, Json(info)).into_response()
            } else {
                let info = compute_update_info(
                    current_version,
                    platform,
                    query.channel.as_deref(),
                    None,
                    &[],
                );
                (StatusCode::OK, Json(info)).into_response()
            }
        }
    }
}

pub async fn update_check_handler(
    State(state): State<AppState>,
    body: Option<Json<UpdateCheckBody>>,
) -> impl IntoResponse {
    let now = Instant::now();
    let current_version = state.config.version();
    let platform = &state.config.deployment_platform;
    let channel_param = body.as_ref().and_then(|b| b.channel.as_deref());

    // Prevent hammering GitHub API: rate limit check requests to at least 30s
    {
        let cache_read = RELEASES_CACHE.read().await;
        if let Some(ref entry) = *cache_read {
            if now.duration_since(entry.checked_at) < MIN_CHECK_INTERVAL {
                let info = compute_update_info(
                    current_version,
                    platform,
                    channel_param,
                    Some(entry.checked_at_iso.clone()),
                    &entry.releases,
                );
                return (StatusCode::OK, Json(info)).into_response();
            }
        }
    }

    let fetch_result = fetch_github_releases().await;
    let mut cache_write = RELEASES_CACHE.write().await;

    match fetch_result {
        Ok(releases) => {
            let iso = Utc::now().to_rfc3339();
            *cache_write = Some(CachedReleases {
                checked_at: now,
                checked_at_iso: iso.clone(),
                releases: releases.clone(),
            });
            let info = compute_update_info(
                current_version,
                platform,
                channel_param,
                Some(iso),
                &releases,
            );
            (StatusCode::OK, Json(info)).into_response()
        }
        Err(err) => {
            tracing::warn!("failed to check upstream github releases: {err}");
            if let Some(ref entry) = *cache_write {
                let info = compute_update_info(
                    current_version,
                    platform,
                    channel_param,
                    Some(entry.checked_at_iso.clone()),
                    &entry.releases,
                );
                (StatusCode::OK, Json(info)).into_response()
            } else {
                let info = compute_update_info(current_version, platform, channel_param, None, &[]);
                (StatusCode::OK, Json(info)).into_response()
            }
        }
    }
}
