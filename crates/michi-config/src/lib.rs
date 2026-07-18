use std::io::Write;
use std::{env, path::Path, path::PathBuf};

use uuid::Uuid;

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

        let resource_profile = michi_core::ResourceProfile::from_config_str(
            &env::var("MICHI_RESOURCE_PROFILE").unwrap_or_else(|_| "balanced".into()),
        );

        let stream_profile = michi_core::StreamProfile::from_config_str(
            &env::var("MICHI_STREAM_PROFILE").unwrap_or_else(|_| "original".into()),
        );

        let format_policy = michi_core::AudioFormatPolicy::from_config_str(
            &env::var("MICHI_FORMAT_POLICY").unwrap_or_else(|_| "lossless".into()),
        );

        Self {
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
            resource_profile,
            stream_profile,
            format_policy,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn version(&self) -> &str {
        self.version
    }

    /// Returns the first music path, if configured.
    /// This is the safe alternative to `music_path()`.
    pub fn primary_music_path(&self) -> Option<&Path> {
        self.music_paths.first().map(|p| p.as_path())
    }

    /// Convenience method returning the first music path.
    /// Deprecated: use `primary_music_path()` for new code.
    /// Guaranteed to return at least `/music` if none configured.
    #[deprecated(since = "0.1.0", note = "use primary_music_path() instead")]
    pub fn music_path(&self) -> &Path {
        &self.music_paths[0]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_url_default_no_mode_param() {
        temp_env::with_var("MICHI_DATABASE", None::<&str>, || {
            let config = Config::from_env();
            assert_eq!(config.database_url, "sqlite:///config/michi.db");
            assert!(
                !config.database_url.contains("?mode=rwc"),
                "default URL should not need ?mode=rwc"
            );
        });
    }

    #[test]
    fn test_port_default() {
        temp_env::with_var("MICHI_PORT", None::<&str>, || {
            let config = Config::from_env();
            assert_eq!(config.port, 8096);
        });
    }

    #[test]
    fn test_music_path_default() {
        temp_env::with_var("MICHI_MUSIC_PATH", None::<&str>, || {
            let config = Config::from_env();
            assert_eq!(config.music_paths, vec![PathBuf::from("/music")]);
            assert_eq!(config.primary_music_path(), Some(Path::new("/music")));
        });
    }

    #[test]
    fn test_music_paths_multiple() {
        temp_env::with_var(
            "MICHI_MUSIC_PATH",
            Some("/music,/mnt/music,/data/music"),
            || {
                let config = Config::from_env();
                assert_eq!(
                    config.music_paths,
                    vec![
                        PathBuf::from("/music"),
                        PathBuf::from("/mnt/music"),
                        PathBuf::from("/data/music"),
                    ]
                );
            },
        );
    }

    #[test]
    fn test_music_paths_single_with_trailing_comma() {
        temp_env::with_var("MICHI_MUSIC_PATH", Some("/music,"), || {
            let config = Config::from_env();
            assert_eq!(config.music_paths, vec![PathBuf::from("/music")]);
        });
    }

    #[test]
    fn test_primary_music_path_default() {
        temp_env::with_var("MICHI_MUSIC_PATH", None::<&str>, || {
            let config = Config::from_env();
            assert_eq!(config.primary_music_path(), Some(Path::new("/music")));
        });
    }

    #[test]
    fn test_primary_music_path_multiple() {
        temp_env::with_var("MICHI_MUSIC_PATH", Some("/a,/b,/c"), || {
            let config = Config::from_env();
            assert_eq!(config.primary_music_path(), Some(Path::new("/a")));
            assert_eq!(config.music_paths.len(), 3);
        });
    }

    #[test]
    fn test_primary_music_path_empty_env_uses_default() {
        temp_env::with_var("MICHI_MUSIC_PATH", Some(""), || {
            let config = Config::from_env();
            assert_eq!(config.primary_music_path(), Some(Path::new("/music")));
            assert_eq!(config.music_paths.len(), 1);
        });
    }

    #[test]
    fn test_auth_disabled_when_not_set() {
        temp_env::with_vars(
            vec![
                ("MICHI_AUTH_USERNAME", None::<&str>),
                ("MICHI_AUTH_PASSWORD", None::<&str>),
            ],
            || {
                let config = Config::from_env();
                assert!(!config.auth_enabled);
                assert!(config.auth_username.is_none());
                assert!(config.auth_password.is_none());
            },
        );
    }

    #[test]
    fn test_auth_enabled_when_both_set() {
        temp_env::with_vars(
            vec![
                ("MICHI_AUTH_USERNAME", Some("admin")),
                ("MICHI_AUTH_PASSWORD", Some("secret")),
            ],
            || {
                let config = Config::from_env();
                assert!(config.auth_enabled);
                assert_eq!(config.auth_username.as_deref(), Some("admin"));
                assert_eq!(config.auth_password.as_deref(), Some("secret"));
            },
        );
    }

    #[test]
    fn test_env_overrides() {
        temp_env::with_var("MICHI_PORT", Some("9999"), || {
            temp_env::with_var("MICHI_DATABASE", Some("sqlite://./local.db"), || {
                let config = Config::from_env();
                assert_eq!(config.port, 9999);
                assert_eq!(config.database_url, "sqlite://./local.db");
            });
        });
    }
}
