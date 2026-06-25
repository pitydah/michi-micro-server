use tracing::info;

pub fn placeholder() -> &'static str {
    info!("michi-sync: placeholder loaded");
    "sync module (placeholder)"
}
