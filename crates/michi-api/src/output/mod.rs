pub mod receiver_sink;

pub use receiver_sink::ReceiverAudioSink;

use michi_playback::{AudioSink, PlaybackError, PlaybackOutputDescription};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaybackOutputSelection {
    Receiver { id: String },
    RoomGroup { id: Uuid },
    Chain { id: Uuid },
}

pub struct ResolvedOutputPlan {
    pub sinks: Vec<Box<dyn AudioSink>>,
    pub description: PlaybackOutputDescription,
}

pub async fn resolve_output(
    selection: &PlaybackOutputSelection,
    state: &AppState,
) -> Result<ResolvedOutputPlan, PlaybackError> {
    match selection {
        PlaybackOutputSelection::Receiver { id } => {
            let reg_arc = state.receiver_manager.registry().await;
            let reg = reg_arc.read().await;
            let entry = reg
                .get(id)
                .ok_or_else(|| PlaybackError::ReceiverNotPaired(id.clone()))?;

            if !entry.paired {
                return Err(PlaybackError::ReceiverNotPaired(id.clone()));
            }

            let name = entry.name.clone();
            drop(reg);

            let sink = Box::new(ReceiverAudioSink::new(
                id.clone(),
                state.receiver_manager.clone(),
            ));

            Ok(ResolvedOutputPlan {
                sinks: vec![sink],
                description: PlaybackOutputDescription {
                    target_id: id.clone(),
                    target_name: name,
                    kind: "receiver".to_string(),
                    receiver_count: 1,
                },
            })
        }
        PlaybackOutputSelection::RoomGroup { id } => {
            let groups = michi_db::list_room_groups_db(&state.db)
                .await
                .map_err(PlaybackError::Database)?;

            let found = groups.into_iter().find(|(gid, _, _, _, _, _)| gid == id);

            let (group_id, group_name, _mode, receiver_ids, volumes, _created_at) =
                found.ok_or_else(|| PlaybackError::OutputNotFound(id.to_string()))?;

            if receiver_ids.is_empty() {
                return Err(PlaybackError::OutputUnavailable(
                    "room group has no configured receivers".to_string(),
                ));
            }

            let reg_arc = state.receiver_manager.registry().await;
            let reg = reg_arc.read().await;

            let mut sinks: Vec<Box<dyn AudioSink>> = Vec::new();
            for r_id in &receiver_ids {
                if let Some(entry) = reg.get(r_id) {
                    if entry.paired {
                        let vol = volumes.get(r_id).copied().unwrap_or(80) as u8;
                        sinks.push(Box::new(ReceiverAudioSink::new_with_config(
                            r_id.clone(),
                            state.receiver_manager.clone(),
                            vol,
                            false,
                        )));
                    }
                }
            }
            drop(reg);

            if sinks.is_empty() {
                return Err(PlaybackError::OutputUnavailable(
                    "none of the receivers in the room group are paired or available".to_string(),
                ));
            }

            let sink_count = sinks.len();
            Ok(ResolvedOutputPlan {
                sinks,
                description: PlaybackOutputDescription {
                    target_id: group_id.to_string(),
                    target_name: group_name,
                    kind: "room_group".to_string(),
                    receiver_count: sink_count,
                },
            })
        }
        PlaybackOutputSelection::Chain { id } => {
            let chain_opt = michi_db::get_chain_with_links(&state.db, id)
                .await
                .map_err(PlaybackError::Database)?;

            let (chain, links) =
                chain_opt.ok_or_else(|| PlaybackError::OutputNotFound(id.to_string()))?;

            if links.is_empty() {
                return Err(PlaybackError::OutputUnavailable(
                    "playback chain has no links configured".to_string(),
                ));
            }

            let reg_arc = state.receiver_manager.registry().await;
            let reg = reg_arc.read().await;

            let mut sinks: Vec<Box<dyn AudioSink>> = Vec::new();
            for link in &links {
                let r_id = link.receiver_id.to_string();
                if let Some(entry) = reg.get(&r_id) {
                    if entry.paired {
                        let vol = link.volume.clamp(0, 100) as u8;
                        sinks.push(Box::new(ReceiverAudioSink::new_with_config(
                            r_id,
                            state.receiver_manager.clone(),
                            vol,
                            link.muted,
                        )));
                    }
                }
            }
            drop(reg);

            if sinks.is_empty() {
                return Err(PlaybackError::OutputUnavailable(
                    "none of the receivers in the chain are paired or available".to_string(),
                ));
            }

            let sink_count = sinks.len();
            Ok(ResolvedOutputPlan {
                sinks,
                description: PlaybackOutputDescription {
                    target_id: id.to_string(),
                    target_name: chain.name,
                    kind: "chain".to_string(),
                    receiver_count: sink_count,
                },
            })
        }
    }
}
