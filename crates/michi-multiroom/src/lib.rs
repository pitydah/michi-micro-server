use tracing::info;

pub fn placeholder() -> &'static str {
    info!("michi-multiroom: placeholder loaded");
    "multiroom module (placeholder)"
}
