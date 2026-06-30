use axum::response::Html;

pub async fn root_handler() -> Html<&'static str> {
    Html(HTML)
}

const HTML: &str = include_str!("../static/index.html");
