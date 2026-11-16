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

#[tokio::test]
async fn test_aircat_home_membership_mutual_auth_and_audience_binding() {
    let (app, pool, state, _admin_token, music_dir) = make_app().await;

    // Provision a real Mobile member identity + Home membership certificate
    let client_sk = ed25519_dalek::SigningKey::from_bytes(&[0x77u8; 32]);
    let client_pk_b64 = URL_SAFE_NO_PAD.encode(client_sk.verifying_key().to_bytes());
    let client_michi_id =
        URL_SAFE_NO_PAD.encode(blake3::hash(&client_sk.verifying_key().to_bytes()).as_bytes());
    let home_id = state.home_root_authority.home_id();

    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &client_michi_id,
        &client_pk_b64,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        "2026-11-16T00:00:00Z",
        10,
    );

    // 1. Request challenge
    let chal_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::types::DeviceAuthChallengeRequest {
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk_b64.clone(),
                        home_id: home_id.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(chal_resp.status(), StatusCode::OK);
    let chal_json: michi_identity::types::DeviceAuthChallengeResponse = serde_json::from_slice(
        &axum::body::to_bytes(chal_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();

    // 2. Sign challenge and establish HOME_MEMBERSHIP session
    let sig = michi_identity::home::sign_device_auth_challenge(
        &client_sk,
        &home_id,
        &chal_json.server_michi_id,
        &client_michi_id,
        &chal_json.challenge_id.to_string(),
        &chal_json.challenge_nonce,
    );
    let sess_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/session")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::types::DeviceAuthSessionRequest {
                        challenge_id: chal_json.challenge_id,
                        client_michi_id: client_michi_id.clone(),
                        membership,
                        client_signature: sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sess_resp.status(), StatusCode::OK);
    let sess_json: michi_identity::types::DeviceAuthSessionResponse = serde_json::from_slice(
        &axum::body::to_bytes(sess_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();

    let server_ok = michi_identity::home::verify_server_auth_confirm(
        &chal_json.server_public_key,
        &sess_json.server_signature,
        &home_id,
        &chal_json.server_michi_id,
        &client_michi_id,
        &chal_json.challenge_id.to_string(),
        &chal_json.challenge_nonce,
    )
    .unwrap();
    assert!(server_ok);

    // 3. Access AirCat manifest with the HOME_MEMBERSHIP bearer token
    let member_auth = format!("Bearer {}", sess_json.session_token);
    let manifest_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/manifest")
                .method("GET")
                .header(header::AUTHORIZATION, &member_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manifest_resp.status(), StatusCode::OK);

    // 4. Attempting to authorize for a DIFFERENT destinationMichiId MUST fail with 403 WRONG_AUDIENCE
    let filename = format!("home-auth-{}.flac", Uuid::new_v4());
    let (sha, size, path) = create_test_file(&music_dir, &filename, b"HOME_AUTH_BOUND_BYTES");
    let track_id = michi_core::track_id_from_library_path(&music_dir, &path);
    michi_db::upsert_track(
        &pool,
        &Track {
            id: track_id,
            file_path: path.to_string_lossy().to_string(),
            title: Some("Home Auth".into()),
            artist: Some("A".into()),
            album: Some("B".into()),
            album_artist: None,
            duration_ms: Some(1000),
            format: AudioFormat::Flac,
            sample_rate: Some(48000),
            bit_depth: Some(16),
            channels: Some(2),
            artwork_id: None,
            genre: None,
            year: Some(2026),
            track_number: Some(1),
            disc_number: None,
            content_hash: Some(sha.clone()),
            file_size: Some(size as u64),
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
    )
    .await
    .unwrap();

    let source_id = state.identity.michi_id().to_base64url();
    let sel_manifest = michi_api::aircat_grants::Manifest {
        manifest_version: 1,
        transfer_id: "tx-spoof-check".into(),
        source_device_id: source_id.clone(),
        total_tracks: 1,
        total_bytes: size,
        items: vec![michi_api::aircat_grants::FileItem {
            logical_track_id: track_id.to_string(),
            remote_track_id: track_id.to_string(),
            title: "Home Auth".into(),
            artist: "A".into(),
            album: "B".into(),
            format: "flac".into(),
            size,
            sha256: sha,
        }],
    };
    let manifest_sha = michi_api::aircat_grants::manifest_sha256(&sel_manifest).unwrap();
    let spoofed_req = michi_api::aircat_grants::Request {
        intent: michi_api::aircat_grants::Intent {
            transfer_id: "tx-spoof-check".into(),
            source_michi_id: source_id,
            destination_michi_id: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".into(),
            source_library_id: "lib-src".into(),
            destination_library_id: "lib-dst".into(),
            selection_scope: "ALBUMS".into(),
            manifest_version: 1,
            required_scopes: vec!["content:read".into(), "content:transfer".into()],
            expires_at: chrono::Utc::now().timestamp_millis() + 300_000,
            signed_grant: None,
            authorization_nonce_base64_url: String::new(),
        },
        manifest: sel_manifest,
        manifest_sha256: manifest_sha,
        client_nonce_base64_url: URL_SAFE_NO_PAD.encode([9u8; 24]),
    };

    let spoof_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/authorize")
                .method("POST")
                .header(header::AUTHORIZATION, &member_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&spoofed_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(spoof_resp.status(), StatusCode::FORBIDDEN);
}

#[derive(Debug)]
struct TestSpkiPinVerifier {
    expected_spki_pin: String,
}

impl rustls::client::danger::ServerCertVerifier for TestSpkiPinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let actual = michi_api::extract_spki_sha256_from_pem(end_entity.as_ref())
            .ok_or_else(|| rustls::Error::General("missing SPKI".into()))?;
        if actual == self.expected_spki_pin {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(format!(
                "SPKI pin mismatch: expected {}, got {}",
                self.expected_spki_pin, actual
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}

#[tokio::test]
async fn test_aircat_real_tls_tcp_spki_pin_handshake() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TEST_CERT_PEM: &str = "-----BEGIN CERTIFICATE-----\n\
MIIBfDCCASOgAwIBAgIUX7+g6Mk29y5KfTOGXHe5zyKqbhkwCgYIKoZIzj0EAwIw\n\
FDESMBAGA1UEAwwJbG9jYWxob3N0MB4XDTI2MTExNjEwNTYzMFoXDTM2MTExMzEw\n\
NTYzMFowFDESMBAGA1UEAwwJbG9jYWxob3N0MFkwEwYHKoZIzj0CAQYIKoZIzj0D\n\
AQcDQgAE9b6n1FluIRt/L4sx6mYwC20FZ6V0Ne0bahTAmzNlaJzfBzkeAo1rcPXW\n\
Hod/mKFcAbKHkhorJgM1Fh7CmHpS8qNTMFEwHQYDVR0OBBYEFAduYBSvyRtn8UeM\n\
laec+KX3P9EvMB8GA1UdIwQYMBaAFAduYBSvyRtn8UeMlaec+KX3P9EvMA8GA1Ud\n\
EwEB/wQFMAMBAf8wCgYIKoZIzj0EAwIDRwAwRAIgWElFfyZMxUAzFn7KTWn8Dmij\n\
nMMDCY8mkO05LpzdwBwCIDve7yHZxu7MzWPGpJKO0wfE7N9JWrOLOC9ZVClYaR9h\n\
-----END CERTIFICATE-----\n";
    const TEST_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg0xUZgFY/U5uw8OWx\n\
RNNxv6f+JRUS/DdOdg1MtwyavMahRANCAAT1vqfUWW4hG38vizHqZjALbQVnpXQ1\n\
7RtqFMCbM2VonN8HOR4CjWtw9dYeh3+YoVwBsoeSGismAzUWHsKYelLy\n\
-----END PRIVATE KEY-----\n";

    let _ = rustls::crypto::ring::default_provider().install_default();
    let expected_spki = michi_api::extract_spki_sha256_from_pem(TEST_CERT_PEM.as_bytes()).unwrap();
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut TEST_CERT_PEM.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    let key = rustls_pemfile::private_key(&mut TEST_KEY_PEM.as_bytes())
        .unwrap()
        .unwrap();
    let server_tls_cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_tls_cfg));

    let (app, _pool, _state, token, _music_dir) = make_app().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server_task = tokio::spawn(async move {
        for _ in 0..2 {
            let Ok((tcp, peer)) = listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let Ok(tls_stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let io = hyper_util::rt::TokioIo::new(tls_stream);
                let svc = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let mut axum_req = req.map(axum::body::Body::new);
                        axum_req
                            .extensions_mut()
                            .insert(axum::extract::ConnectInfo(peer));
                        axum_req
                            .extensions_mut()
                            .insert(michi_api::AirCatTlsVerifiedConnection);
                        let svc = app.clone();
                        async move { tower::ServiceExt::oneshot(svc, axum_req).await }
                    },
                );
                let _ = hyper_util::server::conn::auto::Builder::new(
                    hyper_util::rt::TokioExecutor::new(),
                )
                .serve_connection(io, svc)
                .await;
            });
        }
    });

    // 1. Valid SPKI pin connects and receives full /api/v1/aircat/manifest response from real Axum router
    let ok_cfg = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(TestSpkiPinVerifier {
            expected_spki_pin: expected_spki,
        }))
        .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(ok_cfg));
    let tcp_ok = tokio::net::TcpStream::connect(addr).await.unwrap();
    let domain = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut tls_ok = connector.connect(domain.clone(), tcp_ok).await.unwrap();
    let req_wire = format!(
        "GET /api/v1/aircat/manifest HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    tls_ok.write_all(req_wire.as_bytes()).await.unwrap();
    let mut resp_buf = Vec::new();
    let _ = tls_ok.read_to_end(&mut resp_buf).await;
    let resp_str = String::from_utf8_lossy(&resp_buf);
    assert!(resp_str.contains("200 OK"), "resp={resp_str}");
    let lower_resp = resp_str.to_ascii_lowercase();
    assert!(lower_resp.contains("x-michi-aircat-protocol: michi.aircat.original.v1"));
    assert!(resp_str.contains("\"schema\":\"michi.aircat.manifest.v1\""));

    // 2. Mismatched SPKI pin rejects TLS handshake before sending HTTP bytes
    let bad_cfg = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(TestSpkiPinVerifier {
            expected_spki_pin: "sha256/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=".to_string(),
        }))
        .with_no_client_auth();
    let bad_connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(bad_cfg));
    let tcp_bad = tokio::net::TcpStream::connect(addr).await.unwrap();
    assert!(bad_connector.connect(domain, tcp_bad).await.is_err());

    server_task.abort();
}

#[tokio::test]
async fn test_aircat_midstream_revocation_aborts_large_stream_and_tracks_cache() {
    use futures_util::StreamExt;
    let (app, pool, state, admin_token, music_dir) = make_app().await;
    let admin_auth = format!("Bearer {admin_token}");

    // 1. Provision Home member and create 256 KiB (4 x 64 KiB chunks) track
    let client_sk = ed25519_dalek::SigningKey::from_bytes(&[0x55u8; 32]);
    let client_pk_b64 = URL_SAFE_NO_PAD.encode(client_sk.verifying_key().to_bytes());
    let client_michi_id =
        URL_SAFE_NO_PAD.encode(blake3::hash(&client_sk.verifying_key().to_bytes()).as_bytes());
    let home_id = state.home_root_authority.home_id();
    let membership = state.home_root_authority.issue_membership(
        &home_id,
        &client_michi_id,
        &client_pk_b64,
        "mobile",
        vec![michi_identity::Role::MobilePlayer],
        "2026-11-16T00:00:00Z",
        11,
    );

    let big_bytes = vec![0xABu8; 256 * 1024];
    let filename = format!("large-{}.flac", Uuid::new_v4());
    let (sha, size, path) = create_test_file(&music_dir, &filename, &big_bytes);
    let track_id = michi_core::track_id_from_library_path(&music_dir, &path);
    michi_db::upsert_track(
        &pool,
        &Track {
            id: track_id,
            file_path: path.to_string_lossy().to_string(),
            title: Some("Large Track".into()),
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
            disc_number: None,
            content_hash: Some(sha.clone()),
            file_size: Some(size as u64),
            file_mtime_ns: None,
            starred: false,
            rating: 0,
            starred_at: None,
            replaygain_track_gain: None,
            replaygain_track_peak: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
    )
    .await
    .unwrap();

    // Establish HOME_MEMBERSHIP session
    let chal_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/challenge")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::types::DeviceAuthChallengeRequest {
                        client_michi_id: client_michi_id.clone(),
                        client_public_key: client_pk_b64.clone(),
                        home_id: home_id.clone(),
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let chal: michi_identity::types::DeviceAuthChallengeResponse = serde_json::from_slice(
        &axum::body::to_bytes(chal_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let sig = michi_identity::home::sign_device_auth_challenge(
        &client_sk,
        &home_id,
        &chal.server_michi_id,
        &client_michi_id,
        &chal.challenge_id.to_string(),
        &chal.challenge_nonce,
    );
    let sess_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/session")
                .method("POST")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&michi_identity::types::DeviceAuthSessionRequest {
                        challenge_id: chal.challenge_id,
                        client_michi_id: client_michi_id.clone(),
                        membership,
                        client_signature: sig,
                    })
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let sess: michi_identity::types::DeviceAuthSessionResponse = serde_json::from_slice(
        &axum::body::to_bytes(sess_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let member_auth = format!("Bearer {}", sess.session_token);

    // 2. Call /manifest twice -> second call must hit SHA cache
    for _ in 0..2 {
        let m_resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/aircat/manifest")
                    .method("GET")
                    .header(header::AUTHORIZATION, &member_auth)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(m_resp.status(), StatusCode::OK);
    }

    // 3. Authorize with expiresAt > 600s -> must clamp to 600s instead of failing 404
    let source_id = state.identity.michi_id().to_base64url();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let sel_manifest = michi_api::aircat_grants::Manifest {
        manifest_version: 1,
        transfer_id: "tx-midstream-256k".into(),
        source_device_id: source_id.clone(),
        total_tracks: 1,
        total_bytes: size,
        items: vec![michi_api::aircat_grants::FileItem {
            logical_track_id: track_id.to_string(),
            remote_track_id: track_id.to_string(),
            title: "Large Track".into(),
            artist: "A".into(),
            album: "B".into(),
            format: "flac".into(),
            size,
            sha256: sha.clone(),
        }],
    };
    let manifest_sha = michi_api::aircat_grants::manifest_sha256(&sel_manifest).unwrap();
    let auth_req = michi_api::aircat_grants::Request {
        intent: michi_api::aircat_grants::Intent {
            transfer_id: "tx-midstream-256k".into(),
            source_michi_id: source_id,
            destination_michi_id: client_michi_id.clone(),
            source_library_id: "lib-src".into(),
            destination_library_id: "lib-dst".into(),
            selection_scope: "ALBUMS".into(),
            manifest_version: 1,
            required_scopes: vec!["content:read".into(), "content:transfer".into()],
            expires_at: now_ms + 900_000,
            signed_grant: None,
            authorization_nonce_base64_url: String::new(),
        },
        manifest: sel_manifest,
        manifest_sha256: manifest_sha,
        client_nonce_base64_url: URL_SAFE_NO_PAD.encode([0x33u8; 24]),
    };
    let auth_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/authorize")
                .method("POST")
                .header(header::AUTHORIZATION, &member_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&auth_req).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(auth_resp.status(), StatusCode::OK);
    let grant_bytes = axum::body::to_bytes(auth_resp.into_body(), 64 * 1024)
        .await
        .unwrap();
    let grant: michi_api::aircat_grants::Grant = serde_json::from_slice(&grant_bytes).unwrap();
    assert!(grant.expires_at_millis - grant.issued_at_millis <= 600_000);
    let grant_header = URL_SAFE_NO_PAD.encode(&grant_bytes);

    // 4. Test 304 Not Modified on If-None-Match
    let etag = format!("\"sha256-{sha}-{size}\"");
    let resp_304 = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/aircat/download/{track_id}"))
                .method("GET")
                .header(header::AUTHORIZATION, &member_auth)
                .header("X-Michi-AirCat-Grant", &grant_header)
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp_304.status(), StatusCode::NOT_MODIFIED);

    // 5. Start full download, read 1 chunk (64 KiB), then revoke via HTTP /api/v1/home/revoke
    let dl_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/aircat/download/{track_id}"))
                .method("GET")
                .header(header::AUTHORIZATION, &member_auth)
                .header("X-Michi-AirCat-Grant", &grant_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(dl_resp.status(), StatusCode::OK);
    let mut body_stream = dl_resp.into_body().into_data_stream();

    let first_chunk = body_stream.next().await.unwrap().unwrap();
    assert_eq!(first_chunk.len(), 64 * 1024);

    let revoke_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/home/revoke")
                .method("POST")
                .header(header::AUTHORIZATION, &admin_auth)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&serde_json::json!({
                        "device_michi_id": client_michi_id,
                        "reason": "midstream_killcritic_test"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(revoke_resp.status(), StatusCode::OK);

    // Next chunk MUST fail with stream error rather than delivering all 256 KiB
    let second_chunk = body_stream.next().await.unwrap();
    assert!(second_chunk.is_err());

    // 6. Verify /api/v1/aircat/status telemetry recorded cache hit & midstream abort
    let st_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/aircat/status")
                .method("GET")
                .header(header::AUTHORIZATION, &admin_auth)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(st_resp.status(), StatusCode::OK);
    let st_json: Value = serde_json::from_slice(
        &axum::body::to_bytes(st_resp.into_body(), 64 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(st_json["stats"]["digest_cache_hits"].as_u64().unwrap() >= 1);
    assert!(st_json["stats"]["midstream_revocations"].as_u64().unwrap() >= 1);
}

#[tokio::test]
async fn test_aircat_cleartext_http_rejected_when_tls_active() {
    let (app, _pool, state, token, _music_dir) = make_app().await;
    let cert_der = sample_x509_der_cert();
    *state.aircat_tls.write().await = Some(michi_api::AirCatTlsInfo {
        pinned_base_url: "https://127.0.0.1:9443".into(),
        tls_spki_sha256: michi_api::extract_spki_sha256_from_pem(&cert_der).unwrap(),
        cert_path: std::path::PathBuf::from("/tmp/c.crt"),
        key_path: std::path::PathBuf::from("/tmp/c.key"),
        tls_port: 9443,
    });

    // Bind a real plain HTTP TCP listener to verify ConnectInfo without AirCatTlsVerifiedConnection returns 403 TLS_REQUIRED
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let srv = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
    let wire = format!(
        "GET /api/v1/aircat/manifest HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    tcp.write_all(wire.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    let _ = tcp.read_to_end(&mut out).await;
    let out_str = String::from_utf8_lossy(&out);
    assert!(out_str.contains("403 Forbidden"), "out={out_str}");
    assert!(out_str.contains("TLS_REQUIRED"));

    srv.abort();
}
