use axum::response::{IntoResponse, Response};

const CSS: &str = include_str!("../static/styles.css");
const JS: &str = include_str!("../static/app.js");
const LOGO: &[u8] = include_bytes!("../static/assets/michi-logo.svg");

pub async fn styles_css() -> impl IntoResponse {
    Response::builder()
        .header("content-type", "text/css; charset=utf-8")
        .body(axum::body::Body::from(CSS))
        .unwrap()
}

pub async fn app_js() -> impl IntoResponse {
    Response::builder()
        .header("content-type", "application/javascript; charset=utf-8")
        .body(axum::body::Body::from(JS))
        .unwrap()
}

pub async fn logo_svg() -> impl IntoResponse {
    Response::builder()
        .header("content-type", "image/svg+xml")
        .header("cache-control", "public, max-age=86400")
        .body(axum::body::Body::from(LOGO))
        .unwrap()
}
