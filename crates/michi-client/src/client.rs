use std::time::Duration;

use chrono::{DateTime, Utc};
use michi_core::{LibraryStats, Track};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ClientError, V1ErrorBody};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerFeatures {
    pub library: bool,
    pub search: bool,
    pub streaming: bool,
    pub web_ui: bool,
    pub playlists: bool,
    pub artwork: bool,
    pub sync: bool,
    pub transcoding: bool,
    pub websocket: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub server_id: Uuid,
    pub version: String,
    pub api_version: String,
    pub features: ServerFeatures,
}

#[derive(Debug, Clone)]
pub struct ConnectionState {
    pub server_url: String,
    pub server_id: Uuid,
    pub server_name: String,
    pub api_version: String,
    pub features: ServerFeatures,
    pub token: Option<String>,
    pub last_seen: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct MichiClient {
    inner: reqwest::Client,
    pub state: Option<ConnectionState>,
    timeout: Duration,
}

impl MichiClient {
    pub fn new(timeout_secs: u64) -> Self {
        Self {
            inner: reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout_secs))
                .build()
                .unwrap(),
            state: None,
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(DEFAULT_TIMEOUT.as_secs())
    }

    fn auth_header(&self) -> Option<String> {
        self.state
            .as_ref()
            .and_then(|s| s.token.as_ref())
            .map(|t| format!("Bearer {t}"))
    }

    fn url(&self, path: &str) -> String {
        let base = self
            .state
            .as_ref()
            .map(|s| s.server_url.as_str())
            .unwrap_or("http://localhost:8096");
        format!("{base}{path}")
    }

    pub async fn connect(&mut self, server_url: &str) -> Result<&ConnectionState, ClientError> {
        let url = format!("{server_url}/api/v1/server/info");
        let resp = self
            .inner
            .get(&url)
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Http {
                status,
                message: body,
            });
        }

        let info: ServerInfo = resp
            .json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))?;

        if info.api_version != "v1" {
            return Err(ClientError::ApiVersionMismatch(info.api_version));
        }

        self.state = Some(ConnectionState {
            server_url: server_url.to_string(),
            server_id: info.server_id,
            server_name: info.name,
            api_version: info.api_version,
            features: info.features,
            token: None,
            last_seen: Some(Utc::now()),
        });

        Ok(self.state.as_ref().unwrap())
    }

    pub async fn login(&mut self, username: &str, password: &str) -> Result<(), ClientError> {
        let url = self.url("/api/auth/login");
        let body = serde_json::json!({ "username": username, "password": password });

        let resp = self
            .inner
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let v1: V1ErrorBody = resp
                .json()
                .await
                .map_err(|e| ClientError::InvalidResponse(e.to_string()))?;
            return Err(ClientError::V1 {
                code: v1.code().to_string(),
                message: v1.message().to_string(),
            });
        }

        let login_resp: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))?;

        let token = login_resp["token"]
            .as_str()
            .ok_or(ClientError::InvalidResponse("no token".into()))?
            .to_string();

        if let Some(ref mut state) = self.state {
            state.token = Some(token);
            state.last_seen = Some(Utc::now());
        }

        Ok(())
    }

    fn v1_error(&self, status: u16, text: &str) -> ClientError {
        serde_json::from_str::<V1ErrorBody>(text)
            .map(|v| ClientError::V1 {
                code: v.code().to_string(),
                message: v.message().to_string(),
            })
            .unwrap_or(ClientError::Http {
                status,
                message: text.to_string(),
            })
    }

    pub async fn get_tracks(&self) -> Result<Vec<Track>, ClientError> {
        let url = self.url("/api/v1/tracks");
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth_header().unwrap_or_default())
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let text = resp.text().await.unwrap_or_default();
            return Err(self.v1_error(status, &text));
        }

        resp.json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }

    pub async fn search_tracks(&self, query: &str) -> Result<Vec<Track>, ClientError> {
        let url = self.url(&format!("/api/v1/search?q={}", url_encode(query)));
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth_header().unwrap_or_default())
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let text = resp.text().await.unwrap_or_default();
            return Err(self.v1_error(status, &text));
        }

        resp.json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }

    pub async fn get_track(&self, track_id: Uuid) -> Result<Track, ClientError> {
        let url = self.url(&format!("/api/v1/tracks/{track_id}"));
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth_header().unwrap_or_default())
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let text = resp.text().await.unwrap_or_default();
            return Err(self.v1_error(status, &text));
        }

        resp.json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }

    pub async fn get_library_stats(&self) -> Result<LibraryStats, ClientError> {
        let url = self.url("/api/v1/library/stats");
        let resp = self
            .inner
            .get(&url)
            .header("Authorization", self.auth_header().unwrap_or_default())
            .send()
            .await
            .map_err(|e| ClientError::Connection(e.to_string()))?;

        let status = resp.status().as_u16();
        if status != 200 {
            let text = resp.text().await.unwrap_or_default();
            return Err(self.v1_error(status, &text));
        }

        resp.json()
            .await
            .map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }

    pub fn stream_url(&self, track_id: Uuid) -> String {
        self.url(&format!("/api/v1/stream/{track_id}"))
    }

    pub fn check_features(&self) -> Option<&ServerFeatures> {
        self.state.as_ref().map(|s| &s.features)
    }

    pub fn has_feature(&self, name: &str) -> bool {
        match name {
            "library" => self.state.as_ref().is_some_and(|s| s.features.library),
            "search" => self.state.as_ref().is_some_and(|s| s.features.search),
            "streaming" => self.state.as_ref().is_some_and(|s| s.features.streaming),
            "web_ui" => self.state.as_ref().is_some_and(|s| s.features.web_ui),
            "playlists" => self.state.as_ref().is_some_and(|s| s.features.playlists),
            "artwork" => self.state.as_ref().is_some_and(|s| s.features.artwork),
            "sync" => self.state.as_ref().is_some_and(|s| s.features.sync),
            "transcoding" => self
                .state
                .as_ref()
                .is_some_and(|s| s.features.transcoding),
            "websocket" => self.state.as_ref().is_some_and(|s| s.features.websocket),
            _ => false,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.state.as_ref().and_then(|s| s.token.as_ref()).is_some()
    }

    pub fn server_id(&self) -> Option<Uuid> {
        self.state.as_ref().map(|s| s.server_id)
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout.as_secs()
    }
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode() {
        assert_eq!(url_encode("hello"), "hello");
        assert_eq!(url_encode("pink floyd"), "pink%20floyd");
        assert_eq!(url_encode("qué tal"), "qu%C3%A9%20tal");
    }

    #[test]
    fn test_client_new() {
        let c = MichiClient::with_defaults();
        assert!(c.state.is_none());
        assert!(!c.is_authenticated());
    }

    #[test]
    fn test_has_feature_no_state() {
        let c = MichiClient::with_defaults();
        assert!(!c.has_feature("library"));
        assert!(!c.has_feature("search"));
    }

    #[test]
    fn test_stream_url_no_state() {
        let c = MichiClient::with_defaults();
        let id = uuid::Uuid::new_v4();
        let url = c.stream_url(id);
        assert!(url.contains("/api/v1/stream/"));
    }

    #[test]
    fn test_server_id_no_state() {
        let c = MichiClient::with_defaults();
        assert!(c.server_id().is_none());
    }

    #[test]
    fn test_timeout() {
        let c = MichiClient::new(30);
        assert_eq!(c.timeout_secs(), 30);
        assert!(c.check_features().is_none());
    }

    #[test]
    fn test_has_feature_names() {
        let c = MichiClient::with_defaults();
        assert!(!c.has_feature("library"));
        assert!(!c.has_feature("nonexistent"));
    }
}
