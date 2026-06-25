use tracing::info;

pub fn placeholder() -> &'static str {
    info!("michi-homeassistant: placeholder loaded");
    "home assistant module (placeholder)"
}
