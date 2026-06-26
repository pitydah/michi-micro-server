use michi_client::MichiClient;
use michi_core::{AlbumSummary, ArtistSummary, LibraryStats, Track};
use serde_json::Value;

#[derive(Clone)]
pub struct ApiClient {
    inner: MichiClient,
}

impl ApiClient {
    pub fn new(_base: String, _token: Option<String>) -> Self {
        Self {
            inner: MichiClient::new(10),
        }
    }

    pub async fn connect(&mut self, server_url: &str) -> Result<(), String> {
        self.inner
            .connect(server_url)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub async fn fetch_status(&self) -> Result<Value, String> {
        self.inner.get_status().await.map_err(|e| e.to_string())
    }

    pub async fn fetch_tracks(&self) -> Result<Vec<Track>, String> {
        self.inner.get_tracks().await.map_err(|e| e.to_string())
    }

    pub async fn fetch_albums(&self) -> Result<Vec<AlbumSummary>, String> {
        self.inner.get_albums().await.map_err(|e| e.to_string())
    }

    pub async fn fetch_artists(&self) -> Result<Vec<ArtistSummary>, String> {
        self.inner.get_artists().await.map_err(|e| e.to_string())
    }

    pub async fn search(&self, query: &str) -> Result<Vec<Track>, String> {
        self.inner
            .search_tracks(query)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn fetch_track(&self, id: &str) -> Result<Track, String> {
        let uid = uuid::Uuid::parse_str(id).map_err(|e| format!("invalid UUID: {e}"))?;
        self.inner.get_track(uid).await.map_err(|e| e.to_string())
    }

    pub async fn fetch_stats(&self) -> Result<LibraryStats, String> {
        self.inner
            .get_library_stats()
            .await
            .map_err(|e| e.to_string())
    }

    pub fn stream_url(&self, id: &str) -> String {
        if let Ok(uid) = uuid::Uuid::parse_str(id) {
            self.inner.stream_url(uid)
        } else {
            format!("/api/stream/{id}")
        }
    }

    pub async fn fetch_album_tracks(&self, album: &str) -> Result<Vec<Track>, String> {
        self.inner
            .get_album_tracks(album)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn fetch_artist_tracks(&self, artist: &str) -> Result<Vec<Track>, String> {
        self.inner
            .get_artist_tracks(artist)
            .await
            .map_err(|e| e.to_string())
    }
}
