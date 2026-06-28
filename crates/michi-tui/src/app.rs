use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use michi_core::{AlbumSummary, ArtistSummary, Track};
use ratatui::Terminal;

use crate::api::ApiClient;
use crate::ui;

pub enum Screen {
    Tracks,
    Albums,
    Artists,
    AlbumTracks(String),
    ArtistTracks(String),
    Search,
    Quit,
}

pub struct App {
    pub api: ApiClient,
    pub screen: Screen,
    pub tracks: Vec<Track>,
    pub albums: Vec<AlbumSummary>,
    pub artists: Vec<ArtistSummary>,
    pub current_tracks: Vec<Track>,
    pub search_query: String,
    pub selected: usize,
    pub offset: usize,
    pub status: String,
    pub error: Option<String>,
}

impl App {
    pub fn new(base_url: String, token: Option<String>) -> Self {
        Self {
            api: ApiClient::new(base_url, token),
            screen: Screen::Tracks,
            tracks: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            current_tracks: Vec::new(),
            search_query: String::new(),
            selected: 0,
            offset: 0,
            status: "connecting...".to_string(),
            error: None,
        }
    }

    pub fn run(
        &mut self,
        terminal: &mut Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> anyhow::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        let result = Ok(());
        'main: loop {
            if matches!(self.screen, Screen::Quit) {
                break;
            }
            terminal.draw(|f| ui::draw(f, self))?;
            if !event::poll(Duration::from_millis(200))? {
                continue;
            }
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                #[allow(deprecated)]
                if key.modifiers == crossterm::event::KeyModifiers::CONTROL
                    && key.code == KeyCode::Char('c')
                {
                    break;
                }
                match self.handle_key(key.code) {
                    Ok(()) => {}
                    Err(()) => break 'main,
                }
            }
        }
        crossterm::terminal::disable_raw_mode()?;
        result
    }

    fn handle_key(&mut self, code: KeyCode) -> Result<(), ()> {
        match self.screen {
            Screen::Search => match code {
                KeyCode::Esc => {
                    self.screen = Screen::Tracks;
                    self.search_query.clear();
                }
                KeyCode::Enter => {
                    if self.search_query.is_empty() {
                        self.screen = Screen::Tracks;
                    } else {
                        let q = self.search_query.clone();
                        let q_display = q.clone();
                        self.status = "searching...".to_string();
                        let api = self.api.clone();
                        let fut = async move { api.search(&q).await };
                        let result = tokio::runtime::Handle::current().block_on(fut);
                        match result {
                            Ok(tracks) => {
                                self.current_tracks = tracks;
                                self.selected = 0;
                                self.offset = 0;
                                self.status = format!(
                                    "{} results for '{q_display}'",
                                    self.current_tracks.len()
                                );
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
                KeyCode::Char(c) => self.search_query.push(c),
                KeyCode::Backspace => {
                    self.search_query.pop();
                }
                _ => {}
            },
            _ => match code {
                KeyCode::Char('q') => {
                    self.screen = Screen::Quit;
                }
                KeyCode::Char('1') => self.switch_screen(Screen::Tracks),
                KeyCode::Char('2') => {
                    self.status = "loading albums...".to_string();
                    let api = self.api.clone();
                    let result = tokio::runtime::Handle::current().block_on(api.fetch_albums());
                    match result {
                        Ok(a) => {
                            self.albums = a;
                            self.selected = 0;
                            self.offset = 0;
                            self.screen = Screen::Albums;
                            self.status = format!("{} albums", self.albums.len());
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                KeyCode::Char('3') => {
                    self.status = "loading artists...".to_string();
                    let api = self.api.clone();
                    let result = tokio::runtime::Handle::current().block_on(api.fetch_artists());
                    match result {
                        Ok(a) => {
                            self.artists = a;
                            self.selected = 0;
                            self.offset = 0;
                            self.screen = Screen::Artists;
                            self.status = format!("{} artists", self.artists.len());
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                KeyCode::Char('/') => {
                    self.search_query.clear();
                    self.screen = Screen::Search;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    let len = self.items_len();
                    if len > 0 {
                        self.selected = (self.selected + 1).min(len - 1);
                        self.ensure_visible();
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if self.selected > 0 {
                        self.selected -= 1;
                        self.ensure_visible();
                    }
                }
                KeyCode::Enter => match &self.screen {
                    Screen::Albums => {
                        let idx = self.selected;
                        if idx < self.albums.len() {
                            let album = self.albums[idx].album.clone();
                            self.status = format!("loading album '{album}'...");
                            let api = self.api.clone();
                            let a = album.clone();
                            let result = tokio::runtime::Handle::current()
                                .block_on(api.fetch_album_tracks(&a));
                            match result {
                                Ok(t) => {
                                    self.current_tracks = t;
                                    self.selected = 0;
                                    self.offset = 0;
                                    self.screen = Screen::AlbumTracks(album);
                                    self.status = format!("{} tracks", self.current_tracks.len());
                                }
                                Err(e) => self.error = Some(e),
                            }
                        }
                    }
                    Screen::Artists => {
                        let idx = self.selected;
                        if idx < self.artists.len() {
                            let artist = self.artists[idx].artist.clone().unwrap_or_default();
                            self.status = format!("loading artist '{artist}'...");
                            let api = self.api.clone();
                            let a = artist.clone();
                            let result = tokio::runtime::Handle::current()
                                .block_on(api.fetch_artist_tracks(&a));
                            match result {
                                Ok(t) => {
                                    self.current_tracks = t;
                                    self.selected = 0;
                                    self.offset = 0;
                                    self.screen = Screen::ArtistTracks(artist);
                                    self.status = format!("{} tracks", self.current_tracks.len());
                                }
                                Err(e) => self.error = Some(e),
                            }
                        }
                    }
                    Screen::Tracks => {
                        self.play_selected_track();
                    }
                    Screen::AlbumTracks(_) | Screen::ArtistTracks(_) => {
                        self.play_selected_track();
                    }
                    _ => {}
                },
                KeyCode::Esc => match &self.screen {
                    Screen::AlbumTracks(_) | Screen::ArtistTracks(_) => {
                        self.screen = Screen::Tracks;
                    }
                    _ => {}
                },
                KeyCode::Char(' ') | KeyCode::Char('p') => {
                    self.play_selected_track();
                }
                _ => {}
            },
        }
        Ok(())
    }

    fn switch_screen(&mut self, screen: Screen) {
        if let Screen::Tracks = screen {
            if self.tracks.is_empty() {
                self.status = "loading tracks...".to_string();
                let api = self.api.clone();
                let result = tokio::runtime::Handle::current().block_on(api.fetch_tracks());
                match result {
                    Ok(t) => {
                        self.tracks = t;
                        self.current_tracks = self.tracks.clone();
                        self.status = format!("{} tracks", self.tracks.len());
                    }
                    Err(e) => self.error = Some(e),
                }
            } else {
                self.current_tracks = self.tracks.clone();
            }
            self.selected = 0;
            self.offset = 0;
            self.screen = Screen::Tracks;
        }
    }

    fn items_len(&self) -> usize {
        match &self.screen {
            Screen::Tracks => self.current_tracks.len(),
            Screen::Albums => self.albums.len(),
            Screen::Artists => self.artists.len(),
            Screen::AlbumTracks(_) => self.current_tracks.len(),
            Screen::ArtistTracks(_) => self.current_tracks.len(),
            Screen::Search => 0,
            Screen::Quit => 0,
        }
    }

    fn ensure_visible(&mut self) {
        let height = 20;
        if self.selected >= self.offset + height {
            self.offset = self.selected.saturating_sub(height) + 1;
        }
        if self.selected < self.offset {
            self.offset = self.selected;
        }
    }

    fn play_selected_track(&self) {
        let idx = self.selected;
        let tracks = &self.current_tracks;
        if idx >= tracks.len() {
            return;
        }
        let track = &tracks[idx];
        let url = self.api.stream_url(&track.id.to_string());
        let title = track.title.as_deref().unwrap_or("Unknown");
        let artist = track.artist.as_deref().unwrap_or("Unknown");
        let label = format!("Michi - {title} - {artist}");

        match std::process::Command::new("mpv")
            .args(["--no-video", &format!("--title={label}"), &url])
            .spawn()
        {
            Ok(_) => {}
            Err(_) => {
                // fallback to xdg-open
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;

    fn test_app() -> App {
        let client = ApiClient::new("http://localhost:8096".into(), None);
        App {
            api: client,
            screen: Screen::Tracks,
            tracks: vec![],
            albums: vec![],
            artists: vec![],
            current_tracks: vec![],
            search_query: String::new(),
            selected: 0,
            offset: 0,
            status: "test".into(),
            error: None,
        }
    }

    #[test]
    fn test_items_len_empty() {
        let app = test_app();
        assert_eq!(app.items_len(), 0);
    }

    #[test]
    fn test_items_len_tracks() {
        let mut app = test_app();
        app.current_tracks = vec![michi_core::Track {
            id: uuid::Uuid::new_v4(),
            title: Some("A".into()),
            artist: None,
            album: None,
            album_artist: None,
            duration_ms: None,
            file_path: "/a.flac".into(),
            format: michi_core::AudioFormat::Flac,
            sample_rate: None,
            bit_depth: None,
            channels: None,
            artwork_id: None,
            genre: None,
            year: None,
            track_number: None,
            disc_number: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }];
        assert_eq!(app.items_len(), 1);
    }

    #[test]
    fn test_ensure_visible_scrolls() {
        let mut app = test_app();
        app.current_tracks = vec![
            michi_core::Track {
                id: uuid::Uuid::new_v4(),
                title: Some("X".into()),
                artist: None,
                album: None,
                album_artist: None,
                duration_ms: None,
                file_path: "/x.flac".into(),
                format: michi_core::AudioFormat::Flac,
                sample_rate: None,
                bit_depth: None,
                channels: None,
                artwork_id: None,
                genre: None,
                year: None,
                track_number: None,
                disc_number: None,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            30
        ];
        app.selected = 25;
        app.ensure_visible();
        assert!(
            app.offset > 0,
            "offset should scroll when selection is beyond visible area"
        );
    }

    #[test]
    fn test_app_initial_state() {
        let app = test_app();
        assert_eq!(app.selected, 0);
        assert_eq!(app.offset, 0);
        assert!(app.search_query.is_empty());
        assert!(app.error.is_none());
    }
}
