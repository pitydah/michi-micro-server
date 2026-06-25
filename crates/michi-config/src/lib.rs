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

        let database_url = env::var("MICHI_DATABASE")
            .unwrap_or_else(|_| "sqlite:///config/michi.db?mode=rwc".to_string());

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
