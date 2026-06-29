use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum Permission {
    #[serde(rename = "server.read")]
    ServerRead,
    #[serde(rename = "library.read")]
    LibraryRead,
    #[serde(rename = "library.write")]
    LibraryWrite,
    #[serde(rename = "stream.read")]
    StreamRead,
    #[serde(rename = "download.read")]
    DownloadRead,
    #[serde(rename = "artwork.read")]
    ArtworkRead,
    #[serde(rename = "playlist.read")]
    PlaylistRead,
    #[serde(rename = "playlist.write")]
    PlaylistWrite,
    #[serde(rename = "sync.read_manifest")]
    SyncReadManifest,
    #[serde(rename = "sync.upload_state")]
    SyncUploadState,
    #[serde(rename = "playback.read")]
    PlaybackRead,
    #[serde(rename = "playback.control")]
    PlaybackControl,
    #[serde(rename = "queue.read")]
    QueueRead,
    #[serde(rename = "queue.write")]
    QueueWrite,
    #[serde(rename = "receiver.read")]
    ReceiverRead,
    #[serde(rename = "receiver.control")]
    ReceiverControl,
    #[serde(rename = "room.read")]
    RoomRead,
    #[serde(rename = "room.write")]
    RoomWrite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevicePermissions {
    pub permissions: HashSet<Permission>,
}

impl DevicePermissions {
    pub fn has(&self, perm: &Permission) -> bool {
        self.permissions.contains(perm)
    }

    pub fn has_all(&self, perms: &[Permission]) -> bool {
        perms.iter().all(|p| self.permissions.contains(p))
    }

    pub fn player() -> Self {
        Self {
            permissions: HashSet::from([
                Permission::ServerRead,
                Permission::LibraryRead,
                Permission::LibraryWrite,
                Permission::StreamRead,
                Permission::DownloadRead,
                Permission::ArtworkRead,
                Permission::PlaylistRead,
                Permission::PlaylistWrite,
                Permission::SyncReadManifest,
                Permission::SyncUploadState,
                Permission::PlaybackRead,
                Permission::PlaybackControl,
                Permission::QueueRead,
                Permission::QueueWrite,
                Permission::ReceiverRead,
                Permission::ReceiverControl,
                Permission::RoomRead,
                Permission::RoomWrite,
            ]),
        }
    }

    pub fn mobile() -> Self {
        Self {
            permissions: HashSet::from([
                Permission::LibraryRead,
                Permission::StreamRead,
                Permission::DownloadRead,
                Permission::ArtworkRead,
                Permission::PlaylistRead,
                Permission::SyncReadManifest,
                Permission::SyncUploadState,
                Permission::PlaybackRead,
                Permission::PlaybackControl,
                Permission::QueueRead,
                Permission::QueueWrite,
            ]),
        }
    }

    pub fn stream_receiver() -> Self {
        Self {
            permissions: HashSet::from([
                Permission::ServerRead,
                Permission::StreamRead,
                Permission::PlaybackRead,
                Permission::ReceiverRead,
                Permission::RoomRead,
            ]),
        }
    }
}

impl Default for DevicePermissions {
    fn default() -> Self {
        Self {
            permissions: HashSet::from([Permission::ServerRead]),
        }
    }
}
