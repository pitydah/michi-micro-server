use argon2::PasswordVerifier;
use axum::http::StatusCode;
use axum::Json;

use crate::models::{json_err, SubsonicQuery, SubsonicResponse};
use crate::routes::OsAppState;

/// Verify Subsonic API authentication against configured credentials and database users.
pub async fn check_auth(
    state: &OsAppState,
    query: &SubsonicQuery,
) -> Result<(), (StatusCode, Json<SubsonicResponse>)> {
    let query_user = query.u.as_deref().unwrap_or("");

    if query_user.is_empty() {
        return Err(json_err(
            crate::errors::NOT_AUTHENTICATED,
            "username required",
        ));
    }

    if !state.auth_enabled {
        // When auth is disabled server-wide, any non-empty username satisfies Subsonic protocol
        return Ok(());
    }

    // 1. Check against configured server admin credentials
    if let (Some(ref cfg_user), Some(ref cfg_pass)) = (&state.auth_username, &state.auth_password) {
        if query_user == cfg_user && verify_subsonic_credentials(query, cfg_pass) {
            return Ok(());
        }
    }

    // 2. Check against DB users if user exists in SQLite
    if let Ok(Some((_id, _username, password_hash, _is_admin))) =
        michi_db::get_user_by_username(&state.db, query_user).await
    {
        // For plain / hex password against stored Argon2 hash
        if let Some(ref p) = query.p {
            let decoded = if let Some(payload) = p.strip_prefix("enc:") {
                String::from_utf8(hex::decode(payload).unwrap_or_default()).unwrap_or_default()
            } else {
                p.clone()
            };

            if let Ok(parsed_hash) = argon2::PasswordHash::new(&password_hash) {
                if argon2::Argon2::default()
                    .verify_password(decoded.as_bytes(), &parsed_hash)
                    .is_ok()
                {
                    return Ok(());
                }
            }
        }
    }

    Err(json_err(
        crate::errors::NOT_AUTHENTICATED,
        "invalid username or password",
    ))
}

fn verify_subsonic_credentials(query: &SubsonicQuery, expected_pass: &str) -> bool {
    // Check password if provided (plain or hex-encoded "enc:<hex>")
    if let Some(ref p) = query.p {
        let decoded = if let Some(payload) = p.strip_prefix("enc:") {
            hex::decode(payload).unwrap_or_default()
        } else {
            p.as_bytes().to_vec()
        };
        if decoded == expected_pass.as_bytes() {
            return true;
        }
    }

    // Check token auth: ?t=<md5(password+salt)>&s=<salt>
    if let (Some(token), Some(salt)) = (&query.t, &query.s) {
        let expected = format!("{expected_pass}{salt}");
        let expected_hash = format!("{:x}", md5::compute(expected.as_bytes()));
        if token.eq_ignore_ascii_case(&expected_hash) {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_subsonic_plain_password() {
        let q = SubsonicQuery {
            u: Some("admin".to_string()),
            p: Some("secret123".to_string()),
            ..Default::default()
        };
        assert!(verify_subsonic_credentials(&q, "secret123"));
        assert!(!verify_subsonic_credentials(&q, "wrongpass"));
    }

    #[test]
    fn test_verify_subsonic_hex_password() {
        let q = SubsonicQuery {
            u: Some("admin".to_string()),
            p: Some(format!("enc:{}", hex::encode("secret123"))),
            ..Default::default()
        };
        assert!(verify_subsonic_credentials(&q, "secret123"));
        assert!(!verify_subsonic_credentials(&q, "wrongpass"));
    }

    #[test]
    fn test_verify_subsonic_token_salt() {
        let salt = "c1a2b3";
        let expected_hash = format!("{:x}", md5::compute(format!("secret123{salt}").as_bytes()));
        let q = SubsonicQuery {
            u: Some("admin".to_string()),
            t: Some(expected_hash),
            s: Some(salt.to_string()),
            ..Default::default()
        };

        assert!(verify_subsonic_credentials(&q, "secret123"));
        assert!(!verify_subsonic_credentials(&q, "wrongpass"));
    }
}
