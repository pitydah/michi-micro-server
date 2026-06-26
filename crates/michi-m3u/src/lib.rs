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
