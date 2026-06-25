use tracing::info;

pub fn placeholder() -> &'static str {
    info!("michi-streaming: placeholder loaded");
    "streaming module (placeholder)"
}
