use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{debug, error, warn};

use crate::error::PlaybackError;
use crate::model::PcmFormat;

pub struct FfmpegPcmDecoder {
    file_path: String,
    format: PcmFormat,
    child: Option<Child>,
    stdout: Option<ChildStdout>,
    last_error: Arc<Mutex<Option<String>>>,
    bytes_decoded: u64,
    eof: bool,
}

impl FfmpegPcmDecoder {
    pub async fn check_available() -> bool {
        match Command::new("ffmpeg")
            .arg("-version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
        {
            Ok(status) => status.success(),
            Err(_) => false,
        }
    }

    pub fn new(file_path: String, format: PcmFormat) -> Self {
        Self {
            file_path,
            format,
            child: None,
            stdout: None,
            last_error: Arc::new(Mutex::new(None)),
            bytes_decoded: 0,
            eof: false,
        }
    }

    pub async fn start(&mut self, start_position_ms: u64) -> Result<(), PlaybackError> {
        self.stop().await?;

        if !Path::new(&self.file_path).exists() {
            return Err(PlaybackError::TrackFileMissing(self.file_path.clone()));
        }

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-nostdin");

        if start_position_ms > 0 {
            let secs = start_position_ms as f64 / 1000.0;
            cmd.arg("-ss").arg(format!("{secs:.3}"));
        }

        cmd.arg("-i")
            .arg(&self.file_path)
            .arg("-vn")
            .arg("-sn")
            .arg("-dn")
            .arg("-f")
            .arg("s16le")
            .arg("-acodec")
            .arg("pcm_s16le")
            .arg("-ar")
            .arg(format!("{}", self.format.sample_rate))
            .arg("-ac")
            .arg(format!("{}", self.format.channels))
            .arg("pipe:1");

        cmd.stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            PlaybackError::DecoderUnavailable(format!("failed to spawn ffmpeg: {e}"))
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            PlaybackError::DecoderFailed("failed to capture ffmpeg stdout pipe".to_string())
        })?;

        let stderr = child.stderr.take();
        let last_error = self.last_error.clone();

        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if !line.trim().is_empty() {
                        warn!("ffmpeg stderr: {}", line);
                        *last_error.lock().await = Some(line);
                    }
                }
            });
        }

        self.child = Some(child);
        self.stdout = Some(stdout);
        self.eof = false;
        debug!(
            "ffmpeg decoder started for '{}' at position {}ms",
            self.file_path, start_position_ms
        );

        Ok(())
    }

    pub async fn read_pcm(&mut self, buf: &mut [u8]) -> Result<usize, PlaybackError> {
        if self.eof {
            return Ok(0);
        }

        let stdout = match self.stdout.as_mut() {
            Some(s) => s,
            None => return Err(PlaybackError::DecoderFailed("decoder not started".to_string())),
        };

        match stdout.read(buf).await {
            Ok(0) => {
                self.eof = true;
                if let Some(mut child) = self.child.take() {
                    let _ = child.wait().await;
                }
                self.stdout = None;
                Ok(0)
            }
            Ok(n) => {
                self.bytes_decoded += n as u64;
                Ok(n)
            }
            Err(e) => {
                let err_msg = self
                    .last_error
                    .lock()
                    .await
                    .clone()
                    .unwrap_or_else(|| e.to_string());
                error!("ffmpeg decoder read error: {}", err_msg);
                Err(PlaybackError::DecoderFailed(err_msg))
            }
        }
    }

    pub async fn stop(&mut self) -> Result<(), PlaybackError> {
        self.stdout = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        self.eof = false;
        Ok(())
    }

    pub fn bytes_decoded(&self) -> u64 {
        self.bytes_decoded
    }

    pub fn is_eof(&self) -> bool {
        self.eof
    }
}
