use serde::{Deserialize, Serialize};
use serde_json::Value;

const OS_VERSION: &str = "1.16.1";

#[derive(Debug, Serialize)]
pub struct SubsonicResponse {
    #[serde(rename = "subsonic-response")]
    pub response: SubsonicBody,
}

#[derive(Debug, Serialize)]
pub struct SubsonicBody {
    pub status: &'static str,
    pub version: &'static str,
    #[serde(rename = "type")]
    pub server_type: &'static str,
    #[serde(rename = "serverVersion")]
    pub server_version: &'static str,
    #[serde(rename = "openSubsonic")]
    pub open_subsonic: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<SubsonicError>,
    #[serde(flatten)]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct SubsonicError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct SubsonicQuery {
    pub u: Option<String>,
    pub p: Option<String>,
    pub t: Option<String>,
    pub s: Option<String>,
    pub c: Option<String>,
    pub v: Option<String>,
    pub f: Option<String>,
}

impl SubsonicQuery {
    pub fn format(&self) -> &str {
        self.f.as_deref().unwrap_or("xml")
    }
}

pub fn ok_response(data: Option<Value>) -> SubsonicResponse {
    SubsonicResponse {
        response: SubsonicBody {
            status: "ok",
            version: OS_VERSION,
            server_type: "michi-micro-server",
            server_version: env!("CARGO_PKG_VERSION"),
            open_subsonic: true,
            error: None,
            data,
        },
    }
}

pub fn err_response(code: i32, message: &str) -> SubsonicResponse {
    SubsonicResponse {
        response: SubsonicBody {
            status: "failed",
            version: OS_VERSION,
            server_type: "michi-micro-server",
            server_version: env!("CARGO_PKG_VERSION"),
            open_subsonic: true,
            error: Some(SubsonicError {
                code,
                message: message.to_string(),
            }),
            data: None,
        },
    }
}

pub fn json_ok(data: Option<Value>) -> axum::Json<SubsonicResponse> {
    axum::Json(ok_response(data))
}

pub fn json_err(code: i32, msg: &str) -> (axum::http::StatusCode, axum::Json<SubsonicResponse>) {
    (
        axum::http::StatusCode::OK,
        axum::Json(err_response(code, msg)),
    )
}
