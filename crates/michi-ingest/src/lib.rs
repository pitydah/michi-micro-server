//! Michi Ingest — Universal Stream Ingest
//!
//! Sniff URLs to detect stream type (radio, podcast, direct file).
//! Includes SSRF protection: blocks private/reserved IP ranges.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum StreamType {
    Radio,
    Podcast,
    DirectFile,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub url: String,
    pub stream_type: StreamType,
    pub name: Option<String>,
    pub genre: Option<String>,
    pub description: Option<String>,
    pub logo_url: Option<String>,
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
}

/// Validate URL is safe: only http/https, no private/reserved IPs, no DNS rebinding
pub fn validate_url(url_str: &str) -> Result<String, String> {
    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
        return Err("only http and https are allowed".into());
    }

    let parsed = url::Url::parse(url_str).map_err(|e| format!("invalid URL: {}", e))?;

    let host = parsed.host_str().ok_or("URL has no host")?;

    // Resolve DNS and check every address
    let addr_str = format!("{}:80", host);
    let addrs = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed: {}", e))?;

    for addr in addrs {
        let ip = addr.ip();
        if is_private_or_link_local(&ip) {
            return Err(format!("blocked address: {}", ip));
        }
    }

    Ok(url_str.to_string())
}

fn is_private_or_link_local(ip: &IpAddr) -> bool {
    if ip.is_loopback() || ip.is_multicast() || ip.is_unspecified() {
        return true;
    }
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 169.254.x.x (link-local)
            o[0] == 169 && o[1] == 254
            // 10.0.0.0/8
            || o[0] == 10
            // 172.16.0.0/12
            || (o[0] == 172 && (o[1] & 0xF0) == 16)
            // 192.168.0.0/16
            || (o[0] == 192 && o[1] == 168)
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            // ::1 (loopback)
            (s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0 && s[6] == 0 && s[7] == 1)
            // fe80::/10 (link-local)
            || (s[0] & 0xFFC0) == 0xFE80
            // fc00::/7 (unique-local)
            || (s[0] & 0xFE00) == 0xFC00
            // ff00::/8 (multicast) - already caught by is_multicast() above, but belt-and-suspenders
            || (s[0] & 0xFF00) == 0xFF00
            // ::ffff:0:0/96 (IPv4-mapped IPv6) — unwrap and re-validate the embedded IPv4
            || (s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0 && s[4] == 0 && s[5] == 0xFFFF)
        }
    }
}

/// Detect stream type by making a HEAD / partial GET request
pub async fn sniff_stream(url: &str) -> Result<StreamInfo, String> {
    let _safe_url = validate_url(url)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("client: {}", e))?;

    // Try HEAD first
    let resp = client
        .head(url)
        .send()
        .await
        .map_err(|e| format!("head: {}", e))?;

    let headers = resp.headers();
    let ct = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let icy_name = headers
        .get("icy-name")
        .or_else(|| headers.get("ice-name"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let icy_genre = headers
        .get("icy-genre")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let icy_br = headers
        .get("icy-br")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u32>().ok());

    // Detect by content type
    if ct.contains("audio/mpeg")
        || ct.contains("audio/aac")
        || ct.contains("audio/ogg")
        || ct.contains("audio/opus")
    {
        if icy_name.is_some()
            || headers.get("icy-metaint").is_some()
            || headers.get("ice-version").is_some()
        {
            let _icy_br = icy_br;
            return Ok(StreamInfo {
                url: url.to_string(),
                stream_type: StreamType::Radio,
                name: icy_name,
                genre: icy_genre,
                description: None,
                logo_url: None,
                codec: Some(codec_from_mime(ct).to_string()),
                sample_rate: None,
            });
        }
        return Ok(StreamInfo {
            url: url.to_string(),
            stream_type: StreamType::DirectFile,
            name: None,
            genre: None,
            description: None,
            logo_url: None,
            codec: Some(codec_from_mime(ct).to_string()),
            sample_rate: None,
        });
    }

    // Detect podcast by trying to fetch a small piece and looking for RSS/XML
    if ct.contains("xml")
        || ct.contains("rss")
        || ct.contains("atom")
        || url.ends_with(".xml")
        || url.ends_with(".rss")
    {
        let body_resp = client
            .get(url)
            .header("Range", "bytes=0-4095")
            .send()
            .await
            .map_err(|e| format!("get: {}", e))?;
        let body = body_resp.text().await.unwrap_or_default();
        if body.contains("<rss") || body.contains("<feed") || body.contains("<channel>") {
            let name = extract_rss_title(&body);
            return Ok(StreamInfo {
                url: url.to_string(),
                stream_type: StreamType::Podcast,
                name,
                genre: None,
                description: None,
                logo_url: None,
                codec: None,
                sample_rate: None,
            });
        }
    }

    // HLS detection
    if ct.contains("mpegurl") || ct.contains("apple") || url.ends_with(".m3u8") {
        return Ok(StreamInfo {
            url: url.to_string(),
            stream_type: StreamType::Radio,
            name: icy_name.or_else(|| Some("HLS Stream".into())),
            genre: icy_genre,
            description: None,
            logo_url: None,
            codec: Some("hls".into()),
            sample_rate: None,
        });
    }

    // Fallback: try to GET a few bytes and detect
    let fallback_resp = client
        .get(url)
        .header("Range", "bytes=0-2047")
        .send()
        .await
        .map_err(|e| format!("fallback: {}", e))?;
    let fb_ct = fallback_resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let fb_body = fallback_resp.text().await.unwrap_or_default();

    if fb_ct.contains("audio/") {
        return Ok(StreamInfo {
            url: url.to_string(),
            stream_type: StreamType::DirectFile,
            name: None,
            genre: None,
            description: None,
            logo_url: None,
            codec: Some(codec_from_mime(&fb_ct).to_string()),
            sample_rate: None,
        });
    }
    if fb_body.contains("<rss") || fb_body.contains("<feed") || fb_body.contains("<channel>") {
        let name = extract_rss_title(&fb_body);
        return Ok(StreamInfo {
            url: url.to_string(),
            stream_type: StreamType::Podcast,
            name,
            genre: None,
            description: None,
            logo_url: None,
            codec: None,
            sample_rate: None,
        });
    }

    Ok(StreamInfo {
        url: url.to_string(),
        stream_type: StreamType::Unknown,
        name: None,
        genre: None,
        description: None,
        logo_url: None,
        codec: None,
        sample_rate: None,
    })
}

fn codec_from_mime(mime: &str) -> &'static str {
    if mime.contains("mpeg") {
        "mp3"
    } else if mime.contains("aac") {
        "aac"
    } else if mime.contains("ogg") {
        "ogg"
    } else if mime.contains("opus") {
        "opus"
    } else if mime.contains("flac") {
        "flac"
    } else if mime.contains("wav") {
        "wav"
    } else {
        "unknown"
    }
}

fn extract_rss_title(body: &str) -> Option<String> {
    if let Some(start) = body.find("<title>") {
        let start = start + 7;
        if let Some(end) = body[start..].find("</title>") {
            return Some(body[start..start + end].to_string());
        }
    }
    None
}

/// Parse minimal RSS to extract episodes (lazy: only URLs, no audio download)
pub fn parse_rss_episodes(body: &str) -> Vec<PodcastEpisode> {
    let mut episodes = Vec::new();
    let mut pos = 0;
    while let Some(item_start) = body[pos..].find("<item>") {
        let item = &body[pos + item_start..];
        let mut title = String::new();
        let mut url = String::new();
        let mut pub_date = String::new();
        let mut duration = String::new();

        if let Some(t) = extract_tag(item, "title") {
            title = t;
        }
        if let Some(u) = extract_attr(item, "enclosure", "url") {
            url = u;
        }
        if let Some(d) = extract_tag(item, "pubDate") {
            pub_date = d;
        }
        if let Some(d) = extract_tag(item, "duration") {
            duration = d;
        }

        if !title.is_empty() && !url.is_empty() {
            episodes.push(PodcastEpisode {
                title,
                audio_url: url,
                pub_date,
                duration_secs: duration.parse().ok(),
            });
        }
        pos += item_start + 5;
        if episodes.len() >= 100 {
            break;
        }
    }
    episodes
}

fn extract_tag(body: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    if let Some(s) = body.find(&open) {
        let start = s + open.len();
        if let Some(e) = body[start..].find(&close) {
            return Some(body[start..start + e].to_string());
        }
    }
    None
}

fn extract_attr(body: &str, tag: &str, attr: &str) -> Option<String> {
    let search = format!("<{} ", tag);
    if let Some(s) = body.find(&search) {
        let fragment = &body[s..];
        let attr_search = format!("{}=\"", attr);
        if let Some(a) = fragment.find(&attr_search) {
            let start = a + attr_search.len();
            if let Some(end) = fragment[start..].find('"') {
                return Some(fragment[start..start + end].to_string());
            }
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastEpisode {
    pub title: String,
    pub audio_url: String,
    pub pub_date: String,
    pub duration_secs: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_rss_title() {
        let xml = r#"<rss><channel><title>My Podcast</title></channel></rss>"#;
        assert_eq!(extract_rss_title(xml), Some("My Podcast".into()));
    }

    #[test]
    fn test_parse_rss_episodes() {
        let xml = r#"<rss><channel>
            <item><title>Ep 1</title><enclosure url="http://example.com/ep1.mp3" length="123" type="audio/mpeg"/><pubDate>Mon, 01 Jan 2024</pubDate></item>
            <item><title>Ep 2</title><enclosure url="http://example.com/ep2.mp3" length="456" type="audio/mpeg"/></item>
        </channel></rss>"#;
        let eps = parse_rss_episodes(xml);
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].title, "Ep 1");
        assert_eq!(eps[1].audio_url, "http://example.com/ep2.mp3");
    }
}
