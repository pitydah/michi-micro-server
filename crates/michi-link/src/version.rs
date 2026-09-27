//! Version information for the Michi Link protocol.
//!
//! Note: `api_version` is always `"v1"` and is defined in the server handler.
//! There is no `michi_link_version` — the API contract version is solely `api_version`.
//! This module only holds internal build/application version.

/// Internal application version. Not part of the API contract.
pub const APP_VERSION: &str = "0.1.0";

/// Canonical distributed authority contract profile version.
pub const AUTHORITY_PROFILE_VERSION: &str = "authority-v1";

/// Canonical receiver-lite protocol profile version.
pub const RECEIVER_LITE_PROFILE_VERSION: &str = "v1-lite";

/// Vendored Michi Link submodule commit hash pin.
pub const MICHI_LINK_VENDOR_COMMIT: &str = "1b0684a9457beb0f8d78b491af16a06541f8508d";
