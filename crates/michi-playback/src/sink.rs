use async_trait::async_trait;

use crate::error::PlaybackError;
use crate::model::{PcmFormat, SinkKind, SinkSnapshot};

#[async_trait]
pub trait AudioSink: Send + Sync {
    fn id(&self) -> &str;
    fn kind(&self) -> SinkKind;

    async fn prepare(&mut self, format: PcmFormat) -> Result<(), PlaybackError>;

    async fn write_pcm(&mut self, data: &[u8]) -> Result<usize, PlaybackError>;

    async fn pause(&mut self) -> Result<(), PlaybackError>;

    async fn resume(&mut self) -> Result<(), PlaybackError>;

    async fn set_volume(&mut self, volume: u8) -> Result<(), PlaybackError>;

    async fn health(&self) -> Result<(), PlaybackError>;

    async fn stop(&mut self) -> Result<(), PlaybackError>;

    fn snapshot(&self) -> SinkSnapshot;
}
