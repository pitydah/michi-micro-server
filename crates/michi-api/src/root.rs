use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::LazyLock;

static PROCESSED_HTML: LazyLock<String> = LazyLock::new(|| {
    let raw = include_str!("../static/index.html");
    let v = crate::assets::asset_version();
    raw.replace("__MICHI_ASSET_HASH__", v)
});

pub fn processed_html() -> &'static str {
    &PROCESSED_HTML
}

pub async fn root_handler() -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
        .header(header::PRAGMA, "no-cache")
        .header(header::EXPIRES, "0")
        .body(axum::body::Body::from(PROCESSED_HTML.as_str()))
        .unwrap()
}
