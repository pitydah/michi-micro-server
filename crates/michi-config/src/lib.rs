use std::{env, path::Path, path::PathBuf};

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
    pub scrobble_enabled: bool,
    pub auth_username: Option<String>,
    pub auth_password: Option<String>,
    pub auth_enabled: bool,
    pub allow_registration: bool,
}

impl Config {
    pub fn from_env() -> Self {
        let port = env::var("MICHI_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8096);

        let music_paths = env::var("MICHI_MUSIC_PATH")
            .unwrap_or_else(|_| "/music".to_string())
            .split(',')
            .map(|s| PathBuf::from(s.trim()))
            .filter(|p| !p.as_os_str().is_empty())
            .collect();

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
            scrobble_enabled,
            auth_username,
            auth_password,
            auth_enabled,
            allow_registration,
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn version(&self) -> &str {
        self.version
    }

    /// Convenience method returning the first music path.
    /// Panics if no music paths are configured.
    pub fn music_path(&self) -> &Path {
        &self.music_paths[0]
    }
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
            assert_eq!(config.music_path(), Path::new("/music"));
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
