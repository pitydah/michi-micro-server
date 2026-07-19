use std::io::Write;
use std::{env, path::Path, path::PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub sidebar_collapsed: bool,
    pub cover_art_enabled: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".into(),
            sidebar_collapsed: false,
            cover_art_enabled: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub music_paths: Vec<PathBuf>,
    pub config_path: PathBuf,
    pub cache_path: PathBuf,
    pub database_url: String,
    pub version: &'static str,
    pub sync_peers: Vec<String>,
    pub sync_name: String,
    pub listenbrainz_token: Option<String>,
    pub lastfm_token: Option<String>,
    pub scrobble_enabled: bool,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
    pub auth_enabled: bool,
    pub allow_registration: bool,
    pub server_id: Uuid,
    pub cors_origin: Option<String>,
    pub dev_mode: bool,
    pub resource_profile: michi_core::ResourceProfile,
    pub stream_profile: michi_core::StreamProfile,
    pub format_policy: michi_core::AudioFormatPolicy,
    pub max_remote_bitrate: u32,
    pub remote_sync: bool,
    pub language: String,
    pub ui: UiConfig,
    pub auto_backup_enabled: bool,
    pub backup_max_keep: u32,
    pub job_max_concurrent: u32,
    pub reconnect_delay_max: u32,
}

#[allow(dead_code)]
fn default_port() -> u16 {
    8096
}
#[allow(dead_code)]
fn default_music_paths() -> Vec<PathBuf> {
    vec![PathBuf::from("/music")]
}
#[allow(dead_code)]
fn default_lang() -> String {
    "en".into()
}
#[allow(dead_code)]
fn default_backup_keep() -> u32 {
    7
}
#[allow(dead_code)]
fn default_max_jobs() -> u32 {
    3
}
#[allow(dead_code)]
fn default_reconnect() -> u32 {
    300
}

impl Config {
    pub fn from_env() -> Self {
        let port = env::var("MICHI_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8096);

        let music_paths = {
            let paths: Vec<PathBuf> = env::var("MICHI_MUSIC_PATH")
                .unwrap_or_else(|_| "/music".to_string())
                .split(',')
                .map(|s| PathBuf::from(s.trim()))
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
            if paths.is_empty() {
                vec![PathBuf::from("/music")]
            } else {
                paths
            }
        };

        let config_path = env::var("MICHI_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/config"));

        let cache_path = env::var("MICHI_CACHE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/cache"));

        let database_url =
            env::var("MICHI_DATABASE").unwrap_or_else(|_| "sqlite:///config/michi.db".to_string());

        let sync_peers = env::var("MICHI_SYNC_PEERS")
            .unwrap_or_default()
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let sync_name = env::var("MICHI_SYNC_NAME").unwrap_or_else(|_| "default".to_string());

        let listenbrainz_token = env::var("MICHI_LISTENBRAINZ_TOKEN").ok();
        let lastfm_token = env::var("MICHI_LASTFM_TOKEN").ok();
        let scrobble_enabled = env::var("MICHI_SCROBBLE_ENABLED")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let auth_username = env::var("MICHI_AUTH_USERNAME").ok();
        let auth_password = env::var("MICHI_AUTH_PASSWORD").ok();
        let auth_enabled = auth_username.is_some() && auth_password.is_some();
        let allow_registration = env::var("MICHI_ALLOW_REGISTRATION")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let server_id = load_or_create_server_id(&config_path);

        let cors_origin = env::var("MICHI_CORS_ORIGIN").ok();
        let dev_mode = env::var("MICHI_DEV_MODE")
            .ok()
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let mut config = Self {
            port,
            music_paths,
            config_path,
            cache_path,
            database_url,
            version: env!("CARGO_PKG_VERSION"),
            sync_peers,
            sync_name,
            listenbrainz_token,
            lastfm_token,
            scrobble_enabled,
            auth_username,
            auth_password,
            auth_enabled,
            allow_registration,
            server_id,
            cors_origin,
            dev_mode,
            resource_profile: michi_core::ResourceProfile::from_config_str("balanced"),
            stream_profile: michi_core::StreamProfile::from_config_str("original"),
            format_policy: michi_core::AudioFormatPolicy::from_config_str("lossless"),
            max_remote_bitrate: 320_000,
            remote_sync: false,
            language: "en".into(),
            ui: UiConfig {
                theme: "dark".into(),
                sidebar_collapsed: false,
                cover_art_enabled: true,
            },
            auto_backup_enabled: false,
            backup_max_keep: 7,
            job_max_concurrent: 3,
            reconnect_delay_max: 300,
        };

        // Load from config.json if present (env vars override)
        config.load_file_overrides();
        config.apply_env_overrides();
        config
    }

    fn load_file_overrides(&mut self) {
        let path = self.config_path.join("config.json");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(file_cfg) = serde_json::from_str::<Config>(&content) {
                self.port = file_cfg.port;
                if !file_cfg.music_paths.is_empty() {
                    self.music_paths = file_cfg.music_paths;
                }
                if !file_cfg.sync_peers.is_empty() {
                    self.sync_peers = file_cfg.sync_peers;
                }
                if !file_cfg.sync_name.is_empty() {
                    self.sync_name = file_cfg.sync_name;
                }
                self.scrobble_enabled = file_cfg.scrobble_enabled;
                self.allow_registration = file_cfg.allow_registration;
                self.dev_mode = file_cfg.dev_mode;
                self.resource_profile = file_cfg.resource_profile;
                self.stream_profile = file_cfg.stream_profile;
                self.format_policy = file_cfg.format_policy;
                self.max_remote_bitrate = file_cfg.max_remote_bitrate;
                self.remote_sync = file_cfg.remote_sync;
                self.language = file_cfg.language;
                self.ui = file_cfg.ui;
                self.auto_backup_enabled = file_cfg.auto_backup_enabled;
                self.backup_max_keep = file_cfg.backup_max_keep;
                self.job_max_concurrent = file_cfg.job_max_concurrent;
                self.reconnect_delay_max = file_cfg.reconnect_delay_max;
            }
        }
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = env::var("MICHI_PORT") {
            if let Ok(p) = v.parse::<u16>() {
                self.port = p;
            }
        }
        if let Ok(v) = env::var("MICHI_MUSIC_PATH") {
            let paths: Vec<PathBuf> = v
                .split(',')
                .map(|s| PathBuf::from(s.trim()))
                .filter(|p| !p.as_os_str().is_empty())
                .collect();
            if !paths.is_empty() {
                self.music_paths = paths;
            }
        }
        if let Ok(v) = env::var("MICHI_RESOURCE_PROFILE") {
            self.resource_profile = michi_core::ResourceProfile::from_config_str(&v);
        }
        if let Ok(v) = env::var("MICHI_STREAM_PROFILE") {
            self.stream_profile = michi_core::StreamProfile::from_config_str(&v);
        }
        if let Ok(v) = env::var("MICHI_FORMAT_POLICY") {
            self.format_policy = michi_core::AudioFormatPolicy::from_config_str(&v);
        }
        if let Ok(v) = env::var("MICHI_SYNC_PEERS") {
            self.sync_peers = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        if let Ok(v) = env::var("MICHI_SYNC_NAME") {
            self.sync_name = v;
        }
        if let Ok(v) = env::var("MICHI_SCROBBLE_ENABLED") {
            self.scrobble_enabled = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(v) = env::var("MICHI_ALLOW_REGISTRATION") {
            self.allow_registration = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(v) = env::var("MICHI_CORS_ORIGIN") {
            self.cors_origin = Some(v);
        }
        if let Ok(v) = env::var("MICHI_DEV_MODE") {
            self.dev_mode = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(v) = env::var("MICHI_MAX_REMOTE_BITRATE") {
            if let Ok(p) = v.parse() {
                self.max_remote_bitrate = p;
            }
        }
        if let Ok(v) = env::var("MICHI_REMOTE_SYNC") {
            self.remote_sync = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(v) = env::var("MICHI_LANG") {
            self.language = v;
        }
        if let Ok(v) = env::var("MICHI_AUTO_BACKUP") {
            self.auto_backup_enabled = v == "1" || v.to_lowercase() == "true";
        }
        if let Ok(v) = env::var("MICHI_BACKUP_KEEP") {
            if let Ok(p) = v.parse() {
                self.backup_max_keep = p;
            }
        }
        if let Ok(v) = env::var("MICHI_MAX_JOBS") {
            if let Ok(p) = v.parse() {
                self.job_max_concurrent = p;
            }
        }
        if let Ok(v) = env::var("MICHI_RECONNECT_MAX") {
            if let Ok(p) = v.parse() {
                self.reconnect_delay_max = p;
            }
        }
    }

    pub fn save_to_file(&self) -> Result<(), String> {
        let path = self.config_path.join("config.json");
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, &json).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn version(&self) -> &str {
        self.version
    }

    pub fn primary_music_path(&self) -> Option<&Path> {
        self.music_paths.first().map(|p| p.as_path())
    }

    #[deprecated(since = "0.1.0", note = "use primary_music_path() instead")]
    pub fn music_path(&self) -> &Path {
        &self.music_paths[0]
    }

    pub fn human_resource_profile(&self) -> String {
        match self.resource_profile {
            michi_core::ResourceProfile::Eco => "Eco — minimal CPU/memory usage".into(),
            michi_core::ResourceProfile::Balanced => {
                "Balanced — good quality, moderate CPU usage".into()
            }
            michi_core::ResourceProfile::Performance => {
                "Performance — maximum quality, highest resources".into()
            }
            michi_core::ResourceProfile::Custom => "Custom — user-defined resource limits".into(),
        }
    }

    pub fn human_stream_profile(&self) -> String {
        match self.stream_profile.to_string().as_str() {
            "original" => "Original — no transcoding".into(),
            "high" => "High Quality — transcoded to high bitrate".into(),
            "medium" => "Medium — balanced quality".into(),
            "low" => "Efficient — low bitrate for remote".into(),
            _ => self.stream_profile.to_string(),
        }
    }

    pub fn human_format_policy(&self) -> String {
        match self.format_policy.to_string().as_str() {
            "lossless" => "Lossless — serve original files without conversion".into(),
            "transcoded" => "Transcoded — convert to compatible format on the fly".into(),
            "passthrough" => "Passthrough — let the client decide".into(),
            _ => self.format_policy.to_string(),
        }
    }
}

pub fn load_or_create_server_id(config_path: &Path) -> Uuid {
    let file_path = config_path.join("server_id");
    if let Ok(existing) = std::fs::read_to_string(&file_path) {
        if let Ok(id) = Uuid::parse_str(existing.trim()) {
            return id;
        }
    }
    let id = Uuid::new_v4();
    if let Some(parent) = file_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::File::create(&file_path) {
        let _ = f.write_all(id.to_string().as_bytes());
    }
    id
}

// ── Custom serde for Config: serialize profile enums as strings ──

use serde::ser::SerializeStruct;

impl Serialize for Config {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("Config", 20)?;
        s.serialize_field("port", &self.port)?;
        s.serialize_field("music_paths", &self.music_paths)?;
        s.serialize_field("sync_peers", &self.sync_peers)?;
        s.serialize_field("sync_name", &self.sync_name)?;
        s.serialize_field("scrobble_enabled", &self.scrobble_enabled)?;
        s.serialize_field("allow_registration", &self.allow_registration)?;
        s.serialize_field("dev_mode", &self.dev_mode)?;
        s.serialize_field("resource_profile", &self.resource_profile.to_string())?;
        s.serialize_field("stream_profile", &self.stream_profile.to_string())?;
        s.serialize_field("format_policy", &self.format_policy.to_string())?;
        s.serialize_field("max_remote_bitrate", &self.max_remote_bitrate)?;
        s.serialize_field("remote_sync", &self.remote_sync)?;
        s.serialize_field("language", &self.language)?;
        s.serialize_field("ui", &self.ui)?;
        s.serialize_field("auto_backup_enabled", &self.auto_backup_enabled)?;
        s.serialize_field("backup_max_keep", &self.backup_max_keep)?;
        s.serialize_field("job_max_concurrent", &self.job_max_concurrent)?;
        s.serialize_field("reconnect_delay_max", &self.reconnect_delay_max)?;
        s.serialize_field("cors_origin", &self.cors_origin)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ConfigHelper {
            port: Option<u16>,
            music_paths: Option<Vec<PathBuf>>,
            sync_peers: Option<Vec<String>>,
            sync_name: Option<String>,
            scrobble_enabled: Option<bool>,
            allow_registration: Option<bool>,
            dev_mode: Option<bool>,
            resource_profile: Option<String>,
            stream_profile: Option<String>,
            format_policy: Option<String>,
            max_remote_bitrate: Option<u32>,
            remote_sync: Option<bool>,
            language: Option<String>,
            ui: Option<UiConfig>,
            auto_backup_enabled: Option<bool>,
            backup_max_keep: Option<u32>,
            job_max_concurrent: Option<u32>,
            reconnect_delay_max: Option<u32>,
            cors_origin: Option<String>,
        }

        let h = ConfigHelper::deserialize(deserializer)?;
        // Build directly from defaults to avoid recursion:
        // from_env() -> load_file_overrides() -> Deserialize -> from_env() -> ...
        let mut cfg = Config {
            port: 8096,
            music_paths: vec![PathBuf::from("/music")],
            config_path: PathBuf::from("/config"),
            cache_path: PathBuf::from("/cache"),
            database_url: "sqlite:///config/michi.db".to_string(),
            version: env!("CARGO_PKG_VERSION"),
            sync_peers: Vec::new(),
            sync_name: "default".to_string(),
            listenbrainz_token: None,
            lastfm_token: None,
            scrobble_enabled: false,
            auth_username: None,
            auth_password: None,
            auth_enabled: false,
            allow_registration: false,
            server_id: uuid::Uuid::new_v4(),
            cors_origin: None,
            dev_mode: false,
            resource_profile: michi_core::ResourceProfile::from_config_str("balanced"),
            stream_profile: michi_core::StreamProfile::from_config_str("original"),
            format_policy: michi_core::AudioFormatPolicy::from_config_str("lossless"),
            max_remote_bitrate: 320_000,
            remote_sync: false,
            language: "en".into(),
            ui: UiConfig::default(),
            auto_backup_enabled: false,
            backup_max_keep: 7,
            job_max_concurrent: 3,
            reconnect_delay_max: 300,
        };
        if let Some(v) = h.port {
            cfg.port = v;
        }
        if let Some(v) = h.music_paths {
            if !v.is_empty() {
                cfg.music_paths = v;
            }
        }
        if let Some(v) = h.sync_peers {
            cfg.sync_peers = v;
        }
        if let Some(v) = h.sync_name {
            cfg.sync_name = v;
        }
        if let Some(v) = h.scrobble_enabled {
            cfg.scrobble_enabled = v;
        }
        if let Some(v) = h.allow_registration {
            cfg.allow_registration = v;
        }
        if let Some(v) = h.dev_mode {
            cfg.dev_mode = v;
        }
        if let Some(ref v) = h.resource_profile {
            cfg.resource_profile = michi_core::ResourceProfile::from_config_str(v);
        }
        if let Some(ref v) = h.stream_profile {
            cfg.stream_profile = michi_core::StreamProfile::from_config_str(v);
        }
        if let Some(ref v) = h.format_policy {
            cfg.format_policy = michi_core::AudioFormatPolicy::from_config_str(v);
        }
        if let Some(v) = h.max_remote_bitrate {
            cfg.max_remote_bitrate = v;
        }
        if let Some(v) = h.remote_sync {
            cfg.remote_sync = v;
        }
        if let Some(v) = h.language {
            cfg.language = v;
        }
        if let Some(v) = h.ui {
            cfg.ui = v;
        }
        if let Some(v) = h.auto_backup_enabled {
            cfg.auto_backup_enabled = v;
        }
        if let Some(v) = h.backup_max_keep {
            cfg.backup_max_keep = v;
        }
        if let Some(v) = h.job_max_concurrent {
            cfg.job_max_concurrent = v;
        }
        if let Some(v) = h.reconnect_delay_max {
            cfg.reconnect_delay_max = v;
        }
        if let Some(v) = h.cors_origin {
            cfg.cors_origin = Some(v);
        }
        Ok(cfg)
    }
}
