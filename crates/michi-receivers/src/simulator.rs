use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Simulated receiver state for testing without hardware
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulatedReceiver {
    pub id: Uuid,
    pub name: String,
    pub device_type: String,
    pub host: String,
    pub port: u16,
    pub device_token: Option<String>,
    pub paired: bool,
    pub session_active: bool,
    pub current_track_id: Option<Uuid>,
    pub volume: u32,
    pub online: bool,
    pub uptime_seconds: u64,
}

impl SimulatedReceiver {
    pub fn new(name: &str, host: &str, port: u16) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            device_type: "michi-stream".into(),
            host: host.to_string(),
            port,
            device_token: None,
            paired: false,
            session_active: false,
            current_track_id: None,
            volume: 70,
            online: true,
            uptime_seconds: 0,
        }
    }

    pub fn info(&self) -> crate::models::ReceiverInfo {
        crate::models::ReceiverInfo {
            id: self.id,
            name: self.name.clone(),
            device_type: self.device_type.clone(),
            host: self.host.clone(),
            port: self.port,
            capabilities: vec!["stream".into(), "volume".into()],
            online: self.online,
        }
    }

    pub fn pair_confirm(&mut self, code: &str) -> Option<crate::models::PairConfirmResponse> {
        if code.len() == 6 && code.chars().all(|c| c.is_ascii_alphanumeric()) {
            self.paired = true;
            self.device_token = Some(Uuid::new_v4().to_string());
            Some(crate::models::PairConfirmResponse {
                device_token: self.device_token.clone().unwrap(),
                refresh_token: Uuid::new_v4().to_string(),
                device_id: self.id,
                alias: self.name.clone(),
                permissions: vec![
                    "server.read".into(),
                    "stream.read".into(),
                    "playback.read".into(),
                    "receiver.read".into(),
                    "receiver.control".into(),
                    "receiver.session".into(),
                    "receiver.volume".into(),
                ],
            })
        } else {
            None
        }
    }

    pub fn heartbeat(&mut self) {
        self.uptime_seconds += 60;
        self.online = true;
    }

    pub fn session_start(&mut self, track_id: Uuid, volume: u32) {
        self.session_active = true;
        self.current_track_id = Some(track_id);
        self.volume = volume.min(100);
    }

    pub fn session_stop(&mut self) {
        self.session_active = false;
        self.current_track_id = None;
    }

    pub fn set_volume(&mut self, volume: u32) {
        self.volume = volume.min(100);
    }
}
