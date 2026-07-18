//! Version information for the Michi Link protocol.
//!
//! Note: `api_version` is always `"v1"` and is defined in the server handler.
//! There is no `michi_link_version` — the API contract version is solely `api_version`.
//! This module only holds internal build/application version.

/// Internal application version. Not part of the API contract.
pub const APP_VERSION: &str = "0.1.0";
