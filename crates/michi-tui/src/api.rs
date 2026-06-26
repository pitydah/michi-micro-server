use michi_core::{AlbumSummary, ArtistSummary, LibraryStats, Track};
use serde_json::Value;

#[derive(Clone)]
pub struct ApiClient {
    base: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(base: String, token: Option<String>) -> Self {
        Self {
            base,
            token,
            client: reqwest::Client::new(),
        }
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        if let Some(t) = &self.token {
            if let Ok(v) = reqwest::header::HeaderValue::from_str(&format!("Bearer {t}")) {
                h.insert(reqwest::header::AUTHORIZATION, v);
            }
        }
        h
    }

    pub async fn fetch_status(&self) -> Result<Value, String> {
        let url = format!("{}/api/status", self.base);
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Value>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn fetch_tracks(&self) -> Result<Vec<Track>, String> {
        let url = format!("{}/api/tracks", self.base);
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Vec<Track>>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn fetch_albums(&self) -> Result<Vec<AlbumSummary>, String> {
        let url = format!("{}/api/albums", self.base);
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Vec<AlbumSummary>>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn fetch_artists(&self) -> Result<Vec<ArtistSummary>, String> {
        let url = format!("{}/api/artists", self.base);
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Vec<ArtistSummary>>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Track>, String> {
        let url = format!("{}/api/search?q={}", self.base, urlencoding(query));
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Vec<Track>>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn fetch_track(&self, id: &str) -> Result<Track, String> {
        let url = format!("{}/api/tracks/{id}", self.base);
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Track>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn fetch_stats(&self) -> Result<LibraryStats, String> {
        let url = format!("{}/api/library/stats", self.base);
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<LibraryStats>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub fn stream_url(&self, id: &str) -> String {
        format!("{}/api/stream/{id}", self.base)
    }

    pub async fn fetch_album_tracks(&self, album: &str) -> Result<Vec<Track>, String> {
        let url = format!("{}/api/albums/{}", self.base, urlencoding(album));
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Vec<Track>>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }

    pub async fn fetch_artist_tracks(&self, artist: &str) -> Result<Vec<Track>, String> {
        let url = format!("{}/api/artists/{}", self.base, urlencoding(artist));
        let res = self
            .client
            .get(&url)
            .headers(self.headers())
            .send()
            .await
            .map_err(|e| format!("connection failed: {e}"))?;
        res.json::<Vec<Track>>()
            .await
            .map_err(|e| format!("parse failed: {e}"))
    }
}

fn urlencoding(s: &str) -> String {
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
