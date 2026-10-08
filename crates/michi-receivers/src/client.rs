use crate::models::*;
use crate::session_supervisor::ReceiverClientError;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Context for Trust Architecture V2 mutual authentication and transparent re-authentication.
#[derive(Debug, Clone)]
pub struct ReceiverHomeAuthContext {
    pub home_root_public_key: String,
    pub expected_home_id: String,
    pub client_membership: michi_identity::types::DeviceMembershipDto,
    pub revocations: Vec<michi_identity::types::HomeDeviceRevocationDto>,
}

/// HTTP client for interacting with a Michi Music Stream receiver (canonical Michi Link v1-lite).
pub struct ReceiverClient {
    pub base_url: String,
    client: reqwest::Client,
    pub token: Option<String>,
    pub active_session_id: Option<String>,
    pub active_session_token: Option<String>,
    pub heartbeat_sequence: Arc<AtomicU64>,
    pub identity: Option<Arc<michi_identity::IdentityManager>>,
    pub home_auth_context: Option<ReceiverHomeAuthContext>,
}

impl ReceiverClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token: None,
            active_session_id: None,
            active_session_token: None,
            heartbeat_sequence: Arc::new(AtomicU64::new(0)),
            identity: None,
            home_auth_context: None,
        }
    }

    pub fn with_identity(base_url: &str, identity: Arc<michi_identity::IdentityManager>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
            token: None,
            active_session_id: None,
            active_session_token: None,
            heartbeat_sequence: Arc::new(AtomicU64::new(0)),
            identity: Some(identity),
            home_auth_context: None,
        }
    }

    pub fn set_identity(&mut self, identity: Arc<michi_identity::IdentityManager>) {
        self.identity = Some(identity);
    }

    pub fn set_home_auth_context(&mut self, ctx: ReceiverHomeAuthContext) {
        self.home_auth_context = Some(ctx);
    }

    pub fn set_token(&mut self, token: impl Into<String>) {
        self.token = Some(token.into());
    }

    /// GET /api/v1/server/info (canonical v1-lite)
    pub async fn get_info(&self) -> Result<ReceiverInfo, String> {
        let resp = self
            .client
            .get(format!("{}/api/v1/server/info", self.base_url))
            .send()
            .await
            .map_err(|e| format!("info request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("info request failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("info parse failed: {e}"))
    }

    /// POST /api/v1/auth/challenge (Trust V2 canonical mutual authentication)
    pub async fn auth_challenge(
        &self,
        client_michi_id: &str,
        client_public_key: &str,
        home_id: &str,
    ) -> Result<michi_identity::types::DeviceAuthChallengeResponse, ReceiverProtocolError> {
        let payload = michi_identity::types::DeviceAuthChallengeRequest {
            client_michi_id: client_michi_id.to_string(),
            client_public_key: client_public_key.to_string(),
            home_id: home_id.to_string(),
        };

        let resp = self
            .client
            .post(format!("{}/api/v1/auth/challenge", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("auth_challenge request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("AUTH_CHALLENGE_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("auth challenge rejected")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "AUTH_CHALLENGE_FAILED".into(),
                message: format!("auth_challenge failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }

        let result: michi_identity::types::DeviceAuthChallengeResponse =
            resp.json().await.map_err(|e| ReceiverProtocolError {
                http_status: 500,
                code: "DECODE_ERROR".into(),
                message: format!("auth_challenge parse failed: {e}"),
                details: serde_json::Value::Null,
            })?;
        Ok(result)
    }

    /// POST /api/v1/auth/session (Trust V2 mutual session creation)
    pub async fn auth_session(
        &mut self,
        challenge_id: uuid::Uuid,
        client_michi_id: &str,
        membership: michi_identity::types::DeviceMembershipDto,
        client_signature: &str,
    ) -> Result<michi_identity::types::DeviceAuthSessionResponse, ReceiverProtocolError> {
        let payload = michi_identity::types::DeviceAuthSessionRequest {
            challenge_id,
            client_michi_id: client_michi_id.to_string(),
            membership,
            client_signature: client_signature.to_string(),
        };

        let resp = self
            .client
            .post(format!("{}/api/v1/auth/session", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("auth_session request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("AUTH_SESSION_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("auth session rejected")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "AUTH_SESSION_FAILED".into(),
                message: format!("auth_session failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }

        let result: michi_identity::types::DeviceAuthSessionResponse =
            resp.json().await.map_err(|e| ReceiverProtocolError {
                http_status: 500,
                code: "DECODE_ERROR".into(),
                message: format!("auth_session parse failed: {e}"),
                details: serde_json::Value::Null,
            })?;
        self.token = Some(result.session_token.clone());
        Ok(result)
    }

    /// High-level mutual authentication with Trust Architecture V2.
    pub async fn authenticate(
        &mut self,
        home_root_public_key: &str,
        expected_home_id: &str,
        client_membership: &michi_identity::types::DeviceMembershipDto,
        revocations: &[michi_identity::types::HomeDeviceRevocationDto],
    ) -> Result<michi_identity::types::DeviceAuthSessionResponse, ReceiverClientError> {
        let id = self.identity.as_ref().ok_or_else(|| {
            ReceiverClientError::Protocol("IdentityManager not configured on client".into())
        })?;

        let client_michi_id = id.michi_id().to_base64url();
        let client_public_key = id.public_key_base64url();

        let chal_resp = self
            .auth_challenge(&client_michi_id, &client_public_key, expected_home_id)
            .await
            .map_err(|e| ReceiverClientError::Protocol(format!("auth_challenge failed: {e}")))?;

        let challenge_id_str = chal_resp.challenge_id.to_string();

        // Compute client signature over canonical domain separation
        let payload = michi_identity::home::device_auth_challenge_payload(
            expected_home_id,
            &chal_resp.server_michi_id,
            &client_michi_id,
            &challenge_id_str,
            &chal_resp.challenge_nonce,
        );
        let (client_sig, _) = id.sign_base64url(&payload);

        let sess_resp = self
            .auth_session(
                chal_resp.challenge_id,
                &client_michi_id,
                client_membership.clone(),
                &client_sig,
            )
            .await
            .map_err(|e| ReceiverClientError::Protocol(format!("auth_session failed: {e}")))?;

        // Verify mutual server response
        michi_identity::home::verify_server_auth_session(
            &sess_resp,
            home_root_public_key,
            expected_home_id,
            &client_michi_id,
            &challenge_id_str,
            revocations,
        )
        .map_err(|e| {
            ReceiverClientError::Protocol(format!(
                "server mutual authentication verification failed: {e}"
            ))
        })?;

        self.token = Some(sess_resp.session_token.clone());
        Ok(sess_resp)
    }

    /// Re-authenticates using the stored home_auth_context if available.
    pub async fn reauthenticate(&mut self) -> Result<(), ReceiverClientError> {
        let ctx = self.home_auth_context.clone().ok_or_else(|| {
            ReceiverClientError::Protocol("No home auth context configured on client".into())
        })?;

        self.authenticate(
            &ctx.home_root_public_key,
            &ctx.expected_home_id,
            &ctx.client_membership,
            &ctx.revocations,
        )
        .await?;

        Ok(())
    }

    /// Push a signed revocation record to the receiver (POST /api/v1/home/revocations).
    pub async fn push_revocation(
        &self,
        revocation: &michi_identity::types::HomeDeviceRevocationDto,
    ) -> Result<(), ReceiverClientError> {
        let url = format!("{}/api/v1/home/revocations", self.base_url);
        let mut req = self.client.post(&url).json(revocation);
        if let Some(ref token) = self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| {
            if e.is_timeout() {
                ReceiverClientError::Timeout
            } else {
                ReceiverClientError::Offline(e.to_string())
            }
        })?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(ReceiverClientError::from_response_parts(
                status.as_u16(),
                &body,
            ))
        }
    }

    /// POST /api/v1/pair/start (canonical) with Ed25519 challenge signature over RAW nonce bytes
    pub async fn pair_start(&mut self, _initiator_id: &str) -> Result<PairStartResponse, String> {
        let (michi_id, public_key, challenge_nonce, challenge_signature) =
            if let Some(ref id) = self.identity {
                let nonce_raw: [u8; 32] = rand::random();
                let challenge_nonce = URL_SAFE_NO_PAD.encode(nonce_raw);
                let (sig_b64, pk_b64) = id.sign_base64url(&nonce_raw);
                (
                    id.michi_id().to_base64url(),
                    pk_b64,
                    challenge_nonce,
                    sig_b64,
                )
            } else {
                return Err("IdentityManager not configured on ReceiverClient".to_string());
            };

        let payload = serde_json::json!({
            "device_name": "Michi Micro Server",
            "device_type": "server",
            "roles": ["music_server", "playback_host"],
            "auth_strategy": "RECEIVER_BUTTON",
            "michi_id": michi_id,
            "public_key": public_key,
            "challenge_nonce": challenge_nonce,
            "challenge_signature": challenge_signature,
        });

        let resp = self
            .client
            .post(format!("{}/api/v1/pair/start", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("pair_start request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("pair_start failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("pair_start parse failed: {e}"))
    }

    /// POST /api/v1/pair/confirm (canonical) with 6-digit PIN verification
    pub async fn pair_confirm(
        &mut self,
        session_id: &str,
        _initiator_id: &str,
        pin: &str,
    ) -> Result<PairConfirmResponse, ReceiverProtocolError> {
        let (michi_id, public_key) = if let Some(ref id) = self.identity {
            (id.michi_id().to_base64url(), id.public_key_base64url())
        } else {
            return Err(ReceiverProtocolError {
                http_status: 500,
                code: "INTERNAL_ERROR".into(),
                message: "IdentityManager not configured on ReceiverClient".into(),
                details: serde_json::Value::Null,
            });
        };

        let payload = serde_json::json!({
            "session_id": session_id,
            "pin": pin,
            "michi_id": michi_id,
            "public_key": public_key,
        });

        let resp = self
            .client
            .post(format!("{}/api/v1/pair/confirm", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("pair_confirm request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PAIRING_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pair confirm rejected")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "PAIRING_FAILED".into(),
                message: format!("pair_confirm failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }
        let result: PairConfirmResponse = resp.json().await.map_err(|e| ReceiverProtocolError {
            http_status: 500,
            code: "DECODE_ERROR".into(),
            message: format!("pair_confirm parse failed: {e}"),
            details: serde_json::Value::Null,
        })?;
        if let Some(ref t) = result.token {
            self.token = Some(t.clone());
        }
        Ok(result)
    }

    /// GET /api/v1/pair/status?session_id=<uuid>
    pub async fn pair_status(
        &self,
        session_id: &str,
    ) -> Result<PairStatusResponse, ReceiverProtocolError> {
        let resp = self
            .client
            .get(format!(
                "{}/api/v1/pair/status?session_id={}",
                self.base_url, session_id
            ))
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("pair_status request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PAIR_STATUS_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pair status check failed")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "PAIR_STATUS_FAILED".into(),
                message: format!("pair_status failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }

        let result: PairStatusResponse = resp.json().await.map_err(|e| ReceiverProtocolError {
            http_status: 500,
            code: "DECODE_ERROR".into(),
            message: format!("pair_status parse failed: {e}"),
            details: serde_json::Value::Null,
        })?;
        Ok(result)
    }

    /// POST /api/v1/pair/recover/start to get a fresh receiver-issued single-use challenge nonce
    pub async fn pair_recover_start(
        &self,
    ) -> Result<PairRecoverStartResponse, ReceiverProtocolError> {
        let (michi_id, public_key) = if let Some(ref id) = self.identity {
            (id.michi_id().to_base64url(), id.public_key_base64url())
        } else {
            return Err(ReceiverProtocolError {
                http_status: 500,
                code: "INTERNAL_ERROR".into(),
                message: "IdentityManager not configured on ReceiverClient".into(),
                details: serde_json::Value::Null,
            });
        };

        let payload = serde_json::json!({
            "michi_id": michi_id,
            "public_key": public_key,
        });

        let resp = self
            .client
            .post(format!("{}/api/v1/pair/recover/start", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("pair_recover_start request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PAIR_RECOVER_START_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pair recover start failed")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "PAIR_RECOVER_START_FAILED".into(),
                message: format!("pair_recover_start failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }

        let result: PairRecoverStartResponse =
            resp.json().await.map_err(|e| ReceiverProtocolError {
                http_status: 500,
                code: "DECODE_ERROR".into(),
                message: format!("pair_recover_start parse failed: {e}"),
                details: serde_json::Value::Null,
            })?;
        Ok(result)
    }

    /// Validates receiver server_michi_id and server_public_key before signing any recovery challenge.
    /// Ensures that:
    /// 1. server_public_key is a valid 32-byte Ed25519 public key.
    /// 2. server_michi_id matches the BLAKE3 derivation of server_public_key (ed25519-blake3-v1).
    /// 3. If expected_michi_id / expected_public_key are provided, server identity matches expected values.
    /// 4. challenge_nonce is valid base64url and at least 16 bytes.
    pub fn validate_recover_start_identity(
        start_resp: &PairRecoverStartResponse,
        expected_michi_id: Option<&str>,
        expected_public_key: Option<&str>,
    ) -> Result<(), ReceiverProtocolError> {
        let pk_bytes = URL_SAFE_NO_PAD
            .decode(&start_resp.server_public_key)
            .map_err(|e| ReceiverProtocolError {
                http_status: 400,
                code: "INVALID_SERVER_PUBLIC_KEY".into(),
                message: format!("invalid base64url server_public_key: {e}"),
                details: serde_json::Value::Null,
            })?;
        let key_bytes: [u8; 32] =
            pk_bytes
                .as_slice()
                .try_into()
                .map_err(|_| ReceiverProtocolError {
                    http_status: 400,
                    code: "INVALID_SERVER_PUBLIC_KEY".into(),
                    message: "server_public_key must be exactly 32 bytes".into(),
                    details: serde_json::Value::Null,
                })?;
        let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes).map_err(|e| {
            ReceiverProtocolError {
                http_status: 400,
                code: "INVALID_SERVER_PUBLIC_KEY".into(),
                message: format!("invalid Ed25519 server_public_key: {e}"),
                details: serde_json::Value::Null,
            }
        })?;
        let derived_michi_id =
            michi_identity::types::MichiId::from_public_key(&verifying_key).to_base64url();

        if start_resp.server_michi_id != derived_michi_id {
            return Err(ReceiverProtocolError {
                http_status: 400,
                code: "IDENTITY_MISMATCH".into(),
                message: format!(
                    "server_michi_id '{}' does not match derived identity '{}'",
                    start_resp.server_michi_id, derived_michi_id
                ),
                details: serde_json::Value::Null,
            });
        }

        if let Some(exp_id) = expected_michi_id {
            if start_resp.server_michi_id != exp_id {
                return Err(ReceiverProtocolError {
                    http_status: 400,
                    code: "IDENTITY_MISMATCH".into(),
                    message: format!(
                        "server_michi_id '{}' does not match expected '{}'",
                        start_resp.server_michi_id, exp_id
                    ),
                    details: serde_json::Value::Null,
                });
            }
        }

        if let Some(exp_pk) = expected_public_key {
            if start_resp.server_public_key != exp_pk {
                return Err(ReceiverProtocolError {
                    http_status: 400,
                    code: "IDENTITY_MISMATCH".into(),
                    message: format!(
                        "server_public_key '{}' does not match expected '{}'",
                        start_resp.server_public_key, exp_pk
                    ),
                    details: serde_json::Value::Null,
                });
            }
        }

        let nonce_bytes = URL_SAFE_NO_PAD
            .decode(&start_resp.challenge_nonce)
            .map_err(|e| ReceiverProtocolError {
                http_status: 400,
                code: "INVALID_CHALLENGE".into(),
                message: format!("invalid base64url challenge nonce: {e}"),
                details: serde_json::Value::Null,
            })?;
        if nonce_bytes.len() < 16 {
            return Err(ReceiverProtocolError {
                http_status: 400,
                code: "INVALID_CHALLENGE".into(),
                message: "challenge_nonce must be at least 16 bytes".into(),
                details: serde_json::Value::Null,
            });
        }

        Ok(())
    }

    /// POST /api/v1/pair/recover with Ed25519 signature
    pub async fn pair_recover(
        &mut self,
        challenge_nonce: &str,
    ) -> Result<PairConfirmResponse, ReceiverProtocolError> {
        let (michi_id, public_key, signature) = if let Some(ref id) = self.identity {
            let nonce_bytes =
                URL_SAFE_NO_PAD
                    .decode(challenge_nonce)
                    .map_err(|e| ReceiverProtocolError {
                        http_status: 400,
                        code: "INVALID_CHALLENGE".into(),
                        message: format!("invalid base64url challenge nonce: {e}"),
                        details: serde_json::Value::Null,
                    })?;
            let (sig, pk) = id.sign_base64url(&nonce_bytes);
            (id.michi_id().to_base64url(), pk, sig)
        } else {
            return Err(ReceiverProtocolError {
                http_status: 500,
                code: "INTERNAL_ERROR".into(),
                message: "IdentityManager not configured on ReceiverClient".into(),
                details: serde_json::Value::Null,
            });
        };

        let payload = serde_json::json!({
            "michi_id": michi_id,
            "public_key": public_key,
            "challenge_nonce": challenge_nonce,
            "challenge_signature": signature,
        });

        let resp = self
            .client
            .post(format!("{}/api/v1/pair/recover", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| ReceiverProtocolError {
                http_status: 503,
                code: "NETWORK_ERROR".into(),
                message: format!("pair_recover request failed: {e}"),
                details: serde_json::Value::Null,
            })?;

        let status = resp.status();
        if !status.is_success() {
            let status_code = status.as_u16();
            if let Ok(err_val) = resp.json::<serde_json::Value>().await {
                if let Some(err_obj) = err_val.get("error") {
                    let code = err_obj
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("PAIRING_FAILED")
                        .to_string();
                    let message = err_obj
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pair recover rejected")
                        .to_string();
                    let details = err_obj
                        .get("details")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);
                    return Err(ReceiverProtocolError {
                        http_status: status_code,
                        code,
                        message,
                        details,
                    });
                }
            }
            return Err(ReceiverProtocolError {
                http_status: status_code,
                code: "PAIR_RECOVER_FAILED".into(),
                message: format!("pair_recover failed with status {status}"),
                details: serde_json::Value::Null,
            });
        }

        let result: PairConfirmResponse = resp.json().await.map_err(|e| ReceiverProtocolError {
            http_status: 500,
            code: "DECODE_ERROR".into(),
            message: format!("pair_recover parse failed: {e}"),
            details: serde_json::Value::Null,
        })?;

        // Strict validation per pair-recover-response.schema.json:
        // required: ["token", "expires_in", "device_id", "server_id"]
        let token = result
            .token
            .as_ref()
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| ReceiverProtocolError {
                http_status: 500,
                code: "CONTRACT_VIOLATION".into(),
                message: "recover response missing or empty token".into(),
                details: serde_json::Value::Null,
            })?;
        if result.expires_in.is_none() {
            return Err(ReceiverProtocolError {
                http_status: 500,
                code: "CONTRACT_VIOLATION".into(),
                message: "recover response missing expires_in".into(),
                details: serde_json::Value::Null,
            });
        }
        let _device_id = result
            .device_id
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .ok_or_else(|| ReceiverProtocolError {
                http_status: 500,
                code: "CONTRACT_VIOLATION".into(),
                message: "recover response missing or empty device_id".into(),
                details: serde_json::Value::Null,
            })?;
        let _server_id = result
            .server_id
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| ReceiverProtocolError {
                http_status: 500,
                code: "CONTRACT_VIOLATION".into(),
                message: "recover response missing or empty server_id".into(),
                details: serde_json::Value::Null,
            })?;

        self.token = Some(token.clone());
        Ok(result)
    }

    /// Performs 2-step replay-resistant recovery:
    /// 1. POST /api/v1/pair/recover/start -> obtains receiver challenge_nonce
    /// 2. Validates server_michi_id and server_public_key before signing
    /// 3. POST /api/v1/pair/recover -> signs receiver challenge_nonce and recovers token
    /// 4. Strictly validates recover response
    pub async fn pair_recover_auto(
        &mut self,
        expected_michi_id: Option<&str>,
        expected_public_key: Option<&str>,
    ) -> Result<PairConfirmResponse, ReceiverProtocolError> {
        let start_resp = self.pair_recover_start().await?;
        Self::validate_recover_start_identity(&start_resp, expected_michi_id, expected_public_key)?;
        self.pair_recover(&start_resp.challenge_nonce).await
    }

    fn apply_session_headers(&self, mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(ref stok) = self.active_session_token {
            req = req.header("X-Michi-Session", stok);
        }
        if let Some(ref t) = self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }
        req
    }

    /// POST /api/v1/receiver-lite/heartbeat (canonical)
    pub async fn heartbeat(&self) -> Result<HeartbeatResponse, ReceiverClientError> {
        let session_id = self.active_session_id.as_ref().ok_or_else(|| {
            ReceiverClientError::Protocol(
                "NoActiveSession: cannot heartbeat without active session".to_string(),
            )
        })?;

        let seq = self.heartbeat_sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let sent_at_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let payload = serde_json::json!({
            "session_id": session_id,
            "sequence": seq,
            "sent_at_ms": sent_at_ms,
        });

        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver-lite/heartbeat", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req.json(&payload).send().await.map_err(|e| {
            if e.is_timeout() {
                ReceiverClientError::Timeout
            } else {
                ReceiverClientError::Offline(e.to_string())
            }
        })?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ReceiverClientError::from_response_parts(
                status.as_u16(),
                &body,
            ));
        }
        resp.json()
            .await
            .map_err(|e| ReceiverClientError::Protocol(format!("heartbeat parse failed: {e}")))
    }

    /// POST /api/v1/receiver-lite/session with optional Perch authority grant
    #[allow(clippy::too_many_arguments)]
    pub async fn session_start_with_authority(
        &mut self,
        _session_id_hint: &str,
        codec: &str,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
        _stream_port_hint: u16,
        buffer_ms: u64,
        volume: u32,
        authority: Option<&crate::authority_models::AuthorityGrant>,
    ) -> Result<NegotiatedReceiverSession, String> {
        if volume > 100 {
            return Err(format!("volume {volume} exceeds maximum of 100"));
        }
        let ssrc: u32 = rand::random::<u32>().max(1);

        let payload = serde_json::json!({
            "transport": "rtp_udp",
            "codec": codec,
            "sample_rate": sample_rate,
            "bit_depth": bit_depth,
            "channels": channels,
            "packet_ms": 10,
            "buffer_ms": buffer_ms,
            "payload_type": 97,
            "ssrc": ssrc,
            "volume": volume,
        });

        let mut req = self
            .client
            .post(format!("{}/api/v1/receiver-lite/session", self.base_url));
        if let Some(ref t) = self.token {
            req = req.header("Authorization", format!("Bearer {t}"));
        }

        if let Some(grant) = authority {
            req = req
                .header("X-Michi-Authority-Grant", &grant.grant_token)
                .header("X-Michi-Authority-Instance", &grant.authority_instance_id)
                .header("X-Michi-Authority-Epoch", grant.lease_epoch.to_string())
                .header("X-Authority-Grant", &grant.grant_token)
                .header("X-Authority-Instance", &grant.authority_instance_id)
                .header("X-Authority-Epoch", grant.lease_epoch.to_string());
        }

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("session_start request failed: {e}"))?;

        let status = resp.status();
        if status != reqwest::StatusCode::CREATED {
            return Err(format!(
                "session_start failed: expected HTTP 201 Created, got {status}"
            ));
        }
        let raw_resp: SessionStartResponse = resp
            .json()
            .await
            .map_err(|e| format!("session_start parse failed: {e}"))?;

        let negotiated = raw_resp.validate_strict()?;

        self.active_session_id = Some(negotiated.session_id.clone());
        self.active_session_token = Some(negotiated.session_token.clone());
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        Ok(negotiated)
    }

    /// POST /api/v1/receiver-lite/session (canonical HTTP 201)
    #[allow(clippy::too_many_arguments)]
    pub async fn session_start(
        &mut self,
        session_id_hint: &str,
        codec: &str,
        sample_rate: u32,
        bit_depth: u32,
        channels: u32,
        stream_port_hint: u16,
        buffer_ms: u64,
        volume: u32,
    ) -> Result<NegotiatedReceiverSession, String> {
        self.session_start_with_authority(
            session_id_hint,
            codec,
            sample_rate,
            bit_depth,
            channels,
            stream_port_hint,
            buffer_ms,
            volume,
            None,
        )
        .await
    }

    /// PATCH /api/v1/receiver-lite/session (canonical)
    pub async fn set_volume(&self, volume: u32) -> Result<VolumeResponse, String> {
        if self.active_session_id.is_none() {
            return Err("NoActiveSession: cannot set volume without active session".to_string());
        }
        if volume > 100 {
            return Err(format!("volume {volume} exceeds maximum of 100"));
        }
        let payload = serde_json::json!({
            "volume": volume,
        });

        let mut req = self
            .client
            .patch(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("set_volume request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("set_volume failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("set_volume parse failed: {e}"))
    }

    /// PATCH /api/v1/receiver-lite/session {"paused": bool}
    pub async fn session_pause(&mut self, paused: bool) -> Result<(), String> {
        if self.active_session_id.is_none() {
            return Err(
                "NoActiveSession: cannot pause/resume session when no session is active"
                    .to_string(),
            );
        }
        let payload = serde_json::json!({
            "paused": paused,
        });
        let mut req = self
            .client
            .patch(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("session_pause request failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("session_pause failed with status {status}"));
        }
        Ok(())
    }

    /// DELETE /api/v1/receiver-lite/session (canonical HTTP 204 or 200)
    pub async fn session_stop(&mut self) -> Result<SessionStopResponse, String> {
        if self.active_session_id.is_none() {
            return Err(
                "NoActiveSession: cannot stop session when no session is active".to_string(),
            );
        }
        let mut req = self
            .client
            .delete(format!("{}/api/v1/receiver-lite/session", self.base_url));
        req = self.apply_session_headers(req);

        let resp = req
            .send()
            .await
            .map_err(|e| format!("session_stop request failed: {e}"))?;

        let status = resp.status();
        if status != reqwest::StatusCode::NO_CONTENT
            && status != reqwest::StatusCode::OK
            && !status.is_success()
        {
            return Err(format!("session_stop failed with status {status}"));
        }

        self.active_session_id = None;
        self.active_session_token = None;
        self.heartbeat_sequence.store(0, Ordering::SeqCst);

        Ok(SessionStopResponse {
            status: Some("session_stopped".to_string()),
            session_id: None,
            error: None,
        })
    }

    /// Verifies whether the currently configured token is accepted by the receiver using
    /// an authenticated endpoint (GET /api/v1/receiver-lite/session).
    ///
    /// Unlike /api/v1/server/info which is public/unauthenticated, GET /receiver-lite/session
    /// requires BearerAuth:
    /// - Returns Ok(true) if status is 200 (active session) or 404 (no active session, but authenticated).
    /// - Returns Ok(false) if status is 401 or 403 (unauthorized/invalid token).
    pub async fn verify_token(&self) -> Result<bool, ReceiverProtocolError> {
        let req = self
            .client
            .get(format!("{}/api/v1/receiver-lite/session", self.base_url));
        let req = self.apply_session_headers(req);
        let resp = req.send().await.map_err(|e| ReceiverProtocolError {
            http_status: 503,
            code: "NETWORK_ERROR".into(),
            message: format!("token verification request failed: {e}"),
            details: serde_json::Value::Null,
        })?;

        let status = resp.status().as_u16();
        if status == 401 || status == 403 {
            Ok(false)
        } else if status == 200 || status == 404 {
            Ok(true)
        } else {
            Err(ReceiverProtocolError {
                http_status: status,
                code: "TOKEN_VERIFICATION_FAILED".into(),
                message: format!("unexpected status {status} during token verification"),
                details: serde_json::Value::Null,
            })
        }
    }

    /// GET /api/v1/receiver-lite/session (canonical)
    pub async fn get_playback_state(&self) -> Result<ReceiverPlaybackState, String> {
        let req = self
            .client
            .get(format!("{}/api/v1/receiver-lite/session", self.base_url));
        let req = self.apply_session_headers(req);
        let resp = req
            .send()
            .await
            .map_err(|e| format!("get_playback_state failed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            return Err(format!("get_playback_state failed with status {status}"));
        }
        resp.json()
            .await
            .map_err(|e| format!("get_playback_state parse failed: {e}"))
    }

    // Fault injection helpers
    pub async fn fault_latency(&self, latency_ms: u64) -> Result<(), String> {
        let payload = serde_json::json!({ "latency_ms": latency_ms });
        self.client
            .post(format!("{}/api/v1/receiver/fault/latency", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn fault_offline(&self, offline: bool) -> Result<(), String> {
        let payload = serde_json::json!({ "offline": offline });
        self.client
            .post(format!("{}/api/v1/receiver/fault/offline", self.base_url))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn fault_network_drop(&self, drop_count: u32) -> Result<(), String> {
        let payload = serde_json::json!({ "drop_count": drop_count });
        self.client
            .post(format!(
                "{}/api/v1/receiver/fault/network_drop",
                self.base_url
            ))
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub async fn fault_reset(&self) -> Result<(), String> {
        self.client
            .post(format!("{}/api/v1/receiver/fault/reset", self.base_url))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
