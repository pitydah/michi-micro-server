//! AirCat v1 dedicated HTTP producer routes for michi-micro-server.
//!
//! Provides:
//! - GET  /api/v1/aircat/manifest
//! - POST /api/v1/aircat/authorize
//! - GET  /api/v1/aircat/download/:remote_id
//! - GET  /api/v1/aircat/enrollment-qr
//!
//! All routes require authenticated Home membership. Original files are served
//! strictly without transcoding, with strong quoted ETags and receipt header
//! X-Michi-AirCat-Authorized-Grant-SHA256.

use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::VerifyingKey;
use michi_identity::types::decode_base64url_strict;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use uuid::Uuid;

use crate::{
    aircat_grants::{self, ApprovedAsset, AuthorizedContext, Refusal},
    aircat_original_file,
    aircat_producer_security::ProducerSnapshot,
    AppState,
};

pub const AIRCAT_PROTOCOL_HEADER: &str = "X-Michi-AirCat-Protocol";
pub const AIRCAT_PROTOCOL_VALUE: &str = "michi.aircat.original.v1";

pub fn aircat_json_headers() -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::HeaderName::from_static("x-michi-aircat-protocol"),
        header::HeaderValue::from_static(AIRCAT_PROTOCOL_VALUE),
    );
    headers.insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    headers
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AirCatCatalogTrack {
    pub track_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_track_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_track_id: Option<String>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub format: String,
    pub size: i64,
    pub checksum: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AirCatCatalogManifest {
    pub schema: String,
    pub version: i32,
    pub device_id: String,
    pub total_tracks: i32,
    pub total_size: i64,
    pub tracks: Vec<AirCatCatalogTrack>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AirCatTrustedEnrollmentQr {
    #[serde(rename = "type")]
    pub qr_type: String,
    pub home_id: String,
    pub authority_michi_id: String,
    pub authority_public_key_base64_url: String,
    pub source_michi_id: String,
    pub pinned_base_url: String,
    pub tls_spki_sha256: String,
    pub nonce_base64_url: String,
    pub issued_at_millis: i64,
    pub expires_at_millis: i64,
}

pub fn aircat_error(
    status: StatusCode,
    code: &str,
    message: &str,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": {
                "code": code,
                "message": message,
                "details": {}
            }
        })),
    )
}

fn refusal_to_status(refusal: Refusal) -> (StatusCode, &'static str, &'static str) {
    match refusal {
        Refusal::UntrustedHomeMember => (
            StatusCode::FORBIDDEN,
            "UNTRUSTED_MEMBER",
            "caller is not an active Home member",
        ),
        Refusal::WrongAudience => (
            StatusCode::FORBIDDEN,
            "WRONG_AUDIENCE",
            "destination or audience mismatch",
        ),
        Refusal::Revoked => (
            StatusCode::FORBIDDEN,
            "MEMBER_REVOKED",
            "member identity is revoked",
        ),
        Refusal::Expired => (
            StatusCode::FORBIDDEN,
            "GRANT_EXPIRED",
            "grant or intent time window expired",
        ),
        Refusal::NotOriginalBytes => (
            StatusCode::NOT_FOUND,
            "ASSET_NOT_FOUND",
            "asset not found or bytes do not match original library files",
        ),
        Refusal::BadSignature => (
            StatusCode::FORBIDDEN,
            "BAD_SIGNATURE",
            "invalid Home signature",
        ),
        Refusal::InvalidFields => (
            StatusCode::BAD_REQUEST,
            "INVALID_FIELDS",
            "invalid fields or canonical schema violation",
        ),
    }
}

/// Checks whether a given device/member is revoked in this Home.
pub async fn is_member_revoked(state: &AppState, home_id: &str, device_michi_id: &str) -> bool {
    // 1. In-memory session manager revocations
    let mem_revs = state.receiver_manager.get_revocations().await;
    if mem_revs
        .iter()
        .any(|r| r.revoked_device_michi_id == device_michi_id)
    {
        return true;
    }

    // 2. Persisted SQLite home_revocations
    if let Ok(db_revs) = michi_db::list_home_revocations_db(&state.db, home_id).await {
        if db_revs
            .iter()
            .any(|r| r.revoked_device_michi_id == device_michi_id)
        {
            return true;
        }
    }

    false
}

/// Resolves and cryptographically checks the caller's Home session and identity.
pub async fn resolve_authenticated_caller(
    state: &AppState,
    headers: &HeaderMap,
    claimed_destination: Option<&str>,
) -> Result<String, (StatusCode, Json<serde_json::Value>)> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());

    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => h[7..].trim(),
        _ => {
            return Err(aircat_error(
                StatusCode::UNAUTHORIZED,
                "UNAUTHORIZED",
                "missing or malformed authorization header",
            ));
        }
    };

    let is_user_session = state.auth_sessions.validate(token).await;
    let is_device_token = state
        .token_store
        .validate(token, michi_link::TokenType::Device)
        .await;

    if !is_user_session && is_device_token.is_err() {
        return Err(aircat_error(
            StatusCode::UNAUTHORIZED,
            "INVALID_TOKEN",
            "invalid or expired session token",
        ));
    }

    let authenticated_member = if let Ok(device_uuid) = is_device_token {
        if let Ok(Some(dev)) = michi_db::get_link_device(&state.db, &device_uuid).await {
            if dev.revoked {
                return Err(aircat_error(
                    StatusCode::FORBIDDEN,
                    "DEVICE_REVOKED",
                    "device is revoked",
                ));
            }
            if let Some(claimed) = claimed_destination {
                if dev.alias == claimed || dev.device_id.to_string() == claimed {
                    claimed.to_string()
                } else if !dev.alias.is_empty() {
                    dev.alias
                } else {
                    claimed.to_string()
                }
            } else if !dev.alias.is_empty() {
                dev.alias
            } else {
                dev.device_id.to_string()
            }
        } else if let Some(claimed) = claimed_destination {
            claimed.to_string()
        } else {
            device_uuid.to_string()
        }
    } else {
        // Active user/admin session
        if let Some(claimed) = claimed_destination {
            claimed.to_string()
        } else if let Some(hdr_dev) = headers
            .get("X-Michi-Device-Id")
            .and_then(|h| h.to_str().ok())
        {
            hdr_dev.to_string()
        } else {
            state.identity.michi_id().to_base64url()
        }
    };

    // Header identity coherence check: if X-Michi-Device-Id header is provided, it MUST match
    if let Some(hdr_dev) = headers
        .get("X-Michi-Device-Id")
        .and_then(|h| h.to_str().ok())
    {
        if !hdr_dev.is_empty() && hdr_dev != authenticated_member {
            return Err(aircat_error(
                StatusCode::FORBIDDEN,
                "IDENTITY_MISMATCH",
                "X-Michi-Device-Id header does not match authenticated member",
            ));
        }
    }

    // Check Home revocation status
    let home_id = state.home_root_authority.home_id();
    if is_member_revoked(state, &home_id, &authenticated_member).await {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "MEMBER_REVOKED",
            "authenticated member is revoked in this Home",
        ));
    }

    Ok(authenticated_member)
}

/// GET /api/v1/aircat/manifest
///
/// Returns the original track catalog for AirCat transfer.
pub async fn aircat_manifest_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<AirCatCatalogManifest>), (StatusCode, Json<serde_json::Value>)> {
    let _authenticated_member = resolve_authenticated_caller(&state, &headers, None).await?;

    let producer_michi_id = state.identity.michi_id().to_base64url();

    let tracks = michi_db::list_tracks(&state.db).await.map_err(|e| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "DB_ERROR",
            &e.to_string(),
        )
    })?;

    let mut catalog_tracks = Vec::new();
    let mut total_size: i64 = 0;

    for track in tracks {
        if let Ok((_item, revision)) =
            aircat_original_file::inspect_original_asset(&state.config.music_paths, &track).await
        {
            total_size = total_size.saturating_add(revision.exact_size);
            let track_id_str = track.id.to_string();
            catalog_tracks.push(AirCatCatalogTrack {
                track_id: track_id_str.clone(),
                logical_track_id: Some(track_id_str.clone()),
                remote_track_id: Some(track_id_str),
                title: track.title.unwrap_or_default(),
                artist: track.artist.unwrap_or_default(),
                album: track.album.unwrap_or_default(),
                format: track.format.as_str().to_string(),
                size: revision.exact_size,
                checksum: revision.exact_sha256,
            });
        }
    }

    let total_tracks = catalog_tracks.len() as i32;

    Ok((
        aircat_json_headers(),
        Json(AirCatCatalogManifest {
            schema: "michi.aircat.manifest.v1".to_string(),
            version: 1,
            device_id: producer_michi_id,
            total_tracks,
            total_size,
            tracks: catalog_tracks,
        }),
    ))
}

/// POST /api/v1/aircat/authorize
///
/// Validates selection and intent against real original assets, records a producer
/// snapshot, reserves the challenge nonce, and issues a signed Ed25519 grant.
pub async fn aircat_authorize_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<aircat_grants::Request>,
) -> Result<(HeaderMap, Json<aircat_grants::Grant>), (StatusCode, Json<serde_json::Value>)> {
    let authenticated_member =
        resolve_authenticated_caller(&state, &headers, Some(&body.intent.destination_michi_id))
            .await?;

    // AUTH-02: destinationMichiId in body MUST match authenticated member
    if body.intent.destination_michi_id != authenticated_member {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "WRONG_AUDIENCE",
            "destinationMichiId does not match authenticated member",
        ));
    }

    let now_ms = chrono::Utc::now().timestamp_millis();
    let home_id = state.home_root_authority.home_id();
    let producer_id = state.identity.michi_id().to_base64url();

    // Verify intent source matches this server
    if body.intent.source_michi_id != producer_id {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "WRONG_SOURCE",
            "sourceMichiId does not match producer identity",
        ));
    }

    // Verify manifest hash matching canonical computation
    let computed_manifest_sha = aircat_grants::manifest_sha256(&body.manifest).map_err(|r| {
        let (status, code, msg) = refusal_to_status(r);
        aircat_error(status, code, msg)
    })?;

    if body.manifest_sha256 != computed_manifest_sha {
        return Err(aircat_error(
            StatusCode::CONFLICT,
            "MANIFEST_ALTERED",
            "manifestSha256 does not match canonical manifest calculation",
        ));
    }

    // Verify each selected item exists in server library and matches exact bytes
    let mut approved_assets = Vec::with_capacity(body.manifest.items.len());
    let mut original_revisions = Vec::with_capacity(body.manifest.items.len());

    for item in &body.manifest.items {
        let track_uuid = Uuid::parse_str(&item.remote_track_id).map_err(|_| {
            aircat_error(
                StatusCode::BAD_REQUEST,
                "INVALID_TRACK_ID",
                "remoteTrackId is not a valid UUID",
            )
        })?;

        let track = michi_db::get_track(&state.db, &track_uuid)
            .await
            .map_err(|e| {
                aircat_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DB_ERROR",
                    &e.to_string(),
                )
            })?
            .ok_or_else(|| {
                aircat_error(
                    StatusCode::NOT_FOUND,
                    "TRACK_NOT_FOUND",
                    &format!("track {} not found in library", item.remote_track_id),
                )
            })?;

        let (_file_item, revision) =
            aircat_original_file::inspect_original_asset(&state.config.music_paths, &track)
                .await
                .map_err(|r| {
                    let (status, code, msg) = refusal_to_status(r);
                    aircat_error(status, code, msg)
                })?;

        if revision.exact_size != item.size
            || !revision.exact_sha256.eq_ignore_ascii_case(&item.sha256)
        {
            return Err(aircat_error(
                StatusCode::CONFLICT,
                "ORIGINAL_BYTES_MISMATCH",
                "requested item size or sha256 does not match server original file",
            ));
        }

        approved_assets.push(ApprovedAsset {
            remote_track_id: item.remote_track_id.clone(),
            exact_size: revision.exact_size,
            exact_sha256: revision.exact_sha256.clone(),
        });

        original_revisions.push(revision);
    }

    // Construct and record ProducerSnapshot
    let snapshot = ProducerSnapshot {
        manifest: body.manifest.clone(),
        manifest_sha256: body.manifest_sha256.clone(),
        authenticated_destination_michi_id: authenticated_member.clone(),
        expires_at_ms: body.intent.expires_at,
        originals: original_revisions,
    };

    if !state
        .aircat_security
        .has_snapshot(&body.manifest.transfer_id, &body.manifest_sha256)
    {
        state
            .aircat_security
            .record_producer_snapshot(snapshot, now_ms)
            .map_err(|r| {
                let (status, code, msg) = refusal_to_status(r);
                aircat_error(status, code, msg)
            })?;
    }

    // Reserve nonce and authorization challenge atomically
    let _reserved_snapshot = state
        .aircat_security
        .reserve_authorization(&body, &authenticated_member, now_ms)
        .map_err(|r| {
            if r == Refusal::InvalidFields {
                aircat_error(
                    StatusCode::CONFLICT,
                    "NONCE_REPLAY",
                    "authorization challenge nonce already consumed or invalid",
                )
            } else {
                let (status, code, msg) = refusal_to_status(r);
                aircat_error(status, code, msg)
            }
        })?;

    // Build AuthorizedContext
    let ctx = AuthorizedContext {
        home_id,
        source_michi_id: producer_id,
        destination_michi_id: authenticated_member,
        source_library_id: body.intent.source_library_id.clone(),
        allowed_destination_library_id: body.intent.destination_library_id.clone(),
        is_active_home_member: true,
        may_read_original_files: true,
        approved_assets,
    };

    // Issue grant using HomeRootAuthority
    let grant = aircat_grants::issue_grant(&body, &ctx, &state.home_root_authority, now_ms)
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    Ok((aircat_json_headers(), Json(grant)))
}

/// GET /api/v1/aircat/download/:remote_id
///
/// Streams raw original uncompressed/original file bytes with ETag and receipt header.
pub async fn aircat_download_handler(
    State(state): State<AppState>,
    Path(remote_id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let now_ms = chrono::Utc::now().timestamp_millis();

    // Extract X-Michi-AirCat-Grant header
    let grant_header_raw = headers
        .get("X-Michi-AirCat-Grant")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            aircat_error(
                StatusCode::UNAUTHORIZED,
                "MISSING_GRANT",
                "missing X-Michi-AirCat-Grant header",
            )
        })?;

    // Compute mandatory protocol receipt header: lowercase_hex(sha256(ASCII(header_value)))
    let receipt = aircat_grants::receipt_for_grant_header(grant_header_raw).map_err(|_| {
        aircat_error(
            StatusCode::BAD_REQUEST,
            "INVALID_GRANT_HEADER",
            "grant header is not valid base64url or exceeds size limits",
        )
    })?;

    // Decode grant JSON
    let grant_json_bytes = URL_SAFE_NO_PAD.decode(grant_header_raw).map_err(|_| {
        aircat_error(
            StatusCode::BAD_REQUEST,
            "INVALID_GRANT_BASE64",
            "failed to decode base64url grant payload",
        )
    })?;

    let grant: aircat_grants::Grant = serde_json::from_slice(&grant_json_bytes).map_err(|_| {
        aircat_error(
            StatusCode::BAD_REQUEST,
            "INVALID_GRANT_JSON",
            "failed to parse grant json",
        )
    })?;

    // Authenticate caller session
    let authenticated_member =
        resolve_authenticated_caller(&state, &headers, Some(&grant.destination_michi_id)).await?;

    // Verify audience matches authenticated caller
    if grant.destination_michi_id != authenticated_member {
        return Err(aircat_error(
            StatusCode::FORBIDDEN,
            "WRONG_AUDIENCE",
            "grant destinationMichiId does not match authenticated member",
        ));
    }

    // Verify Home signature and grant parameters
    let root_pk_bytes = decode_base64url_strict(&state.home_root_authority.public_key_base64url())
        .map_err(|_| {
            aircat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "CRYPTO_ERROR",
                "failed to decode root authority public key",
            )
        })?;
    let root_key = VerifyingKey::from_bytes(&root_pk_bytes.try_into().map_err(|_| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CRYPTO_ERROR",
            "invalid root authority public key length",
        )
    })?)
    .map_err(|_| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "CRYPTO_ERROR",
            "invalid root authority public key format",
        )
    })?;

    let is_revoked = is_member_revoked(&state, &grant.home_id, &grant.destination_michi_id).await;

    aircat_grants::verify_grant(&grant, &root_key, &authenticated_member, now_ms, is_revoked)
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    // Verify remote_id is in retained authorized snapshot
    let revision = state
        .aircat_security
        .selected_original(
            &grant.transfer_id,
            &grant.manifest_sha256,
            &authenticated_member,
            &remote_id,
            now_ms,
        )
        .map_err(|r| {
            let (status, code, msg) = refusal_to_status(r);
            aircat_error(status, code, msg)
        })?;

    // Lookup track in DB
    let track_uuid = Uuid::parse_str(&remote_id).map_err(|_| {
        aircat_error(
            StatusCode::BAD_REQUEST,
            "INVALID_TRACK_ID",
            "remote_id is not a valid UUID",
        )
    })?;

    let track = michi_db::get_track(&state.db, &track_uuid)
        .await
        .map_err(|e| {
            aircat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DB_ERROR",
                &e.to_string(),
            )
        })?
        .ok_or_else(|| {
            aircat_error(
                StatusCode::NOT_FOUND,
                "TRACK_NOT_FOUND",
                "track not found in server library",
            )
        })?;

    // Concurrently open original file descriptor
    let (_path, mut file) =
        michi_streaming::open_track_file_async(&state.config.music_paths, &track)
            .await
            .map_err(|e| aircat_error(StatusCode::NOT_FOUND, "FILE_NOT_FOUND", &e.to_string()))?;

    // Verify file revision in descriptor has not changed
    let metadata = file.metadata().await.map_err(|e| {
        aircat_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "IO_ERROR",
            &e.to_string(),
        )
    })?;

    let file_len = metadata.len() as i64;
    if file_len != revision.exact_size {
        return Err(aircat_error(
            StatusCode::CONFLICT,
            "FILE_REVISION_CHANGED",
            "original file size changed during transfer",
        ));
    }

    let filename = format!("{}.{}", track.id, track.format.as_str());

    // Check If-Range header
    let if_range_matched =
        if let Some(if_range) = headers.get(header::IF_RANGE).and_then(|h| h.to_str().ok()) {
            if_range.trim() == revision.strong_etag
        } else {
            true
        };

    // Range handling
    if if_range_matched {
        if let Some(range_header) = headers.get(header::RANGE).and_then(|h| h.to_str().ok()) {
            match michi_streaming::parse_range(range_header, file_len as u64) {
                Ok(range) => {
                    file.seek(std::io::SeekFrom::Start(range.start))
                        .await
                        .map_err(|e| {
                            aircat_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "IO_ERROR",
                                &e.to_string(),
                            )
                        })?;

                    let taken = file.take(range.content_length());
                    let stream = tokio_util::io::ReaderStream::new(taken);

                    let response = Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
                        .header(header::CONTENT_TYPE, "application/octet-stream")
                        .header(header::CONTENT_RANGE, range.content_range_header())
                        .header(header::CONTENT_LENGTH, range.content_length().to_string())
                        .header(header::ACCEPT_RANGES, "bytes")
                        .header(header::ETAG, &revision.strong_etag)
                        .header("X-Michi-AirCat-Authorized-Grant-SHA256", receipt)
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{filename}\""),
                        )
                        .body(Body::from_stream(stream))
                        .map_err(|e| {
                            aircat_error(
                                StatusCode::INTERNAL_SERVER_ERROR,
                                "RESPONSE_BUILD_ERROR",
                                &e.to_string(),
                            )
                        })?;

                    return Ok(response);
                }
                Err(_) => {
                    let resp = Response::builder()
                        .status(StatusCode::RANGE_NOT_SATISFIABLE)
                        .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
                        .header(header::CONTENT_RANGE, format!("bytes */{file_len}"))
                        .header("X-Michi-AirCat-Authorized-Grant-SHA256", receipt)
                        .body(Body::empty())
                        .unwrap();
                    return Ok(resp);
                }
            }
        }
    }

    // Full 200 OK download
    let stream = tokio_util::io::ReaderStream::new(file);
    let response = Response::builder()
        .status(StatusCode::OK)
        .header(AIRCAT_PROTOCOL_HEADER, AIRCAT_PROTOCOL_VALUE)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, file_len.to_string())
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::ETAG, &revision.strong_etag)
        .header("X-Michi-AirCat-Authorized-Grant-SHA256", receipt)
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        )
        .body(Body::from_stream(stream))
        .map_err(|e| {
            aircat_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "RESPONSE_BUILD_ERROR",
                &e.to_string(),
            )
        })?;

    Ok(response)
}

/// GET /api/v1/aircat/enrollment-qr
///
/// Generates an administrative AirCat enrollment QR payload (michi.aircat.home.enrollment.v1).
pub async fn aircat_enrollment_qr_handler(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(HeaderMap, Json<AirCatTrustedEnrollmentQr>), (StatusCode, Json<serde_json::Value>)> {
    let _authenticated_member = resolve_authenticated_caller(&state, &headers, None).await?;

    let now_ms = chrono::Utc::now().timestamp_millis();
    let home_id = state.home_root_authority.home_id();
    let root_pk = state.home_root_authority.public_key_base64url();
    let source_michi_id = state.identity.michi_id().to_base64url();

    let mut random_nonce = [0u8; 24];
    use rand::RngCore;
    rand::rngs::OsRng.fill_bytes(&mut random_nonce);
    let nonce_base64_url = URL_SAFE_NO_PAD.encode(random_nonce);

    // Compute or format pinnedBaseUrl and tlsSpkiSha256
    let pinned_base_url = format!("https://localhost:{}", state.config.port);
    let tls_spki = {
        let pk_bytes = decode_base64url_strict(&root_pk).unwrap_or_default();
        let digest = Sha256::digest(&pk_bytes);
        format!(
            "sha256/{}",
            base64::engine::general_purpose::STANDARD.encode(digest)
        )
    };

    Ok((
        aircat_json_headers(),
        Json(AirCatTrustedEnrollmentQr {
            qr_type: "michi.aircat.home.enrollment.v1".to_string(),
            home_id: home_id.clone(),
            authority_michi_id: home_id,
            authority_public_key_base64_url: root_pk,
            source_michi_id,
            pinned_base_url,
            tls_spki_sha256: tls_spki,
            nonce_base64_url,
            issued_at_millis: now_ms,
            expires_at_millis: now_ms + 600_000,
        }),
    ))
}
