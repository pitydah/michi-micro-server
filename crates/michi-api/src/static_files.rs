use axum::{
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

const CSS: &str = include_str!("../static/styles.css");
const JS: &str = include_str!("../static/app.js");
const LOGO: &[u8] = include_bytes!("../static/assets/michi-logo.svg");
const FAVICON_SVG: &[u8] = include_bytes!("../static/assets/michi-micro-server.svg");
const FAVICON_PNG: &[u8] = include_bytes!("../static/assets/michi-micro-server.png");

const I18N_EN: &str = include_str!("../static/i18n/en.json");
const I18N_ES: &str = include_str!("../static/i18n/es.json");
const I18N_PT: &str = include_str!("../static/i18n/pt.json");
const I18N_DE: &str = include_str!("../static/i18n/de.json");
const I18N_FR: &str = include_str!("../static/i18n/fr.json");
const I18N_IT: &str = include_str!("../static/i18n/it.json");
const I18N_RU: &str = include_str!("../static/i18n/ru.json");
const I18N_ZH: &str = include_str!("../static/i18n/zh.json");
const I18N_JA: &str = include_str!("../static/i18n/ja.json");

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

pub async fn favicon_svg() -> impl IntoResponse {
    Response::builder()
        .header("content-type", "image/svg+xml")
        .header("cache-control", "public, max-age=86400")
        .body(axum::body::Body::from(FAVICON_SVG))
        .unwrap()
}

pub async fn favicon_png() -> impl IntoResponse {
    Response::builder()
        .header("content-type", "image/png")
        .header("cache-control", "public, max-age=86400")
        .body(axum::body::Body::from(FAVICON_PNG))
        .unwrap()
}

pub async fn i18n_handler(
    Path(lang): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<serde_json::Value>)> {
    let data = match lang.as_str() {
        "en" => I18N_EN,
        "es" => I18N_ES,
        "pt" => I18N_PT,
        "de" => I18N_DE,
        "fr" => I18N_FR,
        "it" => I18N_IT,
        "ru" => I18N_RU,
        "zh" => I18N_ZH,
        "ja" => I18N_JA,
        _ => I18N_EN,
    };
    Ok(Response::builder()
        .header("content-type", "application/json; charset=utf-8")
        .header("cache-control", "public, max-age=3600")
        .body(axum::body::Body::from(data))
        .unwrap())
}
