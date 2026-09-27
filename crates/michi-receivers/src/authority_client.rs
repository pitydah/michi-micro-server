use crate::authority_models::*;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use std::time::Duration;
use url::Url;

/// HTTP client for Perch authority operations on Michi Music Stream.
#[derive(Clone)]
pub struct ReceiverAuthorityClient {
    http: reqwest::Client,
}

impl Default for ReceiverAuthorityClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ReceiverAuthorityClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { http }
    }

    fn apply_auth(headers: &mut HeaderMap, token: Option<&str>) {
        if let Some(tok) = token {
            if let Ok(val) = HeaderValue::from_str(&format!("Bearer {tok}")) {
                headers.insert(AUTHORIZATION, val);
            }
        }
    }

    /// GET /api/v1/authority/info
    pub async fn info(
        &self,
        endpoint: &Url,
        token: Option<&str>,
    ) -> Result<AuthorityInfo, AuthorityError> {
        let url = endpoint
            .join("api/v1/authority/info")
            .map_err(|e| AuthorityError::Protocol(format!("invalid url: {e}")))?;

        let mut headers = HeaderMap::new();
        Self::apply_auth(&mut headers, token);

        let resp = self
            .http
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| AuthorityError::Http(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(AuthorityError::Unsupported);
        }
        if !resp.status().is_success() {
            return Err(AuthorityError::Http(format!("status {}", resp.status())));
        }

        resp.json::<AuthorityInfo>()
            .await
            .map_err(|e| AuthorityError::Protocol(e.to_string()))
    }

    /// GET /api/v1/authority/state
    pub async fn state(
        &self,
        endpoint: &Url,
        token: Option<&str>,
    ) -> Result<AuthorityState, AuthorityError> {
        let url = endpoint
            .join("api/v1/authority/state")
            .map_err(|e| AuthorityError::Protocol(format!("invalid url: {e}")))?;

        let mut headers = HeaderMap::new();
        Self::apply_auth(&mut headers, token);

        let resp = self
            .http
            .get(url)
            .headers(headers)
            .send()
            .await
            .map_err(|e| AuthorityError::Http(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AuthorityError::Unauthorized("invalid token".into()));
        }
        if !resp.status().is_success() {
            return Err(AuthorityError::Http(format!("status {}", resp.status())));
        }

        resp.json::<AuthorityState>()
            .await
            .map_err(|e| AuthorityError::Protocol(e.to_string()))
    }

    /// POST /api/v1/authority/claim
    pub async fn claim(
        &self,
        endpoint: &Url,
        token: Option<&str>,
        claimant_michi_id: &str,
        claimant_name: &str,
        claimant_service: &str,
    ) -> Result<AuthorityGrant, AuthorityError> {
        let url = endpoint
            .join("api/v1/authority/claim")
            .map_err(|e| AuthorityError::Protocol(format!("invalid url: {e}")))?;

        let mut headers = HeaderMap::new();
        Self::apply_auth(&mut headers, token);

        let body = serde_json::json!({
            "claimant_michi_id": claimant_michi_id,
            "claimant_name": claimant_name,
            "claimant_service": claimant_service,
        });

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthorityError::Http(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            let error_json: serde_json::Value = resp.json().await.unwrap_or_default();
            let details = error_json.get("error").and_then(|e| e.get("details"));
            let owner_michi_id = details
                .and_then(|d| d.get("owner_michi_id"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned);
            let owner_name = details
                .and_then(|d| d.get("owner_name"))
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned);
            return Err(AuthorityError::Occupied {
                owner_michi_id,
                owner_name,
            });
        }

        if !resp.status().is_success() {
            return Err(AuthorityError::Http(format!(
                "claim status {}",
                resp.status()
            )));
        }

        resp.json::<AuthorityGrant>()
            .await
            .map_err(|e| AuthorityError::Protocol(e.to_string()))
    }

    /// POST /api/v1/authority/release
    pub async fn release(
        &self,
        endpoint: &Url,
        token: Option<&str>,
        stamp: &AuthorityStamp,
    ) -> Result<AuthorityState, AuthorityError> {
        let url = endpoint
            .join("api/v1/authority/release")
            .map_err(|e| AuthorityError::Protocol(format!("invalid url: {e}")))?;

        let mut headers = HeaderMap::new();
        Self::apply_auth(&mut headers, token);

        let body = serde_json::json!({
            "authority_instance_id": stamp.authority_instance_id,
            "lease_epoch": stamp.lease_epoch,
        });

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthorityError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AuthorityError::Http(format!(
                "release status {}",
                resp.status()
            )));
        }

        resp.json::<AuthorityState>()
            .await
            .map_err(|e| AuthorityError::Protocol(e.to_string()))
    }

    /// POST /api/v1/authority/takeover (explicit takeover / Pounce)
    pub async fn takeover(
        &self,
        endpoint: &Url,
        token: Option<&str>,
        claimant_michi_id: &str,
        claimant_name: &str,
        claimant_service: &str,
        user_explicit: bool,
    ) -> Result<AuthorityGrant, AuthorityError> {
        if !user_explicit {
            return Err(AuthorityError::TakeoverRequiresExplicit);
        }

        let url = endpoint
            .join("api/v1/authority/takeover")
            .map_err(|e| AuthorityError::Protocol(format!("invalid url: {e}")))?;

        let mut headers = HeaderMap::new();
        Self::apply_auth(&mut headers, token);

        let body = serde_json::json!({
            "claimant_michi_id": claimant_michi_id,
            "claimant_name": claimant_name,
            "claimant_service": claimant_service,
            "user_explicit": true,
        });

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthorityError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AuthorityError::Http(format!(
                "takeover status {}",
                resp.status()
            )));
        }

        resp.json::<AuthorityGrant>()
            .await
            .map_err(|e| AuthorityError::Protocol(e.to_string()))
    }

    /// POST /api/v1/authority/handoff (PawPass authority step)
    pub async fn handoff(
        &self,
        endpoint: &Url,
        token: Option<&str>,
        target_michi_id: &str,
        target_proof: &str,
        stamp: &AuthorityStamp,
    ) -> Result<AuthorityGrant, AuthorityError> {
        let url = endpoint
            .join("api/v1/authority/handoff")
            .map_err(|e| AuthorityError::Protocol(format!("invalid url: {e}")))?;

        let mut headers = HeaderMap::new();
        Self::apply_auth(&mut headers, token);

        let body = serde_json::json!({
            "target_michi_id": target_michi_id,
            "target_ready_proof": target_proof,
            "authority_instance_id": stamp.authority_instance_id,
            "lease_epoch": stamp.lease_epoch,
        });

        let resp = self
            .http
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|e| AuthorityError::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AuthorityError::Http(format!(
                "handoff status {}",
                resp.status()
            )));
        }

        resp.json::<AuthorityGrant>()
            .await
            .map_err(|e| AuthorityError::Protocol(e.to_string()))
    }
}
