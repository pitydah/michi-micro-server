//! KILLCRITIC R1.4 §13 — Independent Pre-Mobile Network Reference Client E2E
//!
//! Acts strictly as an external network client over REAL TCP HTTP and REAL
//! `tokio-rustls` HTTPS (with an OpenSSL CA-chained leaf certificate and an
//! explicit CA trust store — NO `danger_accept_invalid_certs`, NO internal
//! `AppState` auth shortcuts).
//!
//! Exercises all 19 mandatory interoperability phases and emits a verifiable
//! structured receipt to `target/release-evidence/pre-mobile-reference-client-receipt.json`
//! (or `MICHI_PRE_MOBILE_RECEIPT_PATH` when set by CI).
//!
//! HONESTY BOUNDARY: This proves server + protocol readiness via a network
//! reference client. It does NOT certify Michi Music Mobile (Android) itself.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::http::StatusCode;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use ed25519_dalek::SigningKey;
use michi_api::{
    create_router, validate_x509_for_home_tls, AirCatTlsInfo, AirCatTlsVerifiedConnection,
    AndroidTrustVerificationStatus, AppState, TlsCertificateProfile, TlsTrustMode,
};
use michi_config::Config;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

struct OpensslCaFixture {
    dir: PathBuf,
    ca_cert_pem: Vec<u8>,
    fullchain_pem: Vec<u8>,
    leaf_key_pem: Vec<u8>,
}

fn generate_ca_chained_openssl_fixture(prefix: &str) -> OpensslCaFixture {
    let dir = std::env::temp_dir().join(format!("michi-{prefix}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();

    let run_openssl = |args: &[&str]| {
        let out = Command::new("openssl")
            .args(args)
            .output()
            .expect("openssl binary must be installed");
        assert!(
            out.status.success(),
            "openssl {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };

    let ca_key = dir.join("ca.key");
    let ca_crt = dir.join("ca.crt");
    let leaf_key = dir.join("leaf.key");
    let leaf_csr = dir.join("leaf.csr");
    let leaf_crt = dir.join("leaf.crt");
    let ext_file = dir.join("leaf.ext");

    run_openssl(&[
        "req",
        "-x509",
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:prime256v1",
        "-nodes",
        "-keyout",
        ca_key.to_str().unwrap(),
        "-out",
        ca_crt.to_str().unwrap(),
        "-days",
        "30",
        "-subj",
        "/CN=Michi R1.4 Reference Test CA",
        "-addext",
        "basicConstraints=critical,CA:TRUE",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
    ]);

    run_openssl(&[
        "req",
        "-newkey",
        "ec",
        "-pkeyopt",
        "ec_paramgen_curve:prime256v1",
        "-nodes",
        "-keyout",
        leaf_key.to_str().unwrap(),
        "-out",
        leaf_csr.to_str().unwrap(),
        "-subj",
        "/CN=localhost",
    ]);

    std::fs::write(
        &ext_file,
        "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1\n",
    )
    .unwrap();

    run_openssl(&[
        "x509",
        "-req",
        "-in",
        leaf_csr.to_str().unwrap(),
        "-CA",
        ca_crt.to_str().unwrap(),
        "-CAkey",
        ca_key.to_str().unwrap(),
        "-CAcreateserial",
        "-out",
        leaf_crt.to_str().unwrap(),
        "-days",
        "30",
        "-extfile",
        ext_file.to_str().unwrap(),
    ]);

    let ca_cert_pem = std::fs::read(&ca_crt).unwrap();
    let leaf_cert_pem = std::fs::read(&leaf_crt).unwrap();
    let leaf_key_pem = std::fs::read(&leaf_key).unwrap();
    let mut fullchain_pem = leaf_cert_pem;
    fullchain_pem.extend_from_slice(&ca_cert_pem);

    OpensslCaFixture {
        dir,
        ca_cert_pem,
        fullchain_pem,
        leaf_key_pem,
    }
}

fn gen_client_keypair() -> (SigningKey, String, String) {
    let mut rng = rand::rngs::OsRng;
    let sk = SigningKey::generate(&mut rng);
    let vk = sk.verifying_key();
    let pk_b64 = URL_SAFE_NO_PAD.encode(vk.to_bytes());
    let mid = michi_identity::MichiId::from_public_key(&vk).to_base64url();
    (sk, pk_b64, mid)
}

async fn seed_audio_track(
    pool: &sqlx::SqlitePool,
    music_dir: &Path,
    name: &str,
) -> (Uuid, Vec<u8>, String) {
    let path = music_dir.join(format!("{name}.wav"));
    let pcm_len: usize = 4096;
    let mut wav = Vec::with_capacity(44 + pcm_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36u32 + pcm_len as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&48000u32.to_le_bytes());
    wav.extend_from_slice(&(48000u32 * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(pcm_len as u32).to_le_bytes());
    for i in 0..pcm_len {
        wav.push(((i * 37 + name.len() * 13) & 0xff) as u8);
    }
    std::fs::write(&path, &wav).unwrap();

    let sha256 = hex::encode(Sha256::digest(&wav));
    let track = michi_core::Track {
        id: michi_core::track_id_from_library_path(music_dir, &path),
        title: Some(name.to_string()),
        artist: Some("Michi Reference".to_string()),
        album: Some("R1.4 E2E".to_string()),
        album_artist: Some("Michi Reference".to_string()),
        duration_ms: Some(15_000),
        file_path: path.to_string_lossy().to_string(),
        format: michi_core::AudioFormat::Wav,
        sample_rate: Some(48000),
        bit_depth: Some(16),
        channels: Some(2),
        artwork_id: None,
        genre: Some("Reference".to_string()),
        year: Some(2026),
        track_number: Some(1),
        disc_number: Some(1),
        content_hash: Some(sha256.clone()),
        file_size: Some(wav.len() as u64),
        file_mtime_ns: None,
        starred: false,
        rating: 0,
        starred_at: None,
        replaygain_track_gain: None,
        replaygain_track_peak: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    michi_db::upsert_track(pool, &track).await.unwrap();
    (track.id, wav, sha256)
}

fn compile_home_roster_schema() -> jsonschema::Validator {
    let schema_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor/michi-link/schemas")
        .canonicalize()
        .expect("vendor/michi-link/schemas must exist");
    let target_json: Value = serde_json::from_str(
        &std::fs::read_to_string(schema_dir.join("home-device-roster.schema.json")).unwrap(),
    )
    .unwrap();

    let mut options = jsonschema::ValidationOptions::default();
    for entry in std::fs::read_dir(&schema_dir)
        .unwrap()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let json: Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            if let Some(id) = json.get("$id").and_then(|v| v.as_str()).map(str::to_string) {
                if let Ok(resource) = jsonschema::Resource::from_contents(json) {
                    options.with_resource(id, resource);
                }
            }
        }
    }
    options.build(&target_json).unwrap()
}

#[tokio::test]
async fn test_r14_pre_mobile_reference_client_full_19_phase_e2e() {
    let _ = rustls::crypto::ring::default_provider().install_default();

    let mut phases: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    let fx = generate_ca_chained_openssl_fixture("r14-ref-client");
    let untrusted_fx = generate_ca_chained_openssl_fixture("r14-untrusted-ca");

    let tmp = std::env::temp_dir().join(format!("michi-r14-ref-e2e-{}", Uuid::new_v4()));
    let music_dir = tmp.join("music");
    let config_dir = tmp.join("config");
    let cache_dir = tmp.join("cache");
    std::fs::create_dir_all(&music_dir).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();

    let db_url = format!("sqlite://{}", tmp.join("ref_e2e.db").display());
    let pool = michi_db::init_pool(&db_url).await.unwrap();

    let admin_user = "admin";
    let admin_pass = "SuperSecretPass123!";
    let admin_hash = michi_api::auth::hash_password(admin_pass).unwrap();
    let admin_id = Uuid::new_v4();
    michi_db::create_user(&pool, &admin_id, admin_user, &admin_hash, true)
        .await
        .unwrap();

    let cfg = Config {
        port: 9090,
        music_paths: vec![music_dir.clone()],
        config_path: config_dir.clone(),
        cache_path: cache_dir,
        database_url: db_url,
        version: "1.0.0-rc.4",
        sync_peers: Vec::new(),
        sync_name: "michi-r14-ref-server".to_string(),
        listenbrainz_token: None,
        lastfm_token: None,
        scrobble_enabled: false,
        auth_username: Some(admin_user.to_string()),
        auth_password: Some(admin_pass.to_string()),
        auth_enabled: true,
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
        trusted_proxies: Vec::new(),
        deployment_platform: "docker".into(),
    };

    let identity = Arc::new(
        michi_identity::IdentityManager::load_or_generate(&cfg.config_path, "Ref Server", "pwd")
            .unwrap(),
    );
    let state = AppState::new_with_identity(cfg, pool.clone(), Some(admin_id), identity.clone());

    // Seed 3 real audio tracks on disk + SQLite
    let (track_1, track_1_bytes, track_1_sha) = seed_audio_track(&pool, &music_dir, "ref_01").await;
    let (track_2, _t2_bytes, _t2_sha) = seed_audio_track(&pool, &music_dir, "ref_02").await;
    let (track_3, _t3_bytes, _t3_sha) = seed_audio_track(&pool, &music_dir, "ref_03").await;

    // Bind real TCP HTTP + real tokio-rustls HTTPS listeners
    let http_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http_addr = http_listener.local_addr().unwrap();
    let https_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let https_addr = https_listener.local_addr().unwrap();

    let base_http = format!("http://127.0.0.1:{}", http_addr.port());
    let base_https = format!("https://localhost:{}", https_addr.port());

    let verified_tls = validate_x509_for_home_tls(
        &fx.fullchain_pem,
        &fx.leaf_key_pem,
        &base_https,
        chrono::Utc::now(),
    )
    .unwrap();
    assert_eq!(verified_tls.tls_trust_mode, TlsTrustMode::CaChained);

    *state.aircat_tls.write().await = Some(AirCatTlsInfo {
        pinned_base_url: base_https.clone(),
        tls_spki_sha256: verified_tls.tls_spki_sha256.clone(),
        cert_path: fx.dir.join("leaf.crt"),
        key_path: fx.dir.join("leaf.key"),
        tls_port: https_addr.port(),
        tls_trust_mode: TlsTrustMode::CaChained,
        certificate_profile: TlsCertificateProfile::CaIssuedChainPresent,
        chain_cryptographically_verified: true,
        android_trust_verification_status: AndroidTrustVerificationStatus::NotVerified,
        cert_chain_len: 2,
        cert_not_before: Some(verified_tls.not_before.to_rfc3339()),
        cert_not_after: Some(verified_tls.not_after.to_rfc3339()),
    });
    state.set_aircat_tls_listener_ready(true);

    let certs: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls_pemfile::certs(&mut std::io::Cursor::new(&fx.fullchain_pem))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
    let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(&fx.leaf_key_pem))
        .unwrap()
        .unwrap();
    let server_tls_cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_tls_cfg));

    let app = create_router(state.clone());
    let http_app = app.clone();
    let http_task = tokio::spawn(async move {
        let _ = axum::serve(
            http_listener,
            http_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    let https_app = app.clone();
    let https_task = tokio::spawn(async move {
        loop {
            let Ok((tcp, peer)) = https_listener.accept().await else {
                break;
            };
            let acceptor = acceptor.clone();
            let app = https_app.clone();
            tokio::spawn(async move {
                let Ok(tls_stream) = acceptor.accept(tcp).await else {
                    return;
                };
                let io = hyper_util::rt::TokioIo::new(tls_stream);
                let svc = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        let mut axum_req = req.map(axum::body::Body::new);
                        axum_req.extensions_mut().insert(ConnectInfo(peer));
                        axum_req
                            .extensions_mut()
                            .insert(AirCatTlsVerifiedConnection);
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

    // External HTTP/HTTPS clients — explicit CA trust ONLY (NO danger_accept_invalid_certs!)
    let http_client = reqwest::Client::new();
    let trusted_ca = reqwest::Certificate::from_pem(&fx.ca_cert_pem).unwrap();
    let https_client = reqwest::Client::builder()
        .add_root_certificate(trusted_ca)
        .resolve("localhost", https_addr)
        .build()
        .unwrap();
    let wrong_ca = reqwest::Certificate::from_pem(&untrusted_fx.ca_cert_pem).unwrap();
    let untrusted_https_client = reqwest::Client::builder()
        .add_root_certificate(wrong_ca)
        .resolve("localhost", https_addr)
        .build()
        .unwrap();

    // ── PHASE 1: DISCOVERY ──────────────────────────────────────────────────
    let connect = michi_connect::MichiConnect::new(identity.clone(), http_addr.port(), None);
    connect.set_home_id(state.home_root_authority.home_id());
    let announce = connect
        .build_signed_announce(http_addr.port(), "127.0.0.1")
        .unwrap();
    let announce_bytes = serde_json::to_vec(&announce).unwrap();
    let parsed_announce: michi_identity::types::Announce =
        serde_json::from_slice(&announce_bytes).unwrap();
    let verifier_engine = michi_identity::discovery::DiscoveryEngine::new(identity.clone());
    let trust_level = verifier_engine
        .verify_announce(&parsed_announce, None)
        .unwrap();
    assert_eq!(
        trust_level,
        michi_identity::types::TrustLevel::Verified(identity.michi_id().to_base64url())
    );
    phases.insert("DISCOVERY", "PASS");

    // ── PHASE 2: SERVER_IDENTITY ────────────────────────────────────────────
    let server_info: Value = https_client
        .get(format!("{base_https}/api/v1/server/info"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let server_michi_id = server_info["michi_id"].as_str().unwrap().to_string();
    let server_public_key = server_info["public_key"].as_str().unwrap().to_string();
    let home_id = server_info["michi_home_id"].as_str().unwrap().to_string();
    assert_eq!(server_info["identity_scheme"], "ed25519-blake3-v1");
    assert_eq!(
        parsed_announce.michi_id.as_deref(),
        Some(server_michi_id.as_str())
    );
    assert_eq!(
        parsed_announce.public_key.as_deref(),
        Some(server_public_key.as_str())
    );
    // Independently recompute MichiId = BLAKE3(public_key) on the client
    let pk_raw = URL_SAFE_NO_PAD.decode(&server_public_key).unwrap();
    let derived_mid = URL_SAFE_NO_PAD.encode(blake3::hash(&pk_raw).as_bytes());
    assert_eq!(derived_mid, server_michi_id);
    phases.insert("SERVER_IDENTITY", "PASS");

    // ── PHASE 3: TLS ────────────────────────────────────────────────────────
    // 3a. Untrusted CA fails real TLS handshake
    let untrusted_attempt = untrusted_https_client
        .get(format!("{base_https}/api/v1/server/info"))
        .send()
        .await;
    assert!(
        untrusted_attempt.is_err(),
        "client with wrong CA must fail TLS verification"
    );

    let (client_sk, client_pk, client_mid) = gen_client_keypair();
    // 3b. Cleartext HTTP rejects enrollment even with spoofed X-Forwarded-Proto
    let clear_enr = http_client
        .post(format!("{base_http}/api/v1/home/enrollment/challenge"))
        .header("X-Forwarded-Proto", "https")
        .json(&michi_identity::HomeEnrollmentChallengeRequest {
            purpose: "home_enrollment".into(),
            home_id: home_id.clone(),
            client_michi_id: client_mid.clone(),
            client_public_key: client_pk.clone(),
        })
        .send()
        .await
        .unwrap();
    assert_eq!(clear_enr.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        clear_enr.json::<Value>().await.unwrap()["error"]["code"],
        "TLS_REQUIRED"
    );
    phases.insert("TLS", "PASS");

    // ── PHASE 4: SPKI ───────────────────────────────────────────────────────
    // Admin logs in over real HTTPS (/api/v1/auth/login — NO AppState shortcut)
    // and fetches the trusted AirCat enrollment QR carrying the pinned SPKI hash.
    let login_resp: Value = https_client
        .post(format!("{base_https}/api/auth/login"))
        .json(&json!({ "username": admin_user, "password": admin_pass }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let admin_token = login_resp["token"].as_str().unwrap().to_string();

    let enr_qr: Value = https_client
        .get(format!("{base_https}/api/v1/aircat/enrollment-qr"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let server_spki = enr_qr["tlsSpkiSha256"]
        .as_str()
        .expect("tlsSpkiSha256 must be published in enrollment-qr")
        .to_string();
    assert_eq!(server_spki, verified_tls.tls_spki_sha256);
    assert!(
        server_spki.starts_with("sha256/") && server_spki.len() == 51,
        "tlsSpkiSha256 must be canonical OkHttp pin format 'sha256/<44-char-base64>', got '{server_spki}'"
    );
    assert_eq!(enr_qr["homeId"], home_id);
    assert_eq!(enr_qr["sourceMichiId"], server_michi_id);
    phases.insert("SPKI", "PASS");

    // ── PHASE 5: ENROLLMENT_REQUEST ─────────────────────────────────────────
    let enr_ch: michi_identity::HomeEnrollmentChallengeResponse = https_client
        .post(format!("{base_https}/api/v1/home/enrollment/challenge"))
        .json(&michi_identity::HomeEnrollmentChallengeRequest {
            purpose: "home_enrollment".into(),
            home_id: home_id.clone(),
            client_michi_id: client_mid.clone(),
            client_public_key: client_pk.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();

    let enr_pop = michi_identity::sign_home_enrollment_pop(
        &client_sk,
        &home_id,
        &enr_ch.server_michi_id,
        &client_mid,
        &client_pk,
        &enr_ch.challenge_id.to_string(),
        &enr_ch.challenge_nonce,
    );
    let enr_req: michi_identity::HomeEnrollmentRequestResponse = https_client
        .post(format!("{base_https}/api/v1/home/enrollment/request"))
        .json(&michi_identity::HomeEnrollmentRequest {
            home_id: home_id.clone(),
            challenge_id: enr_ch.challenge_id,
            client_michi_id: client_mid.clone(),
            client_public_key: client_pk.clone(),
            device_name: "Pre-Mobile Ref Client".into(),
            device_type: "mobile".into(),
            requested_roles: vec![
                michi_identity::Role::MobilePlayer,
                michi_identity::Role::RemoteController,
            ],
            client_signature: enr_pop,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    phases.insert("ENROLLMENT_REQUEST", "PASS");

    // ── PHASE 6: ADMIN_APPROVAL ─────────────────────────────────────────────
    https_client
        .post(format!(
            "{base_https}/api/v1/home/enrollment/{}/approve",
            enr_req.request_id
        ))
        .bearer_auth(&admin_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    phases.insert("ADMIN_APPROVAL", "PASS");

    // ── PHASE 7: MEMBERSHIP_VERIFICATION ────────────────────────────────────
    let enr_status: michi_identity::HomeEnrollmentStatusResponse = https_client
        .get(format!(
            "{base_https}/api/v1/home/enrollment/{}/status",
            enr_req.request_id
        ))
        .header("X-Michi-Enrollment-Claim", &enr_req.claim_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let membership = enr_status.membership.expect("approved membership present");
    let home_root_pk = enr_status
        .home_root_public_key
        .expect("home_root_public_key present");
    michi_identity::home::verify_membership(&membership, &home_root_pk, &home_id, &[]).unwrap();
    phases.insert("MEMBERSHIP_VERIFICATION", "PASS");

    // ── PHASE 8: HOME_AUTHENTICATION ────────────────────────────────────────
    let auth_ch: michi_identity::DeviceAuthChallengeResponse = https_client
        .post(format!("{base_https}/api/v1/auth/challenge"))
        .json(&michi_identity::DeviceAuthChallengeRequest {
            client_michi_id: client_mid.clone(),
            client_public_key: client_pk.clone(),
            home_id: home_id.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let auth_sig = michi_identity::sign_device_auth_challenge(
        &client_sk,
        &home_id,
        &auth_ch.server_michi_id,
        &client_mid,
        &auth_ch.challenge_id.to_string(),
        &auth_ch.challenge_nonce,
    );
    let sess: michi_identity::DeviceAuthSessionResponse = https_client
        .post(format!("{base_https}/api/v1/auth/session"))
        .json(&michi_identity::DeviceAuthSessionRequest {
            challenge_id: auth_ch.challenge_id,
            client_michi_id: client_mid.clone(),
            membership: membership.clone(),
            client_signature: auth_sig,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    michi_identity::verify_server_auth_session(
        &sess,
        &home_root_pk,
        &home_id,
        &client_mid,
        &auth_ch.challenge_id.to_string(),
        &[],
    )
    .unwrap();
    assert!(sess.session_token.starts_with("michi_home_"));
    phases.insert("HOME_AUTHENTICATION", "PASS");

    // ── PHASE 9: SESSION_AUTHORIZATION ──────────────────────────────────────
    let tracks_https = https_client
        .get(format!("{base_https}/api/v1/tracks"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .send()
        .await
        .unwrap();
    assert_eq!(tracks_https.status(), StatusCode::OK);

    // Same token over cleartext HTTP -> 403 TLS_REQUIRED
    let tracks_http = http_client
        .get(format!("{base_http}/api/v1/tracks"))
        .bearer_auth(&sess.session_token)
        .send()
        .await
        .unwrap();
    assert_eq!(tracks_http.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        tracks_http.json::<Value>().await.unwrap()["error"]["code"],
        "TLS_REQUIRED"
    );

    // Unauthenticated call -> 401
    let unauth_tracks = https_client
        .get(format!("{base_https}/api/v1/tracks"))
        .send()
        .await
        .unwrap();
    assert_eq!(unauth_tracks.status(), StatusCode::UNAUTHORIZED);
    phases.insert("SESSION_AUTHORIZATION", "PASS");

    // ── PHASE 10: CAPABILITIES ──────────────────────────────────────────────
    let caps: Value = https_client
        .get(format!("{base_https}/api/v1/capabilities"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let cap_features = caps["features"].as_array().unwrap();
    for feat_name in ["library", "playback", "stream"] {
        let feat = cap_features
            .iter()
            .find(|f| f["name"] == feat_name)
            .unwrap_or_else(|| panic!("capabilities missing {feat_name}"));
        assert_eq!(feat["enabled"], true);
    }
    assert_eq!(server_info["features"]["library"], true);
    assert_eq!(server_info["features"]["playback"], true);
    assert_eq!(server_info["features"]["aircat_original_v1"], true);
    assert_eq!(server_info["features"]["home_tls_spki_pinned"], true);
    phases.insert("CAPABILITIES", "PASS");

    // ── PHASE 11: HOME_ROSTER ───────────────────────────────────────────────
    let roster_schema = compile_home_roster_schema();
    let roster: Value = https_client
        .get(format!("{base_https}/api/v1/home/roster"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        roster_schema.is_valid(&roster),
        "live HTTPS /api/v1/home/roster must match home-device-roster.schema.json"
    );
    let me = roster["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == client_mid)
        .expect("mobile client must appear in Home roster");
    assert_eq!(me["membership_status"], "enrolled");
    assert_eq!(me["authenticated"], true);
    assert_eq!(me["online"], true);
    assert_eq!(me["presence_state"], "online");
    assert_eq!(me["status"], "online");
    phases.insert("HOME_ROSTER", "PASS");

    // ── PHASE 12: MICHI_CONTROL ─────────────────────────────────────────────
    let set_vol = https_client
        .post(format!("{base_https}/api/v1/playback/control"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&json!({"command": "set_volume", "volume": 72}))
        .send()
        .await
        .unwrap();
    assert_eq!(set_vol.status(), StatusCode::OK);

    // Leading-zero volume rejected without mutating state
    let bad_vol = https_client
        .post(format!("{base_https}/api/v1/playback/control"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&json!({"command": "set_volume", "value": "075"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad_vol.status(), StatusCode::BAD_REQUEST);

    let pb_status: Value = https_client
        .get(format!("{base_https}/api/v1/playback/state"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pb_status["volume"], 72);
    phases.insert("MICHI_CONTROL", "PASS");

    // ── PHASE 13: QUEUE ─────────────────────────────────────────────────────
    let add_q = https_client
        .post(format!("{base_https}/api/v1/queue/items"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&json!({"track_ids": [track_1, track_2, track_3]}))
        .send()
        .await
        .unwrap();
    assert_eq!(add_q.status(), StatusCode::OK);

    let q1: Value = https_client
        .get(format!("{base_https}/api/v1/queue"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(q1["items_count"], 3);
    assert_eq!(q1["queue_degraded"], false);
    let q_items = q1["items"].as_array().unwrap();
    let id0 = q_items[0]["id"].as_str().unwrap().to_string();
    let id1 = q_items[1]["id"].as_str().unwrap().to_string();
    let id2 = q_items[2]["id"].as_str().unwrap().to_string();

    // Reorder: [id2, id0, id1]
    let reorder_resp = https_client
        .put(format!("{base_https}/api/v1/queue/reorder"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&json!({"item_ids": [id2, id0, id1]}))
        .send()
        .await
        .unwrap();
    assert_eq!(reorder_resp.status(), StatusCode::OK);

    // Delete middle item (id0) -> 2 remaining items with contiguous positions 0..1
    let del_resp = https_client
        .delete(format!("{base_https}/api/v1/queue/items/{id0}"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .send()
        .await
        .unwrap();
    assert_eq!(del_resp.status(), StatusCode::OK);

    let q2: Value = https_client
        .get(format!("{base_https}/api/v1/queue"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(q2["items_count"], 2);
    assert_eq!(q2["items"][0]["position"], 0);
    assert_eq!(q2["items"][1]["position"], 1);
    assert!(q2["queue_version"].as_u64().unwrap() > q1["queue_version"].as_u64().unwrap());
    phases.insert("QUEUE", "PASS");

    // Respect the real production per-IP rate limiter (10 rps / burst 20)
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;

    // ── PHASE 14: AIRCAT_MANIFEST ───────────────────────────────────────────
    let manifest_resp = https_client
        .get(format!("{base_https}/api/v1/aircat/manifest"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .send()
        .await
        .unwrap();
    assert_eq!(manifest_resp.status(), StatusCode::OK);
    assert_eq!(
        manifest_resp
            .headers()
            .get("X-Michi-AirCat-Protocol")
            .unwrap()
            .to_str()
            .unwrap(),
        "michi.aircat.original.v1"
    );
    let manifest_json: Value = manifest_resp.json().await.unwrap();
    let manifest_items = manifest_json["tracks"].as_array().unwrap();
    let target_item = manifest_items
        .iter()
        .find(|it| it["track_id"] == track_1.to_string())
        .expect("track_1 must appear in AirCat manifest");
    assert_eq!(
        target_item["size"].as_u64().unwrap(),
        track_1_bytes.len() as u64
    );
    assert_eq!(target_item["checksum"].as_str().unwrap(), track_1_sha);
    phases.insert("AIRCAT_MANIFEST", "PASS");

    // ── PHASE 15: AIRCAT_AUTHORIZATION ──────────────────────────────────────
    let transfer_id = format!("tx-ref-{}", Uuid::new_v4());
    let client_nonce = URL_SAFE_NO_PAD.encode([0x42u8; 24]);
    let now_ms = chrono::Utc::now().timestamp_millis();

    let grant_manifest = michi_api::aircat_grants::Manifest {
        manifest_version: 1,
        transfer_id: transfer_id.clone(),
        source_device_id: server_michi_id.clone(),
        total_tracks: 1,
        total_bytes: track_1_bytes.len() as i64,
        items: vec![michi_api::aircat_grants::FileItem {
            logical_track_id: "mobile-track-001".into(),
            remote_track_id: track_1.to_string(),
            title: "ref_01".into(),
            artist: "Michi Reference".into(),
            album: "R1.4 E2E".into(),
            format: "wav".into(),
            size: track_1_bytes.len() as i64,
            sha256: track_1_sha.clone(),
        }],
    };
    let canonical_manifest_sha =
        michi_api::aircat_grants::manifest_sha256(&grant_manifest).unwrap();
    let auth_req = michi_api::aircat_grants::Request {
        intent: michi_api::aircat_grants::Intent {
            transfer_id: transfer_id.clone(),
            source_michi_id: server_michi_id.clone(),
            destination_michi_id: client_mid.clone(),
            source_library_id: "lib-source-default".into(),
            destination_library_id: "lib-dest-mobile".into(),
            selection_scope: "TRACKS".into(),
            manifest_version: 1,
            required_scopes: vec!["content:read".into(), "content:transfer".into()],
            expires_at: now_ms + 300_000,
            signed_grant: None,
            authorization_nonce_base64_url: String::new(),
        },
        manifest: grant_manifest,
        manifest_sha256: canonical_manifest_sha.clone(),
        client_nonce_base64_url: client_nonce.clone(),
    };

    let grant_resp = https_client
        .post(format!("{base_https}/api/v1/aircat/authorize"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&auth_req)
        .send()
        .await
        .unwrap();
    assert_eq!(grant_resp.status(), StatusCode::OK);
    let grant_raw = grant_resp.bytes().await.unwrap();
    let grant: michi_api::aircat_grants::Grant = serde_json::from_slice(&grant_raw).unwrap();
    assert_eq!(grant.version, 1);
    assert_eq!(grant.home_id, home_id);
    assert_eq!(grant.destination_michi_id, client_mid);
    assert_eq!(grant.manifest_sha256, canonical_manifest_sha);
    assert_eq!(grant.nonce_base64_url, client_nonce);
    assert_eq!(grant.signature_base64_url.len(), 86);

    // Replay of identical nonce -> 409 Conflict
    let replay_resp = https_client
        .post(format!("{base_https}/api/v1/aircat/authorize"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&auth_req)
        .send()
        .await
        .unwrap();
    assert_eq!(replay_resp.status(), StatusCode::CONFLICT);
    phases.insert("AIRCAT_AUTHORIZATION", "PASS");

    // ── PHASE 16: AIRCAT_TRANSFER ───────────────────────────────────────────
    let grant_header = URL_SAFE_NO_PAD.encode(&grant_raw);
    let expected_grant_receipt = hex::encode(Sha256::digest(grant_header.as_bytes()));
    let expected_etag = format!("\"sha256-{}-{}\"", track_1_sha, track_1_bytes.len());

    let dl_resp = https_client
        .get(format!("{base_https}/api/v1/aircat/download/{track_1}"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .send()
        .await
        .unwrap();
    assert_eq!(dl_resp.status(), StatusCode::OK);
    assert_eq!(
        dl_resp
            .headers()
            .get("X-Michi-AirCat-Authorized-Grant-SHA256")
            .unwrap()
            .to_str()
            .unwrap(),
        expected_grant_receipt
    );
    assert_eq!(
        dl_resp.headers().get("etag").unwrap().to_str().unwrap(),
        expected_etag
    );
    let full_downloaded = dl_resp.bytes().await.unwrap();
    assert_eq!(full_downloaded.as_ref(), track_1_bytes.as_slice());
    assert_eq!(hex::encode(Sha256::digest(&full_downloaded)), track_1_sha);
    phases.insert("AIRCAT_TRANSFER", "PASS");

    // ── PHASE 17: AIRCAT_RANGE_RESUME ───────────────────────────────────────
    let part1_resp = https_client
        .get(format!("{base_https}/api/v1/aircat/download/{track_1}"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .header("Range", "bytes=0-1023")
        .header("If-Range", &expected_etag)
        .send()
        .await
        .unwrap();
    assert_eq!(part1_resp.status(), StatusCode::PARTIAL_CONTENT);
    let part1 = part1_resp.bytes().await.unwrap();
    assert_eq!(part1.len(), 1024);

    let part2_resp = https_client
        .get(format!("{base_https}/api/v1/aircat/download/{track_1}"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .header("X-Michi-AirCat-Grant", &grant_header)
        .header("Range", "bytes=1024-")
        .header("If-Range", &expected_etag)
        .send()
        .await
        .unwrap();
    assert_eq!(part2_resp.status(), StatusCode::PARTIAL_CONTENT);
    let part2 = part2_resp.bytes().await.unwrap();

    let mut reassembled = Vec::with_capacity(part1.len() + part2.len());
    reassembled.extend_from_slice(&part1);
    reassembled.extend_from_slice(&part2);
    assert_eq!(reassembled, track_1_bytes);
    assert_eq!(hex::encode(Sha256::digest(&reassembled)), track_1_sha);
    phases.insert("AIRCAT_RANGE_RESUME", "PASS");

    // Respect the real production per-IP rate limiter (10 rps / burst 20)
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    // ── PHASE 18: REVOCATION ────────────────────────────────────────────────
    https_client
        .post(format!("{base_https}/api/v1/home/revoke"))
        .bearer_auth(&admin_token)
        .json(&json!({"device_michi_id": client_mid, "reason": "r14_ref_revocation"}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Immediately fail-closed across library, AirCat, and playback control
    for uri in ["/api/v1/tracks", "/api/v1/aircat/manifest"] {
        let rev_get = https_client
            .get(format!("{base_https}{uri}"))
            .bearer_auth(&sess.session_token)
            .header("X-Michi-Device-Id", &client_mid)
            .send()
            .await
            .unwrap();
        assert!(
            rev_get.status() == StatusCode::UNAUTHORIZED
                || rev_get.status() == StatusCode::FORBIDDEN,
            "revoked token must be rejected on {uri}, got {}",
            rev_get.status()
        );
    }
    let rev_ctrl = https_client
        .post(format!("{base_https}/api/v1/playback/control"))
        .bearer_auth(&sess.session_token)
        .header("X-Michi-Device-Id", &client_mid)
        .json(&json!({"command": "set_volume", "volume": 10}))
        .send()
        .await
        .unwrap();
    assert!(
        rev_ctrl.status() == StatusCode::UNAUTHORIZED || rev_ctrl.status() == StatusCode::FORBIDDEN
    );

    let roster_after_rev: Value = https_client
        .get(format!("{base_https}/api/v1/home/roster"))
        .bearer_auth(&admin_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(roster_schema.is_valid(&roster_after_rev));
    let me_rev = roster_after_rev["devices"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["device_michi_id"] == client_mid)
        .unwrap();
    assert_eq!(me_rev["revoked"], true);
    assert_eq!(me_rev["authenticated"], false);
    assert_eq!(me_rev["online"], false);
    assert_eq!(me_rev["presence_state"], "revoked");
    phases.insert("REVOCATION", "PASS");

    // ── PHASE 19: RECONNECT ─────────────────────────────────────────────────
    // 19a. Revoked device CANNOT re-authenticate
    let rev_reauth = https_client
        .post(format!("{base_https}/api/v1/auth/challenge"))
        .json(&michi_identity::DeviceAuthChallengeRequest {
            client_michi_id: client_mid.clone(),
            client_public_key: client_pk.clone(),
            home_id: home_id.clone(),
        })
        .send()
        .await
        .unwrap();
    if rev_reauth.status() == StatusCode::OK {
        let ch2: michi_identity::DeviceAuthChallengeResponse = rev_reauth.json().await.unwrap();
        let sig2 = michi_identity::sign_device_auth_challenge(
            &client_sk,
            &home_id,
            &ch2.server_michi_id,
            &client_mid,
            &ch2.challenge_id.to_string(),
            &ch2.challenge_nonce,
        );
        let sess2 = https_client
            .post(format!("{base_https}/api/v1/auth/session"))
            .json(&michi_identity::DeviceAuthSessionRequest {
                challenge_id: ch2.challenge_id,
                client_michi_id: client_mid.clone(),
                membership: membership.clone(),
                client_signature: sig2,
            })
            .send()
            .await
            .unwrap();
        assert_eq!(sess2.status(), StatusCode::FORBIDDEN);
    } else {
        assert_eq!(rev_reauth.status(), StatusCode::FORBIDDEN);
    }

    // 19b. A fresh second non-revoked client enrolls & reconnects cleanly over the same HTTPS server
    let (sk2, pk2, mid2) = gen_client_keypair();
    let enr2_ch: michi_identity::HomeEnrollmentChallengeResponse = https_client
        .post(format!("{base_https}/api/v1/home/enrollment/challenge"))
        .json(&michi_identity::HomeEnrollmentChallengeRequest {
            purpose: "home_enrollment".into(),
            home_id: home_id.clone(),
            client_michi_id: mid2.clone(),
            client_public_key: pk2.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let pop2 = michi_identity::sign_home_enrollment_pop(
        &sk2,
        &home_id,
        &enr2_ch.server_michi_id,
        &mid2,
        &pk2,
        &enr2_ch.challenge_id.to_string(),
        &enr2_ch.challenge_nonce,
    );
    let enr2_req: michi_identity::HomeEnrollmentRequestResponse = https_client
        .post(format!("{base_https}/api/v1/home/enrollment/request"))
        .json(&michi_identity::HomeEnrollmentRequest {
            home_id: home_id.clone(),
            challenge_id: enr2_ch.challenge_id,
            client_michi_id: mid2.clone(),
            client_public_key: pk2.clone(),
            device_name: "Ref Client Reconnect #2".into(),
            device_type: "mobile".into(),
            requested_roles: vec![michi_identity::Role::MobilePlayer],
            client_signature: pop2,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    https_client
        .post(format!(
            "{base_https}/api/v1/home/enrollment/{}/approve",
            enr2_req.request_id
        ))
        .bearer_auth(&admin_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let st2: michi_identity::HomeEnrollmentStatusResponse = https_client
        .get(format!(
            "{base_https}/api/v1/home/enrollment/{}/status",
            enr2_req.request_id
        ))
        .header("X-Michi-Enrollment-Claim", &enr2_req.claim_token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let mem2 = st2.membership.unwrap();
    let ach2: michi_identity::DeviceAuthChallengeResponse = https_client
        .post(format!("{base_https}/api/v1/auth/challenge"))
        .json(&michi_identity::DeviceAuthChallengeRequest {
            client_michi_id: mid2.clone(),
            client_public_key: pk2.clone(),
            home_id: home_id.clone(),
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let asig2 = michi_identity::sign_device_auth_challenge(
        &sk2,
        &home_id,
        &ach2.server_michi_id,
        &mid2,
        &ach2.challenge_id.to_string(),
        &ach2.challenge_nonce,
    );
    let sess2: michi_identity::DeviceAuthSessionResponse = https_client
        .post(format!("{base_https}/api/v1/auth/session"))
        .json(&michi_identity::DeviceAuthSessionRequest {
            challenge_id: ach2.challenge_id,
            client_michi_id: mid2.clone(),
            membership: mem2,
            client_signature: asig2,
        })
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let tracks2 = https_client
        .get(format!("{base_https}/api/v1/tracks"))
        .bearer_auth(&sess2.session_token)
        .header("X-Michi-Device-Id", &mid2)
        .send()
        .await
        .unwrap();
    assert_eq!(tracks2.status(), StatusCode::OK);
    phases.insert("RECONNECT", "PASS");

    assert_eq!(phases.len(), 19, "all 19 reference phases must execute");
    assert!(phases.values().all(|v| *v == "PASS"));

    // Emit verifiable structured JSON receipt
    let receipt_path = std::env::var("MICHI_PRE_MOBILE_RECEIPT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/release-evidence/pre-mobile-reference-client-receipt.json")
        });
    if let Some(parent) = receipt_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let receipt = json!({
        "suite": "killcritic-r1.4-pre-mobile-reference-client-e2e",
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "transport": {
            "real_tcp_http": true,
            "real_tokio_rustls_https": true,
            "explicit_ca_trust_store_only": true,
            "danger_accept_invalid_certs": false,
            "internal_appstate_auth_shortcuts": false
        },
        "server_michi_id": server_michi_id,
        "home_id": home_id,
        "tls_spki_sha256": server_spki,
        "phases_total": phases.len(),
        "phases_passed": phases.values().filter(|v| **v == "PASS").count(),
        "phases": phases,
        "honesty_boundary": {
            "MICHI_MUSIC_MOBILE_INTEROP_IMPLEMENTATION": "NOT_IMPLEMENTED / NOT_AUDITED",
            "ANDROID_REAL_DEVICE_E2E": "NOT_RUN",
            "ZIMAOS_ANDROID_PHYSICAL_E2E": "NOT_RUN"
        }
    });
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).unwrap(),
    )
    .unwrap();

    http_task.abort();
    https_task.abort();
    let _ = std::fs::remove_dir_all(&tmp);
    let _ = std::fs::remove_dir_all(&fx.dir);
    let _ = std::fs::remove_dir_all(&untrusted_fx.dir);
}
