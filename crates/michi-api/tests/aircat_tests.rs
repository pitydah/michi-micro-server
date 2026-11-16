use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use michi_api::create_router;
use michi_config::Config;
use michi_core::{AudioFormat, Track};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::io::Write;
use tower::ServiceExt;
use uuid::Uuid;

async fn test_db_with_url() -> (SqlitePool, String) {
    let url = format!(
        "sqlite:file:memdb_aircat_{}?mode=memory&cache=shared",
        Uuid::new_v4()
    );
    let pool = michi_db::init_pool(&url).await.unwrap();
    (pool, url)
}

fn test_config_with_url(db_url: String, music_dir: std::path::PathBuf) -> Config {
    let tmp = std::env::temp_dir().join(format!("michi-aircat-test-{}", Uuid::new_v4()));
    let _ = std::fs::create_dir_all(&tmp);

    Config {
        port: 9091,
        music_paths: vec![music_dir],
        config_path: tmp.join("config"),
        cache_path: tmp.join("cache"),
        database_url: db_url,

        version: "1.0.0-rc.4",
        sync_peers: Vec::new(),
        sync_name: "michi-server-aircat-test".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: None,
        auth_password: None,
        auth_enabled: false,
        allow_registration: false,
        server_id: Uuid::new_v4(),
        cors_origin: None,
        dev_mode: true,
        resource_profile: michi_core::ResourceProfile::Balanced,
        format_policy: michi_core::AudioFormatPolicy::LosslessOnly,
        stream_profile: michi_core::StreamProfile::Original,
        max_remote_bitrate: 320_000,
        remote_sync: false,
        language: "en".into(),
        ui: Default::default(),
        auto_backup_enabled: false,
        backup_max_keep: 7,
        job_max_concurrent: 3,
        reconnect_delay_max: 300,
        opensubsonic_enabled: false,
        trust_proxy: false,
        trusted_proxies: vec!["127.0.0.1".parse().unwrap(), "::1".parse().unwrap()],
        deployment_platform: "unknown".into(),
    }
}

async fn make_app() -> (
    axum::Router,
    SqlitePool,
    michi_api::AppState,
    String,
    std::path::PathBuf,
) {
    let (pool, db_url) = test_db_with_url().await;
    let music_dir = std::env::temp_dir().join(format!("michi-aircat-music-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&music_dir).unwrap();

    let config = test_config_with_url(db_url, music_dir.clone());
    let identity = std::sync::Arc::new(
        michi_identity::IdentityManager::generate(
            &config.config_path,
            "AirCat Test Server",
            "password123",
        )
        .unwrap(),
    );
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, "admin-aircat", "password", true)
        .await
        .unwrap();
    let state =
        michi_api::AppState::new_with_identity(config, pool.clone(), Some(admin_id), identity);
    let token = state.auth_sessions.create_session(admin_id).await.unwrap();
    let app = create_router(state.clone());
    (app, pool, state, token, music_dir)
}

fn create_test_file(
    dir: &std::path::Path,
    filename: &str,
    content: &[u8],
) -> (String, i64, std::path::PathBuf) {
    let file_path = dir.join(filename);
    let mut file = std::fs::File::create(&file_path).unwrap();
    file.write_all(content).unwrap();
    let digest = hex::encode(Sha256::digest(content));
    (digest, content.len() as i64, file_path)
}

#[tokio::test]
async fn test_aircat_manifest_empty_and_populated() {
    let (app, pool, _state, token, music_dir) = make_app().await;
    let auth = format!("Bearer {token}");

    // 1. Empty catalog manifest -> 0/0
    let req = Request::builder()
        .uri("/api/v1/aircat/manifest")
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    assert_eq!(
        resp.headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store"
    );
    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let manifest: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(manifest["schema"], "michi.aircat.manifest.v1");
    assert_eq!(manifest["version"], 1);
    assert_eq!(manifest["total_tracks"], 0);
    assert_eq!(manifest["total_size"], 0);
    assert_eq!(manifest["tracks"].as_array().unwrap().len(), 0);

    // 2. Insert a real track file on disk and in DB
    let filename = format!("test-{}.flac", Uuid::new_v4());
    let content = b"FAKE_FLAC_AUDIO_CONTENT_FOR_AIRCAT_TEST_1234567890";
    let (sha256, size, full_path) = create_test_file(&music_dir, &filename, content);
    let track_id = michi_core::track_id_from_library_path(&music_dir, &full_path);

    let track = Track {
        id: track_id,
        file_path: full_path.to_string_lossy().to_string(),
        title: Some("AirCat Test Track".into()),
        artist: Some("AirCat Artist".into()),
        album: Some("AirCat Album".into()),
        album_artist: None,
        duration_ms: Some(3500),
        format: AudioFormat::Flac,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: Some(2026),
        track_number: Some(1),
        disc_number: None,
        content_hash: Some(sha256.clone()),
        file_size: Some(size as u64),
        file_mtime_ns: Some(1731679400000000000),
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    // 3. Request manifest again -> 1 track with exact totals and SHA-256
    let req = Request::builder()
        .uri("/api/v1/aircat/manifest")
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    assert_eq!(
        resp.headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store"
    );
    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let manifest: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(manifest["total_tracks"], 1);
    assert_eq!(manifest["total_size"], size);

    let t = &manifest["tracks"][0];
    assert_eq!(t["track_id"], track_id.to_string());
    assert_eq!(t["logical_track_id"], track_id.to_string());
    assert_eq!(t["remote_track_id"], track_id.to_string());
    assert_eq!(t["title"], "AirCat Test Track");
    assert_eq!(t["format"], "flac");
    assert_eq!(t["size"], size);
    assert_eq!(t["checksum"], sha256);
}

#[tokio::test]
async fn test_aircat_authorize_and_download_full_flow() {
    let (app, pool, state, token, music_dir) = make_app().await;
    let auth = format!("Bearer {token}");

    // Insert track
    let filename = format!("stream-{}.flac", Uuid::new_v4());
    let content = b"AIRCAT_ORIGINAL_STREAM_BYTES_TEST_REPRODUCIBLE_00001";
    let (sha256, size, full_path) = create_test_file(&music_dir, &filename, content);
    let track_id = michi_core::track_id_from_library_path(&music_dir, &full_path);

    let track = Track {
        id: track_id,
        file_path: full_path.to_string_lossy().to_string(),
        title: Some("AirCat Stream".into()),
        artist: Some("Michi".into()),
        album: Some("Album".into()),
        album_artist: None,
        duration_ms: Some(2000),
        format: AudioFormat::Flac,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: Some(2026),
        track_number: Some(1),
        disc_number: None,
        content_hash: Some(sha256.clone()),
        file_size: Some(size as u64),
        file_mtime_ns: Some(1731679400000000000),
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &track).await.unwrap();

    let producer_id = state.identity.michi_id().to_base64url();
    let dest_member_id = "lhlxbEOUgOykbYw6mg4McXy75xBWBX5HOn8pVbixqbY";
    let transfer_id = format!("tx-{}", Uuid::new_v4());
    let nonce_bytes = [0x5au8; 24];
    let client_nonce = URL_SAFE_NO_PAD.encode(nonce_bytes);
    let now_ms = chrono::Utc::now().timestamp_millis();

    let file_item = michi_api::aircat_grants::FileItem {
        logical_track_id: "client-track-001".into(),
        remote_track_id: track_id.to_string(),
        title: "AirCat Stream".into(),
        artist: "Michi".into(),
        album: "Album".into(),
        format: "flac".into(),
        size,
        sha256: sha256.clone(),
    };

    let manifest_dto = michi_api::aircat_grants::Manifest {
        manifest_version: 1,
        transfer_id: transfer_id.clone(),
        source_device_id: producer_id.clone(),
        total_tracks: 1,
        total_bytes: size,
        items: vec![file_item],
    };

    let canonical_manifest_sha = michi_api::aircat_grants::manifest_sha256(&manifest_dto).unwrap();

    let auth_request = michi_api::aircat_grants::Request {
        intent: michi_api::aircat_grants::Intent {
            transfer_id: transfer_id.clone(),
            source_michi_id: producer_id.clone(),
            destination_michi_id: dest_member_id.to_string(),
            source_library_id: "lib-source-default".into(),
            destination_library_id: "lib-dest-mobile".into(),
            selection_scope: "ALBUMS".into(),
            manifest_version: 1,
            required_scopes: vec!["content:read".into(), "content:transfer".into()],
            expires_at: now_ms + 300_000,
            signed_grant: None,
            authorization_nonce_base64_url: String::new(),
        },
        manifest: manifest_dto,
        manifest_sha256: canonical_manifest_sha.clone(),
        client_nonce_base64_url: client_nonce.clone(),
    };

    // 1. POST /api/v1/aircat/authorize
    let req = Request::builder()
        .uri("/api/v1/aircat/authorize")
        .method("POST")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&auth_request).unwrap()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    assert_eq!(
        resp.headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store"
    );
    let grant_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let grant: michi_api::aircat_grants::Grant = serde_json::from_slice(&grant_bytes).unwrap();

    assert_eq!(grant.version, 1);
    assert_eq!(grant.home_id, state.home_root_authority.home_id());
    assert_eq!(grant.destination_michi_id, dest_member_id);
    assert_eq!(grant.manifest_sha256, canonical_manifest_sha);
    assert_eq!(grant.nonce_base64_url, client_nonce);
    assert_eq!(grant.signature_base64_url.len(), 86);

    // 2. Replay check: A second POST with the exact same nonce fails with 409 Conflict
    let req_replay = Request::builder()
        .uri("/api/v1/aircat/authorize")
        .method("POST")
        .header(header::AUTHORIZATION, &auth)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_vec(&auth_request).unwrap()))
        .unwrap();
    let resp_replay = app.clone().oneshot(req_replay).await.unwrap();
    assert_eq!(resp_replay.status(), StatusCode::CONFLICT);

    // 3. GET /api/v1/aircat/download/:remote_id (Full 200 OK)
    let grant_header = URL_SAFE_NO_PAD.encode(&grant_bytes);
    let expected_receipt = hex::encode(Sha256::digest(grant_header.as_bytes()));

    let req_dl = Request::builder()
        .uri(format!("/api/v1/aircat/download/{track_id}"))
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .body(Body::empty())
        .unwrap();

    let resp_dl = app.clone().oneshot(req_dl).await.unwrap();
    assert_eq!(resp_dl.status(), StatusCode::OK);
    assert_eq!(
        resp_dl
            .headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    assert_eq!(
        resp_dl
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/octet-stream"
    );
    assert_eq!(
        resp_dl
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store, no-transform"
    );
    assert_eq!(
        resp_dl
            .headers()
            .get("X-Michi-AirCat-Authorized-Grant-SHA256")
            .unwrap()
            .to_str()
            .unwrap(),
        expected_receipt
    );
    let expected_etag = format!("\"sha256-{sha256}-{size}\"");
    assert_eq!(
        resp_dl
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap(),
        expected_etag
    );

    let downloaded_bytes = axum::body::to_bytes(resp_dl.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(downloaded_bytes.as_ref(), content);

    // 4. GET /api/v1/aircat/download/:remote_id with Range (206 Partial Content)
    let req_range = Request::builder()
        .uri(format!("/api/v1/aircat/download/{track_id}"))
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .header(header::RANGE, "bytes=0-9")
        .header(header::IF_RANGE, &expected_etag)
        .body(Body::empty())
        .unwrap();

    let resp_range = app.clone().oneshot(req_range).await.unwrap();
    assert_eq!(resp_range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        resp_range
            .headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    assert_eq!(
        resp_range
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap(),
        "application/octet-stream"
    );
    assert_eq!(
        resp_range
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store, no-transform"
    );
    assert_eq!(
        resp_range
            .headers()
            .get("X-Michi-AirCat-Authorized-Grant-SHA256")
            .unwrap()
            .to_str()
            .unwrap(),
        expected_receipt
    );
    assert_eq!(
        resp_range
            .headers()
            .get(header::CONTENT_RANGE)
            .unwrap()
            .to_str()
            .unwrap(),
        format!("bytes 0-9/{size}")
    );
    let range_bytes = axum::body::to_bytes(resp_range.into_body(), 1024 * 1024)
        .await
        .unwrap();
    assert_eq!(range_bytes.as_ref(), &content[0..10]);

    // 5. GET /api/v1/aircat/download/:remote_id with Invalid Range -> 416
    let req_invalid_range = Request::builder()
        .uri(format!("/api/v1/aircat/download/{track_id}"))
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .header(header::RANGE, "bytes=999999-")
        .body(Body::empty())
        .unwrap();

    let resp_inv_range = app.clone().oneshot(req_invalid_range).await.unwrap();
    assert_eq!(resp_inv_range.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        resp_inv_range
            .headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );

    // 6. Adversarial: Download an unselected track ID -> 403 / 404
    let unselected_uuid = Uuid::new_v4();
    let req_unselected = Request::builder()
        .uri(format!("/api/v1/aircat/download/{unselected_uuid}"))
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .body(Body::empty())
        .unwrap();

    let resp_unselected = app.clone().oneshot(req_unselected).await.unwrap();
    assert_eq!(resp_unselected.status(), StatusCode::NOT_FOUND);

    // 7. Revoke member: revoke_device and verify subsequent downloads are rejected with 403
    state
        .receiver_manager
        .revoke_device(dest_member_id, "Test member revocation")
        .await
        .unwrap();

    let req_revoked = Request::builder()
        .uri(format!("/api/v1/aircat/download/{track_id}"))
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .body(Body::empty())
        .unwrap();

    let resp_revoked = app.clone().oneshot(req_revoked).await.unwrap();
    assert_eq!(resp_revoked.status(), StatusCode::FORBIDDEN);
}

fn sample_x509_der_cert() -> Vec<u8> {
    // Minimal valid ASN.1 DER X.509 certificate with a deterministic 32-byte SPKI payload
    let spki = vec![
        0x30, 0x2A, // SEQUENCE (42 bytes)
        0x30, 0x05, 0x06, 0x03, 0x2B, 0x65,
        0x70, // AlgorithmIdentifier (Ed25519 OID 1.3.101.112)
        0x03, 0x21, 0x00, // BIT STRING (33 bytes, 0 unused bits)
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        26, 27, 28, 29, 30, 31, 32,
    ];
    let mut tbs = Vec::new();
    // [0] EXPLICIT version v3 (02 01 02)
    tbs.extend_from_slice(&[0xA0, 0x03, 0x02, 0x01, 0x02]);
    // serialNumber (INTEGER 1)
    tbs.extend_from_slice(&[0x02, 0x01, 0x01]);
    // signature AlgorithmIdentifier (NULL)
    tbs.extend_from_slice(&[0x30, 0x02, 0x05, 0x00]);
    // issuer (empty SEQUENCE)
    tbs.extend_from_slice(&[0x30, 0x00]);
    // validity (empty SEQUENCE)
    tbs.extend_from_slice(&[0x30, 0x00]);
    // subject (empty SEQUENCE)
    tbs.extend_from_slice(&[0x30, 0x00]);
    // subjectPublicKeyInfo
    tbs.extend_from_slice(&spki);

    let mut tbs_tlv = vec![0x30, tbs.len() as u8];
    tbs_tlv.extend_from_slice(&tbs);

    let mut cert_body = Vec::new();
    cert_body.extend_from_slice(&tbs_tlv);
    cert_body.extend_from_slice(&[0x30, 0x02, 0x05, 0x00]); // signatureAlgorithm
    cert_body.extend_from_slice(&[0x03, 0x02, 0x00, 0x00]); // signatureValue

    let mut cert = vec![0x30, cert_body.len() as u8];
    cert.extend_from_slice(&cert_body);
    cert
}

#[tokio::test]
async fn test_aircat_enrollment_qr_handler() {
    let (app, _pool, state, token, _music_dir) = make_app().await;
    let auth = format!("Bearer {token}");

    // 1. Without TLS configured, enrollment QR fails closed with 503 and server/info reports false
    let req_no_tls = Request::builder()
        .uri("/api/v1/aircat/enrollment-qr")
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();
    let resp_no_tls = app.clone().oneshot(req_no_tls).await.unwrap();
    assert_eq!(resp_no_tls.status(), StatusCode::SERVICE_UNAVAILABLE);

    let info_resp_1 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let info_1: Value = serde_json::from_slice(
        &axum::body::to_bytes(info_resp_1.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(info_1["features"]["aircat_original_v1"], false);
    assert_eq!(info_1["features"]["home_tls_spki_pinned"], false);

    // 2. Configure real X.509 SPKI pin on AppState
    let cert_der = sample_x509_der_cert();
    let expected_spki = michi_api::extract_spki_sha256_from_pem(&cert_der).unwrap();
    assert!(expected_spki.starts_with("sha256/"));
    *state.aircat_tls.write().await = Some(michi_api::AirCatTlsInfo {
        pinned_base_url: "https://192.168.1.50:9443".to_string(),
        tls_spki_sha256: expected_spki.clone(),
        cert_path: std::path::PathBuf::from("/tmp/test-tls.crt"),
        key_path: std::path::PathBuf::from("/tmp/test-tls.key"),
        tls_port: 9443,
    });

    let info_resp_2 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/server/info")
                .method("GET")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let info_2: Value = serde_json::from_slice(
        &axum::body::to_bytes(info_resp_2.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(info_2["features"]["aircat_original_v1"], true);
    assert_eq!(info_2["features"]["home_tls_spki_pinned"], true);

    let req = Request::builder()
        .uri("/api/v1/aircat/enrollment-qr")
        .method("GET")
        .header(header::AUTHORIZATION, &auth)
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    assert_eq!(
        resp.headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap(),
        "no-store"
    );
    let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let qr: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(qr["type"], "michi.aircat.home.enrollment.v1");
    assert_eq!(qr["homeId"], state.home_root_authority.home_id());
    assert_eq!(qr["authorityMichiId"], state.home_root_authority.home_id());
    assert_eq!(
        qr["authorityPublicKeyBase64Url"],
        state.home_root_authority.public_key_base64url()
    );
    assert_eq!(
        qr["sourceMichiId"],
        state.identity.michi_id().to_base64url()
    );
    assert_eq!(qr["pinnedBaseUrl"], "https://192.168.1.50:9443");
    assert_eq!(qr["tlsSpkiSha256"], expected_spki);
    assert!(!qr["nonceBase64Url"].as_str().unwrap().is_empty());
    assert!(qr["expiresAtMillis"].as_i64().unwrap() > qr["issuedAtMillis"].as_i64().unwrap());
}

#[cfg(unix)]
#[tokio::test]
async fn test_aircat_rejects_symlinks_and_detects_same_size_file_swap() {
    let (app, pool, state, token, music_dir) = make_app().await;
    let auth = format!("Bearer {token}");

    // 1. Create a real target file and a symlink pointing to it inside music_dir
    let real_name = format!("real-{}.flac", Uuid::new_v4());
    let link_name = format!("link-{}.flac", Uuid::new_v4());
    let (_sha, _size, real_path) = create_test_file(&music_dir, &real_name, b"REAL_AUDIO_DATA_01");
    let link_path = music_dir.join(&link_name);
    std::os::unix::fs::symlink(&real_path, &link_path).unwrap();

    let symlink_track_id = michi_core::track_id_from_library_path(&music_dir, &link_path);
    let sym_track = Track {
        id: symlink_track_id,
        file_path: link_path.to_string_lossy().to_string(),
        title: Some("Symlink Track".into()),
        artist: Some("A".into()),
        album: Some("B".into()),
        album_artist: None,
        duration_ms: Some(10_000),
        format: AudioFormat::Flac,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: Some(2026),
        track_number: Some(1),
        disc_number: Some(1),
        content_hash: None,
        file_size: Some(18),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &sym_track).await.unwrap();

    // Manifest must NOT expose the symlinked track
    let manifest_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let manifest: Value = serde_json::from_slice(
        &axum::body::to_bytes(manifest_resp.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(manifest["total_tracks"], 0);

    // 2. Test same-size file swap detection between authorize and download
    let regular_name = format!("swap-{}.flac", Uuid::new_v4());
    let content_v1 = b"AAAA_ORIGINAL_V1";
    let (sha_v1, size_v1, regular_path) = create_test_file(&music_dir, &regular_name, content_v1);
    let reg_track_id = michi_core::track_id_from_library_path(&music_dir, &regular_path);
    let reg_track = Track {
        id: reg_track_id,
        file_path: regular_path.to_string_lossy().to_string(),
        title: Some("Swap Track".into()),
        artist: Some("A".into()),
        album: Some("B".into()),
        album_artist: None,
        duration_ms: Some(10_000),
        format: AudioFormat::Flac,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: None,
        year: Some(2026),
        track_number: Some(1),
        disc_number: Some(1),
        content_hash: Some(sha_v1.clone()),
        file_size: Some(size_v1 as u64),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(&pool, &reg_track).await.unwrap();

    let source_id = state.identity.michi_id().to_base64url();
    let transfer_id = format!("tx-swap-{}", Uuid::new_v4());
    let sel_manifest = michi_api::aircat_grants::Manifest {
        manifest_version: 1,
        transfer_id: transfer_id.clone(),
        source_device_id: source_id.clone(),
        total_tracks: 1,
        total_bytes: size_v1,
        items: vec![michi_api::aircat_grants::FileItem {
            logical_track_id: reg_track_id.to_string(),
            remote_track_id: reg_track_id.to_string(),
            title: "Swap Track".into(),
            artist: "A".into(),
            album: "B".into(),
            format: "flac".into(),
            size: size_v1,
            sha256: sha_v1,
        }],
    };
    let manifest_sha = michi_api::aircat_grants::manifest_sha256(&sel_manifest).unwrap();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let dest_member_id = "lhlxbEOUgOykbYw6mg4McXy75xBWBX5HOn8pVbixqbY";
    let auth_req = michi_api::aircat_grants::Request {
        intent: michi_api::aircat_grants::Intent {
            transfer_id,
            source_michi_id: source_id,
            destination_michi_id: dest_member_id.into(),
            source_library_id: "lib-src".into(),
            destination_library_id: "lib-dst".into(),
            selection_scope: "ALBUMS".into(),
            manifest_version: 1,
            required_scopes: vec!["content:read".into(), "content:transfer".into()],
            expires_at: now_ms + 300_000,
            signed_grant: None,
            authorization_nonce_base64_url: String::new(),
        },
        manifest: sel_manifest,
        manifest_sha256: manifest_sha,
        client_nonce_base64_url: URL_SAFE_NO_PAD.encode([5u8; 24]),
    };

    let auth_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/authorize")
                .method("POST")
                .header(header::AUTHORIZATION, &auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&auth_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth_resp.status(), StatusCode::OK);
    let grant_bytes = axum::body::to_bytes(auth_resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let grant_header = URL_SAFE_NO_PAD.encode(&grant_bytes);

    // Swap file atomically with a DIFFERENT file of the EXACT SAME byte length (16 bytes)
    let tmp_swap = music_dir.join(format!("tmp-swap-{}", Uuid::new_v4()));
    std::fs::write(&tmp_swap, b"BBBB_MODIFIED_V2").unwrap();
    std::fs::rename(&tmp_swap, &regular_path).unwrap();

    // 3. Rejects gzip-only Accept-Encoding (406)
    let resp_gzip = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/aircat/download/{reg_track_id}"))
                .method("GET")
                .header(header::AUTHORIZATION, &auth)
                .header("X-Michi-AirCat-Grant", &grant_header)
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_gzip.status(), StatusCode::NOT_ACCEPTABLE);

    // 4. Download must detect the inode/mtime swap and fail with 409 Conflict
    let resp_swap = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/aircat/download/{reg_track_id}"))
                .method("GET")
                .header(header::AUTHORIZATION, &auth)
                .header("X-Michi-AirCat-Grant", &grant_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_swap.status(), StatusCode::CONFLICT);
}
