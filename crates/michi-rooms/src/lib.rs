// Michi Rooms — Snapcast integration wrapper

use serde::{Deserialize, Serialize};
use std::fmt;

pub const SNAPCAST_JSON_RPC_URL: &str = "http://127.0.0.1:1780/json-rpc";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SnapcastError {
    Transport(String),
    Timeout,
    HttpStatus(u16),
    JsonParse(String),
    RpcError { code: i64, message: String },
    InvalidResponse(String),
}

impl fmt::Display for SnapcastError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapcastError::Transport(e) => write!(f, "snapcast transport error: {e}"),
            SnapcastError::Timeout => write!(f, "snapcast request timed out"),
            SnapcastError::HttpStatus(code) => write!(f, "snapcast HTTP error status {code}"),
            SnapcastError::JsonParse(e) => write!(f, "snapcast JSON parse error: {e}"),
            SnapcastError::RpcError { code, message } => {
                write!(f, "snapcast RPC error {code}: {message}")
            }
            SnapcastError::InvalidResponse(e) => write!(f, "snapcast invalid response: {e}"),
        }
    }
}

impl std::error::Error for SnapcastError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapcastServerStatus {
    pub available: bool,
    pub version: Option<String>,
    pub host: String,
    pub port: u16,
    pub degraded: bool,
    pub error: Option<String>,
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

/// Centralized JSON-RPC 2.0 client for Snapcast communication.
/// Rigorously validates transport, timeout, HTTP status, JSON validity,
/// and JSON-RPC error objects.
pub fn validate_rpc_response(
    val: serde_json::Value,
    expected_id: i64,
) -> Result<serde_json::Value, SnapcastError> {
    if let Some(version) = val.get("jsonrpc") {
        if version.as_str() != Some("2.0") {
            return Err(SnapcastError::InvalidResponse(format!(
                "invalid jsonrpc version: {version:?}"
            )));
        }
    }

    if let Some(resp_id) = val.get("id") {
        if resp_id.as_i64() != Some(expected_id) {
            return Err(SnapcastError::InvalidResponse(format!(
                "response id mismatch: expected {expected_id}, got {resp_id:?}"
            )));
        }
    }

    let has_error = val.get("error").is_some() && !val["error"].is_null();
    let has_result = val.get("result").is_some() && !val["result"].is_null();

    if has_error && has_result {
        return Err(SnapcastError::InvalidResponse(
            "ambiguous JSON-RPC response containing both result and error".into(),
        ));
    }

    if !has_error && !has_result {
        return Err(SnapcastError::InvalidResponse(
            "missing 'result' or 'error' in JSON-RPC response".into(),
        ));
    }

    if has_error {
        let err_obj = &val["error"];
        let code = err_obj
            .get("code")
            .and_then(|c| c.as_i64())
            .unwrap_or(-32000);
        let msg = err_obj
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown RPC error")
            .to_string();
        return Err(SnapcastError::RpcError { code, message: msg });
    }

    Ok(val["result"].clone())
}

pub async fn rpc_call(
    url: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, SnapcastError> {
    let client = reqwest::Client::new();
    let req_id = 1;
    let req_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "method": method,
        "params": params,
    });

    let resp = client
        .post(url)
        .json(&req_body)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                SnapcastError::Timeout
            } else {
                SnapcastError::Transport(e.to_string())
            }
        })?;

    let status = resp.status();
    if !status.is_success() {
        return Err(SnapcastError::HttpStatus(status.as_u16()));
    }

    let val: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| SnapcastError::JsonParse(e.to_string()))?;

    validate_rpc_response(val, req_id)
}

pub fn parse_groups_from_value(result: &serde_json::Value) -> Result<Vec<Room>, SnapcastError> {
    let groups = result
        .get("server")
        .and_then(|s| s.get("groups"))
        .and_then(|g| g.as_array())
        .ok_or_else(|| {
            SnapcastError::InvalidResponse("missing 'server.groups' array in response".to_string())
        })?;

    let mut rooms = Vec::with_capacity(groups.len());
    for g in groups {
        let id = g
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                SnapcastError::InvalidResponse("group missing required non-empty 'id'".to_string())
            })?;

        let name = g
            .get("name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(id);

        let clients = g
            .get("clients")
            .and_then(|c| c.as_array())
            .map(|a| a.len() as u32)
            .unwrap_or(0);

        let muted = g.get("muted").and_then(|v| v.as_bool()).unwrap_or(false);
        let volume = g
            .get("volume")
            .and_then(|v| v.get("percent"))
            .and_then(|v| v.as_f64())
            .map(|v| v as u32)
            .unwrap_or(100);

        rooms.push(Room {
            id: id.to_string(),
            name: name.to_string(),
            muted,
            volume,
            client_count: clients,
        });
    }

    Ok(rooms)
}

pub async fn check_snapcast() -> SnapcastServerStatus {
    match rpc_call(
        SNAPCAST_JSON_RPC_URL,
        "Server.GetStatus",
        serde_json::json!({}),
    )
    .await
    {
        Ok(result) => {
            let version = result
                .get("server")
                .and_then(|s| s.get("version"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            SnapcastServerStatus {
                available: true,
                version,
                host: "127.0.0.1".into(),
                port: 1780,
                degraded: false,
                error: None,
            }
        }
        Err(e) => match e {
            SnapcastError::Transport(_) | SnapcastError::Timeout => unavailable(),
            other => SnapcastServerStatus {
                available: false,
                version: None,
                host: "127.0.0.1".into(),
                port: 1780,
                degraded: true,
                error: Some(other.to_string()),
            },
        },
    }
}

fn unavailable() -> SnapcastServerStatus {
    SnapcastServerStatus {
        available: false,
        version: None,
        host: "127.0.0.1".into(),
        port: 1780,
        degraded: false,
        error: None,
    }
}

pub async fn get_groups() -> Result<Vec<Room>, SnapcastError> {
    let result = rpc_call(
        SNAPCAST_JSON_RPC_URL,
        "Server.GetStatus",
        serde_json::json!({}),
    )
    .await?;

    parse_groups_from_value(&result)
}

pub async fn set_group_volume(group_id: &str, volume: u32) -> Result<(), SnapcastError> {
    rpc_call(
        SNAPCAST_JSON_RPC_URL,
        "Group.SetVolume",
        serde_json::json!({
            "id": group_id,
            "volume": {"percent": volume, "muted": false}
        }),
    )
    .await?;
    Ok(())
}

pub async fn set_group_mute(group_id: &str, muted: bool) -> Result<(), SnapcastError> {
    rpc_call(
        SNAPCAST_JSON_RPC_URL,
        "Group.SetMute",
        serde_json::json!({
            "id": group_id,
            "mute": muted
        }),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unavailable() {
        let s = unavailable();
        assert!(!s.available);
        assert!(!s.degraded);
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

    #[test]
    fn test_error_display() {
        let err = SnapcastError::RpcError {
            code: -32601,
            message: "Method not found".into(),
        };
        assert_eq!(
            err.to_string(),
            "snapcast RPC error -32601: Method not found"
        );

        let err2 = SnapcastError::HttpStatus(502);
        assert_eq!(err2.to_string(), "snapcast HTTP error status 502");
    }

    // ── Falsification Tests ──────────────────────────────────────────

    #[test]
    fn test_falsification_jsonrpc_error() {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "error": {
                "code": -32600,
                "message": "Invalid Request"
            }
        });
        let res = validate_rpc_response(payload, 1);
        assert!(matches!(
            res,
            Err(SnapcastError::RpcError { code: -32600, .. })
        ));
    }

    #[test]
    fn test_falsification_missing_result_and_error() {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1
        });
        let res = validate_rpc_response(payload, 1);
        assert!(matches!(res, Err(SnapcastError::InvalidResponse(_))));
    }

    #[test]
    fn test_falsification_ambiguous_result_and_error() {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {"ok": true},
            "error": {"code": 1, "message": "fail"}
        });
        let res = validate_rpc_response(payload, 1);
        assert!(matches!(res, Err(SnapcastError::InvalidResponse(_))));
    }

    #[test]
    fn test_falsification_wrong_response_id() {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 999,
            "result": {"ok": true}
        });
        let res = validate_rpc_response(payload, 1);
        assert!(matches!(res, Err(SnapcastError::InvalidResponse(_))));
    }

    #[test]
    fn test_falsification_group_missing_id() {
        let payload = serde_json::json!({
            "server": {
                "groups": [
                    {
                        "name": "Living Room",
                        "clients": []
                    }
                ]
            }
        });
        let res = parse_groups_from_value(&payload);
        assert!(matches!(res, Err(SnapcastError::InvalidResponse(_))));
    }

    #[test]
    fn test_falsification_valid_group() {
        let payload = serde_json::json!({
            "server": {
                "groups": [
                    {
                        "id": "grp-1",
                        "name": "Living Room",
                        "muted": false,
                        "volume": {"percent": 70},
                        "clients": [{"id": "c1"}]
                    }
                ]
            }
        });
        let res = parse_groups_from_value(&payload).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].id, "grp-1");
        assert_eq!(res[0].name, "Living Room");
        assert_eq!(res[0].volume, 70);
        assert_eq!(res[0].client_count, 1);
    }
}
