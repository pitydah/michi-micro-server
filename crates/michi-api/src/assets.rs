use sha2::{Digest, Sha256};
use std::sync::LazyLock;

pub static ASSET_VERSION: LazyLock<String> = LazyLock::new(|| {
    let mut hasher = Sha256::new();
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(include_bytes!("../static/styles.css"));
    hasher.update(include_bytes!("../static/hero-cat.css"));
    hasher.update(include_bytes!("../static/app.js"));
    let hex = hex::encode(hasher.finalize());
    format!("{}-{}", env!("CARGO_PKG_VERSION"), &hex[..12])
});

pub fn asset_version() -> &'static str {
    &ASSET_VERSION
}

pub fn build_commit() -> Option<String> {
    std::env::var("MICHI_BUILD_COMMIT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            option_env!("MICHI_BUILD_COMMIT")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}
