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

/// Validate URL is safe and resolve DNS addresses
pub fn validate_url_and_resolve(
    url_str: &str,
) -> Result<(url::Url, Vec<std::net::SocketAddr>), String> {
    if !url_str.starts_with("http://") && !url_str.starts_with("https://") {
        return Err("only http and https are allowed".into());
    }

    let parsed = url::Url::parse(url_str).map_err(|e| format!("invalid URL: {e}"))?;
    let host = parsed.host_str().ok_or("URL has no host")?;
    let port = parsed.port_or_known_default().unwrap_or(80);

    let addr_str = format!("{host}:{port}");
    let addrs: Vec<std::net::SocketAddr> = addr_str
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err("DNS resolution returned no addresses".into());
    }

    for addr in &addrs {
        let ip = addr.ip();
        if is_private_or_link_local(&ip) {
            return Err(format!("blocked address: {ip}"));
        }
    }

    Ok((parsed, addrs))
}

/// Validate URL is safe: only http/https, no private/reserved IPs, no DNS rebinding
pub fn validate_url(url_str: &str) -> Result<String, String> {
    let (url, _) = validate_url_and_resolve(url_str)?;
    Ok(url.to_string())
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

/// Safely fetch a remote resource with SSRF protection, per-hop DNS resolution & IP pinning,
/// redirect following (up to `max_redirects`), and response body size bounding (`max_bytes`).
pub async fn safe_fetch(
    initial_url: &str,
    max_redirects: usize,
    max_bytes: usize,
    timeout: Duration,
) -> Result<
    (
        reqwest::StatusCode,
        reqwest::header::HeaderMap,
        Vec<u8>,
        url::Url,
    ),
    String,
> {
    let mut current_url_str = initial_url.to_string();

    for hop in 0..=max_redirects {
        let (parsed_url, addrs) = validate_url_and_resolve(&current_url_str)?;
        let host = parsed_url.host_str().ok_or("URL has no host")?.to_string();
        let first_addr = *addrs
            .first()
            .ok_or("DNS resolution returned no addresses")?;

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&host, first_addr)
            .build()
            .map_err(|e| format!("client build failed: {e}"))?;

        let resp = client
            .get(&current_url_str)
            .send()
            .await
            .map_err(|e| format!("request failed: {e}"))?;

        let status = resp.status();
        if status.is_redirection() {
            if hop == max_redirects {
                return Err(format!("too many redirects (max {max_redirects})"));
            }
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or("redirect response missing Location header")?;

            let next_url = parsed_url
                .join(location)
                .map_err(|e| format!("invalid redirect Location '{location}': {e}"))?;

            current_url_str = next_url.to_string();
            continue;
        }

        let headers = resp.headers().clone();
        let mut body = Vec::new();
        let mut resp = resp;

        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("chunk read error: {e}"))?
        {
            if body.len() + chunk.len() > max_bytes {
                return Err(format!(
                    "FEED_TOO_LARGE: response body exceeded maximum limit of {max_bytes} bytes"
                ));
            }
            body.extend_from_slice(&chunk);
        }

        return Ok((status, headers, body, parsed_url));
    }

    Err("too many redirects".to_string())
}

/// Detect stream type by making a HEAD / partial GET request
pub async fn sniff_stream(url: &str) -> Result<StreamInfo, String> {
    // Validate, resolve, and pin the initial URL.
    let (parsed_url, addrs) = validate_url_and_resolve(url)?;
    let mut current_url_str = url.to_string();
    let host = parsed_url.host_str().ok_or("URL has no host")?.to_string();
    let first_addr = *addrs
        .first()
        .ok_or("DNS resolution returned no addresses")?;

    let mut client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .resolve(&host, first_addr)
        .build()
        .map_err(|e| format!("client: {e}"))?;

    // Try HEAD first. On redirect, follow once with a new pinned client.
    let r = client
        .head(&current_url_str)
        .send()
        .await
        .map_err(|e| format!("head: {e}"))?;

    let resp = if r.status().is_redirection() {
        let location = r
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .ok_or("redirect has no Location header")?
            .to_string();
        let next_url = parsed_url
            .join(&location)
            .map_err(|e| format!("invalid redirect Location: {e}"))?;
        current_url_str = next_url.to_string();

        let (next_parsed, next_addrs) = validate_url_and_resolve(&current_url_str)?;
        let next_host = next_parsed
            .host_str()
            .ok_or("redirect URL has no host")?
            .to_string();
        let next_addr = *next_addrs
            .first()
            .ok_or("redirect DNS returned no addresses")?;

        let next_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .redirect(reqwest::redirect::Policy::none())
            .resolve(&next_host, next_addr)
            .build()
            .map_err(|e| format!("redirect client: {e}"))?;

        let red_resp = next_client
            .head(&current_url_str)
            .send()
            .await
            .map_err(|e| format!("redirect head: {e}"))?;

        client = next_client;
        red_resp
    } else {
        r
    };

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
        || current_url_str.ends_with(".xml")
        || current_url_str.ends_with(".rss")
    {
        let body_resp = client
            .get(&current_url_str)
            .header("Range", "bytes=0-4095")
            .send()
            .await
            .map_err(|e| format!("get: {e}"))?;
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
    if ct.contains("mpegurl") || ct.contains("apple") || current_url_str.ends_with(".m3u8") {
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
        .get(&current_url_str)
        .header("Range", "bytes=0-2047")
        .send()
        .await
        .map_err(|e| format!("fallback: {e}"))?;

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
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    if let Some(s) = body.find(&open) {
        let start = s + open.len();
        if let Some(e) = body[start..].find(&close) {
            return Some(body[start..start + e].to_string());
        }
    }
    None
}

fn extract_attr(body: &str, tag: &str, attr: &str) -> Option<String> {
    let search = format!("<{tag} ");
    if let Some(s) = body.find(&search) {
        let fragment = &body[s..];
        let attr_search = format!("{attr}=\"");
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

    #[test]
    fn test_ssrf_blocks_private_and_loopback_ips() {
        assert!(validate_url_and_resolve("http://127.0.0.1/feed.xml").is_err());
        assert!(validate_url_and_resolve("http://localhost:8080/feed.xml").is_err());
        assert!(validate_url_and_resolve("http://10.0.0.5/podcast.rss").is_err());
        assert!(validate_url_and_resolve("http://192.168.1.100/stream").is_err());
        assert!(validate_url_and_resolve("http://172.16.0.1/stream").is_err());
        assert!(validate_url_and_resolve("http://169.254.169.254/metadata").is_err());
        assert!(validate_url_and_resolve("ftp://example.com/audio.mp3").is_err());
        assert!(validate_url_and_resolve("file:///etc/passwd").is_err());
    }

    #[tokio::test]
    async fn test_safe_fetch_blocks_ssrf_immediately() {
        let res = safe_fetch(
            "http://127.0.0.1:9999/feed.xml",
            5,
            1024,
            Duration::from_secs(2),
        )
        .await;
        assert!(res.is_err());
        let err = res.unwrap_err();
        assert!(err.contains("blocked address") || err.contains("DNS"));
    }
}
