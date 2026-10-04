use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

use michi_api::AppState;
use michi_config::Config;
use michi_receivers::{ReceiverCapabilities, ReceiverPresence, ReceiverRegistryEntry};

#[tokio::test]
async fn test_receiver_capability_persistence_and_restart_round_trip() {
    let tmp = tempdir().unwrap();
    let db_path = tmp.path().join("restart_test.db");
    let db_url = format!("sqlite://{}", db_path.display());
    let config_dir = tmp.path().join("config");
    let cache_dir = tmp.path().join("cache");
    let music_dir = tmp.path().join("music");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::create_dir_all(&music_dir).unwrap();

    let pool = michi_db::init_pool(&db_url).await.unwrap();

    let config = Config {
        port: 9999,
        music_paths: vec![music_dir.clone()],
        config_path: config_dir.clone(),
        cache_path: cache_dir.clone(),
        database_url: db_url.clone(),
        version: "test",
        sync_peers: Vec::new(),
        sync_name: "test".to_string(),
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
    };

    let identity = Arc::new(
        michi_identity::IdentityManager::load_or_generate(&config_dir, "Test Server", "password")
            .unwrap(),
    );

    // ── AppState Instance 1: Pair and persist receiver ──────────────────
    let state1 = AppState::new_with_identity(config.clone(), pool.clone(), None, identity.clone());

    let receiver_id = "test-stream-esp32-01".to_string();
    let michi_id = "michi-stream-id-01".to_string();
    let raw_token = "valid-test-receiver-token-43chars-base64url".to_string();

    let custom_caps = ReceiverCapabilities {
        device_type: "standard".to_string(),
        supported_codecs: vec!["pcm_s16le".to_string()],
        max_sample_rate: 48000,
        max_bit_depth: 16,
        supported_transports: vec!["rtp_udp".to_string()],
        supported_sample_rates: vec![48000], // Strictly 48000, no fabricated 44100!
        supported_bit_depths: vec![16],
        supported_channels: vec![2],
        features: vec!["perch_v1".to_string()],
        authority_features: vec!["authority-v1".to_string()],
    };

    let entry1 = ReceiverRegistryEntry {
        receiver_id: receiver_id.clone(),
        michi_id: Some(michi_id.clone()),
        name: "Living Room ESP32 Stream".to_string(),
        device_type: "standard".to_string(),
        base_url: "http://192.168.1.150:8080/".to_string(),
        paired: true,
        token: Some(raw_token.clone()),
        presence: ReceiverPresence::VerifiedOnline,
        last_seen: Some(chrono::Utc::now()),
        capabilities: custom_caps.features.clone(),
        capabilities_verified_at: Some(chrono::Utc::now()),
        capabilities_stale: false,
        authority_supported: true,
        owner_michi_id: None,
        owner_name: None,
        active_session_id: None,
        max_sample_rate: custom_caps.max_sample_rate,
        max_bit_depth: custom_caps.max_bit_depth,
        supported_transports: custom_caps.supported_transports.clone(),
        supported_codecs: custom_caps.supported_codecs.clone(),
        supported_sample_rates: custom_caps.supported_sample_rates.clone(),
        supported_bit_depths: custom_caps.supported_bit_depths.clone(),
        supported_channels: custom_caps.supported_channels.clone(),
        maximum_safe_volume: Some(100),
        qualification: michi_receivers::ReceiverQualification::Qualified,
        ..Default::default()
    };

    // Encrypt and persist
    let cred_store1 = state1.receiver_credential_store.as_ref().as_ref().unwrap();
    let (ciphertext, nonce) = cred_store1.encrypt_token(&receiver_id, &raw_token).unwrap();
    let now_str = chrono::Utc::now().to_rfc3339();

    let prec = michi_db::PersistedReceiver {
        id: receiver_id.clone(),
        name: entry1.name.clone(),
        device_type: entry1.device_type.clone(),
        base_url: entry1.base_url.clone(),
        paired: true,
        online: true,
        audio_capabilities: serde_json::to_string(&custom_caps).unwrap(),
        last_seen: Some(now_str.clone()),
        paired_at: Some(now_str.clone()),
        created_at: now_str.clone(),
        updated_at: now_str.clone(),
        michi_id: Some(michi_id.clone()),
        capabilities_json: Some(serde_json::to_string(&custom_caps).unwrap()),
        capabilities_observed_at: Some(now_str.clone()),
        authority_supported: true,
    };

    let pcred = michi_db::PersistedReceiverCredential {
        receiver_id: receiver_id.clone(),
        ciphertext,
        nonce,
        version: 1,
        created_at: now_str.clone(),
        updated_at: now_str.clone(),
    };

    michi_db::persist_paired_receiver_transaction(&pool, &prec, &pcred)
        .await
        .unwrap();

    // Verify DB write
    let stored = michi_db::get_receiver_db(&pool, &receiver_id)
        .await
        .unwrap()
        .expect("receiver must exist in DB");
    assert_eq!(stored.id, receiver_id);

    // Shutdown instance 1
    state1
        .shutdown_and_wait(std::time::Duration::from_millis(500))
        .await;

    // ── AppState Instance 2: Fresh restart from same DB & credentials key ──
    let pool2 = michi_db::init_pool(&db_url).await.unwrap();
    let state2 = AppState::new_with_identity(config, pool2, None, identity);

    state2.bootstrap_runtime().await.unwrap();

    let reg_arc = state2.receiver_manager.registry().await;
    let reg2 = reg_arc.read().await;
    let entry2 = reg2.get(&receiver_id).expect("receiver must be restored");

    // Assert exact capability vectors restored
    assert_eq!(entry2.receiver_id, receiver_id);
    assert_eq!(entry2.michi_id.as_deref(), Some(michi_id.as_str()));
    assert!(entry2.paired, "receiver must be marked paired");
    assert_eq!(
        entry2.token.as_ref(),
        Some(&raw_token),
        "token must be decrypted"
    );

    // Truthful presence at boot: Offline until fresh Scent evidence arrives
    assert_eq!(
        entry2.presence,
        ReceiverPresence::Offline,
        "presence must start Offline until revalidated"
    );
    assert!(
        entry2.capabilities_stale,
        "capabilities must be marked stale on boot"
    );

    // Exact audio capabilities restored without fabrication
    assert_eq!(entry2.max_sample_rate, 48000);
    assert_eq!(entry2.max_bit_depth, 16);
    assert_eq!(entry2.supported_sample_rates, vec![48000]);
    assert!(
        !entry2.supported_sample_rates.contains(&44100),
        "must not invent 44.1 kHz support!"
    );
    assert_eq!(entry2.supported_bit_depths, vec![16]);
    assert_eq!(entry2.supported_channels, vec![2]);
    assert_eq!(entry2.supported_codecs, vec!["pcm_s16le".to_string()]);
    assert_eq!(entry2.supported_transports, vec!["rtp_udp".to_string()]);
    assert!(entry2.authority_supported);

    drop(reg2);

    // ── Simulate fresh server/info observation ──────────────────────────
    {
        let mut reg_write = reg_arc.write().await;
        let entry_mut = reg_write.get_mut(&receiver_id).unwrap();
        entry_mut.capabilities_stale = false;
        entry_mut.presence = ReceiverPresence::VerifiedOnline;
        entry_mut.capabilities_verified_at = Some(chrono::Utc::now());
    }

    let reg_after = reg_arc.read().await;
    let entry_after = reg_after.get(&receiver_id).unwrap();
    assert!(!entry_after.capabilities_stale);
    assert_eq!(entry_after.presence, ReceiverPresence::VerifiedOnline);

    state2
        .shutdown_and_wait(std::time::Duration::from_millis(500))
        .await;
}
