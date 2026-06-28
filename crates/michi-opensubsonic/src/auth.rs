use crate::models::SubsonicQuery;

pub fn check_auth(
    query: &SubsonicQuery,
) -> Result<
    (),
    (
        axum::http::StatusCode,
        axum::Json<crate::models::SubsonicResponse>,
    ),
> {
    // Check that client provides at least a username
    if query.u.as_deref().unwrap_or("").is_empty() {
        return Err(crate::models::json_err(
            crate::errors::NOT_AUTHENTICATED,
            "username required",
        ));
    }
    Ok(())
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
    // All versions accepted for now
    Ok(())
}
