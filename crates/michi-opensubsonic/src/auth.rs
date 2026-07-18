use crate::models::SubsonicQuery;

/// Verify Subsonic API authentication (sync version for use with ? operator).
///
/// To use with auth, call `check_auth_with_config` instead.
/// This version accepts any non-empty username.
pub fn check_auth(
    query: &SubsonicQuery,
) -> Result<
    (),
    (
        axum::http::StatusCode,
        axum::Json<crate::models::SubsonicResponse>,
    ),
> {
    let username = query.u.as_deref().unwrap_or("");
    if username.is_empty() {
        return Err(crate::models::json_err(
            crate::errors::NOT_AUTHENTICATED,
            "username required",
        ));
    }
    Ok(())
}

/// Verify Subsonic API authentication with configured credentials.
pub fn check_auth_with_config(
    query: &SubsonicQuery,
    username: &str,
    password: &str,
) -> Result<
    (),
    (
        axum::http::StatusCode,
        axum::Json<crate::models::SubsonicResponse>,
    ),
> {
    let query_user = query.u.as_deref().unwrap_or("");

    if query_user.is_empty() {
        return Err(crate::models::json_err(
            crate::errors::NOT_AUTHENTICATED,
            "username required",
        ));
    }

    // If username doesn't match, fail
    if query_user != username {
        return Err(crate::models::json_err(
            crate::errors::NOT_AUTHENTICATED,
            "invalid username or password",
        ));
    }

    // Check password if provided (legacy: hex-encoded "enc:<hex>")
    if let Some(ref p) = query.p {
        let decoded = if p.starts_with("enc:") {
            hex::decode(&p[4..]).unwrap_or_default()
        } else {
            p.as_bytes().to_vec()
        };
        if decoded == password.as_bytes() {
            return Ok(());
        }
    }

    // Check token auth: ?t=<md5(password+salt)>&s=<salt>
    if let (Some(token), Some(salt)) = (&query.t, &query.s) {
        let expected = format!("{}{}", password, salt);
        let expected_hash = format!("{:x}", md5::compute(expected.as_bytes()));
        if *token == expected_hash {
            return Ok(());
        }
    }

    Err(crate::models::json_err(
        crate::errors::NOT_AUTHENTICATED,
        "invalid username or password",
    ))
}

pub fn check_server_version(
    _query: &SubsonicQuery,
) -> Result<
    (),
    (
        axum::http::StatusCode,
        axum::Json<crate::models::SubsonicResponse>,
    ),
> {
    Ok(())
}
