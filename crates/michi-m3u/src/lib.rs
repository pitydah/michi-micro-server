use thiserror::Error;

#[derive(Debug, Clone)]
pub struct M3uEntry {
    pub duration: Option<u64>,
    pub title: Option<String>,
    pub path: String,
}

#[derive(Debug, Error)]
pub enum M3uError {
    #[error("invalid M3U entry: {0}")]
    InvalidEntry(String),
}

pub fn parse_m3u(content: &str) -> Result<Vec<M3uEntry>, M3uError> {
    let mut entries = Vec::new();
    let mut extinf: Option<(Option<u64>, Option<String>)> = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            if let Some(info) = trimmed.strip_prefix("#EXTINF:") {
                let (dur_str, title) = info.split_once(',').unwrap_or((info, ""));
                let duration = dur_str.trim().parse::<u64>().ok();
                extinf = Some((
                    duration,
                    if title.is_empty() {
                        None
                    } else {
                        Some(title.to_string())
                    },
                ));
            }
            continue;
        }
        entries.push(M3uEntry {
            duration: extinf.as_ref().and_then(|(d, _)| *d),
            title: extinf.as_ref().and_then(|(_, t)| t.clone()),
            path: trimmed.to_string(),
        });
        extinf = None;
    }

    Ok(entries)
}

pub fn serialize_m3u(entries: &[M3uEntry]) -> String {
    let mut output = String::from("#EXTM3U\n");
    for entry in entries {
        if entry.duration.is_some() || entry.title.is_some() {
            let dur = entry.duration.unwrap_or(0);
            let title = entry.title.as_deref().unwrap_or("");
            output.push_str(&format!("#EXTINF:{},{}\n", dur, title));
        }
        output.push_str(&format!("{}\n", entry.path));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_m3u() {
        let m3u = "#EXTM3U\n\
                   #EXTINF:240,Test Song\n\
                   /music/test.flac\n\
                   #EXTINF:300,Another Song\n\
                   /music/another.flac\n";
        let entries = parse_m3u(m3u).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].duration, Some(240));
        assert_eq!(entries[0].title.as_deref(), Some("Test Song"));
        assert_eq!(entries[0].path, "/music/test.flac");
        assert_eq!(entries[1].duration, Some(300));
        assert_eq!(entries[1].title.as_deref(), Some("Another Song"));
    }

    #[test]
    fn test_parse_no_extinf() {
        let m3u = "/music/test.flac\n/music/other.flac\n";
        let entries = parse_m3u(m3u).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].duration.is_none());
        assert!(entries[0].title.is_none());
    }

    #[test]
    fn test_parse_without_header() {
        let m3u = "#EXTINF:180,Song\n/path/song.flac\n";
        let entries = parse_m3u(m3u).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].duration, Some(180));
    }

    #[test]
    fn test_parse_skip_comments() {
        let m3u = "# comment line\n\
                   /music/test.flac\n\
                   # another comment\n";
        let entries = parse_m3u(m3u).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "/music/test.flac");
    }

    #[test]
    fn test_serialize_roundtrip() {
        let entries = vec![
            M3uEntry {
                duration: Some(240),
                title: Some("Song One".into()),
                path: "/music/song1.flac".into(),
            },
            M3uEntry {
                duration: None,
                title: None,
                path: "/music/song2.flac".into(),
            },
        ];
        let output = serialize_m3u(&entries);
        assert!(output.starts_with("#EXTM3U\n"));
        assert!(output.contains("#EXTINF:240,Song One\n"));
        assert!(output.contains("/music/song2.flac\n"));

        let parsed = parse_m3u(&output).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].duration, Some(240));
        assert_eq!(parsed[0].title.as_deref(), Some("Song One"));
        assert_eq!(parsed[0].path, "/music/song1.flac");
        assert_eq!(parsed[1].duration, None);
        assert_eq!(parsed[1].path, "/music/song2.flac");
    }

    #[test]
    fn test_parse_empty() {
        let entries = parse_m3u("").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_only_comments() {
        let entries = parse_m3u("# comment\n# another\n").unwrap();
        assert!(entries.is_empty());
    }
}

#[derive(Debug, Clone)]
pub struct CueTrack {
    pub index: u32,
    pub title: String,
    pub performer: Option<String>,
    pub file: String,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}

pub fn parse_cue(content: &str) -> Vec<CueTrack> {
    let mut tracks = Vec::new();
    let mut current_file = String::new();
    let mut current_performer = None;
    let mut current_title = String::new();
    let mut current_index = 0u32;
    let mut current_start = 0u64;
    let mut in_track = false;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("REM") {
            continue;
        }
        if let Some(file) = line.strip_prefix("FILE ") {
            if let Some(path) = file.split('"').nth(1) {
                current_file = path.to_string();
            }
        } else if line.starts_with("PERFORMER ") {
            current_performer = line.split('"').nth(1).map(|s| s.to_string());
        } else if line.starts_with("TRACK ") {
            if in_track && !current_title.is_empty() {
                tracks.push(CueTrack {
                    index: current_index,
                    title: current_title.clone(),
                    performer: current_performer.clone(),
                    file: current_file.clone(),
                    start_ms: current_start,
                    end_ms: None,
                });
            }
            in_track = true;
            current_index = line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            current_title.clear();
        } else if line.starts_with("TITLE ") {
            current_title = line.split('"').nth(1).unwrap_or("").to_string();
        } else if line.starts_with("INDEX 01 ") {
            if let Some(time) = line.strip_prefix("INDEX 01 ") {
                let parts: Vec<&str> = time.split(':').collect();
                if parts.len() == 3 {
                    let mins: u64 = parts[0].parse().unwrap_or(0);
                    let secs: u64 = parts[1].parse().unwrap_or(0);
                    let frames: u64 = parts[2].parse().unwrap_or(0);
                    current_start = (mins * 60 + secs) * 1000 + (frames * 1000 / 75);
                }
            }
        }
    }

    if in_track && !current_title.is_empty() {
        tracks.push(CueTrack {
            index: current_index,
            title: current_title,
            performer: current_performer,
            file: current_file,
            start_ms: current_start,
            end_ms: None,
        });
    }

    // Calculate end_ms for each track (except last)
    for i in 0..tracks.len().saturating_sub(1) {
        tracks[i].end_ms = Some(tracks[i + 1].start_ms - 1);
    }

    tracks
}
