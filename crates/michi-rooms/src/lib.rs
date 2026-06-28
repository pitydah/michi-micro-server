// Michi Rooms — Snapcast integration wrapper

use serde::{Deserialize, Serialize};

const SNAPCAST_JSON_RPC_URL: &str = "http://127.0.0.1:1780/json-rpc";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapcastServerStatus {
    pub available: bool,
    pub version: Option<String>,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub muted: bool,
    pub volume: u32,
    pub client_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapClient {
    pub id: String,
    pub name: String,
    pub host: String,
    pub connected: bool,
    pub volume: u32,
    pub latency_ms: u32,
    pub group_id: Option<String>,
}

pub async fn check_snapcast() -> SnapcastServerStatus {
    let client = reqwest::Client::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "Server.GetStatus",
        "params": {}
    });
    match client
        .post(SNAPCAST_JSON_RPC_URL)
        .json(&req)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
    {
        Ok(resp) => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let version = body
                    .get("result")
                    .and_then(|r| r.get("server"))
                    .and_then(|s| s.get("version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                SnapcastServerStatus {
                    available: true,
                    version,
                    host: "127.0.0.1".into(),
                    port: 1780,
                }
            } else {
                unavailable()
            }
        }
        Err(_) => unavailable(),
    }
}

fn unavailable() -> SnapcastServerStatus {
    SnapcastServerStatus {
        available: false,
        version: None,
        host: "127.0.0.1".into(),
        port: 1780,
    }
}

pub async fn get_groups() -> Result<Vec<Room>, String> {
    let client = reqwest::Client::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "Server.GetStatus",
        "params": {}
    });
    let resp = client
        .post(SNAPCAST_JSON_RPC_URL)
        .json(&req)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("snapcast connection failed: {e}"))?;
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("snapcast parse failed: {e}"))?;
    let groups = body
        .get("result")
        .and_then(|r| r.get("server"))
        .and_then(|s| s.get("groups"))
        .and_then(|g| g.as_array())
        .ok_or_else(|| "unexpected snapcast response".to_string())?;

    Ok(groups
        .iter()
        .map(|g| {
            let clients = g
                .get("clients")
                .and_then(|c| c.as_array())
                .map(|a| a.len() as u32)
                .unwrap_or(0);
            Room {
                id: g
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                name: g
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unnamed")
                    .to_string(),
                muted: g.get("muted").and_then(|v| v.as_bool()).unwrap_or(false),
                volume: g
                    .get("volume")
                    .and_then(|v| v.get("percent"))
                    .and_then(|v| v.as_f64())
                    .map(|v| v as u32)
                    .unwrap_or(100),
                client_count: clients,
            }
        })
        .collect())
}

pub async fn set_group_volume(group_id: &str, volume: u32) -> Result<(), String> {
    let client = reqwest::Client::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "Group.SetVolume",
        "params": {
            "id": group_id,
            "volume": {"percent": volume, "muted": false}
        }
    });
    client
        .post(SNAPCAST_JSON_RPC_URL)
        .json(&req)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("snapcast volume failed: {e}"))?;
    Ok(())
}

pub async fn set_group_mute(group_id: &str, muted: bool) -> Result<(), String> {
    let client = reqwest::Client::new();
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "Group.SetMute",
        "params": {"id": group_id, "mute": muted}
    });
    client
        .post(SNAPCAST_JSON_RPC_URL)
        .json(&req)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| format!("snapcast mute failed: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unavailable() {
        let s = unavailable();
        assert!(!s.available);
        assert_eq!(s.host, "127.0.0.1");
        assert_eq!(s.port, 1780);
    }

    #[test]
    fn test_room_serde() {
        let room = Room {
            id: "test-id".into(),
            name: "Living Room".into(),
            muted: false,
            volume: 80,
            client_count: 2,
        };
        let json = serde_json::to_string(&room).unwrap();
        assert!(json.contains("Living Room"));
        assert!(json.contains("80"));
    }

    #[test]
    fn test_snapclient_serde() {
        let c = SnapClient {
            id: "cli-1".into(),
            name: "Kitchen Speaker".into(),
            host: "192.168.1.100".into(),
            connected: true,
            volume: 75,
            latency_ms: 100,
            group_id: Some("group-1".into()),
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("Kitchen Speaker"));
        assert!(json.contains("75"));
    }
}
