use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::permissions::{DevicePermissions, Permission};
use crate::LinkError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub device_id: Uuid,
    pub alias: String,
    pub device_type: String,
    pub device_model: Option<String>,
    pub token_hash: String,
    pub permissions: DevicePermissions,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingSession {
    pub pairing_id: Uuid,
    pub code: String,
    pub device_name: String,
    pub device_type: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub confirmed: bool,
}

impl DeviceEntry {
    pub fn new(
        device_id: Uuid,
        alias: String,
        device_type: String,
        device_model: Option<String>,
        token_hash: String,
    ) -> Self {
        let permissions = match device_type.as_str() {
            "player" | "michi-player" => DevicePermissions::player(),
            "mobile" | "michi-mobile" => DevicePermissions::mobile(),
            "stream-receiver" | "michi-stream" => DevicePermissions::stream_receiver(),
            _ => DevicePermissions::default(),
        };

        Self {
            device_id,
            alias,
            device_type,
            device_model,
            token_hash,
            permissions,
            created_at: chrono::Utc::now(),
            last_seen: None,
            revoked: false,
        }
    }

    pub fn check_permission(&self, permission: &Permission) -> Result<(), LinkError> {
        if self.revoked {
            return Err(LinkError::DeviceRevoked);
        }
        if !self.permissions.has(permission) {
            return Err(LinkError::InsufficientPermissions(format!(
                "{:?}",
                permission
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenPair {
    pub device_token: String,
    pub refresh_token: String,
    pub device_id: Uuid,
    pub permissions: DevicePermissions,
}

pub fn generate_pairing_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let code: String = (0..6)
        .map(|_| {
            let idx = rng.gen_range(0..36);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'A' + idx - 10) as char
            }
        })
        .collect();
    code
}

pub fn generate_device_token() -> String {
    Uuid::new_v4().to_string()
}
