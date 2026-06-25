use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub music_path: PathBuf,
    pub config_path: PathBuf,
    pub cache_path: PathBuf,
    pub database_url: String,
    pub version: &'static str,
}

impl Config {
    pub fn from_env() -> Self {
        let port = env::var("MICHI_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(8096);

        let music_path = env::var("MICHI_MUSIC_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/music"));

        let config_path = env::var("MICHI_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/config"));

        let cache_path = env::var("MICHI_CACHE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/cache"));

        let database_url =
            env::var("MICHI_DATABASE").unwrap_or_else(|_| "sqlite:///config/michi.db".to_string());

        Self {
            port,
            music_path,
            config_path,
            cache_path,
            database_url,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn version(&self) -> &str {
        self.version
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
            assert_eq!(config.music_path, PathBuf::from("/music"));
        });
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
