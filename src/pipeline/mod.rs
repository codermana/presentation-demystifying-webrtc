use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
};

use tokio::sync::broadcast;

use crate::h264::EncodedAccessUnit;

mod file_h264;
mod macos_screen;

const STARTUP_CHANNEL_CAPACITY: usize = 2;
const DEFAULT_TARGET_FPS: i32 = 15;
const DEFAULT_MAX_STREAM_WIDTH: i32 = 1280;
const DEFAULT_TARGET_AVERAGE_BITRATE_BPS: i32 = 5_000_000;

#[derive(Clone)]
pub struct MediaPipeline {
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
    source_name: &'static str,
}

#[derive(Default)]
pub(crate) struct PipelineStats {
    pub(crate) pipeline_ready: AtomicBool,
    pub(crate) raw_frames: AtomicU64,
    pub(crate) encode_attempts: AtomicU64,
    pub(crate) encoded_frames: AtomicU64,
    pub(crate) dropped_frames: AtomicU64,
    pub(crate) last_raw_frame_at_ms: AtomicU64,
    pub(crate) last_encode_attempt_at_ms: AtomicU64,
    pub(crate) last_encoded_at_ms: AtomicU64,
    pub(crate) last_error: Mutex<Option<String>>,
}

pub struct PipelineHealth {
    pub source: &'static str,
    pub pipeline_ready: bool,
    pub raw_frames: u64,
    pub encode_attempts: u64,
    pub encoded_frames: u64,
    pub dropped_frames: u64,
    pub last_raw_frame_at_ms: u64,
    pub last_encode_attempt_at_ms: u64,
    pub last_encoded_at_ms: u64,
    pub last_error: Option<String>,
}

pub struct PipelineConfig {
    source: SourceConfig,
}

enum SourceConfig {
    MacosScreen(macos_screen::MacosScreenConfig),
    FileH264(file_h264::FileH264Config),
    Camera,
    CameraAudio,
    ScreenAudio,
}

impl PipelineConfig {
    fn from_env() -> Result<Self, String> {
        let source = std::env::var("POC_SOURCE").unwrap_or_else(|_| "screen".to_string());
        let source = match source.as_str() {
            "screen" => SourceConfig::MacosScreen(macos_screen::MacosScreenConfig::from_env()),
            "file" => SourceConfig::FileH264(file_h264::FileH264Config::from_env()?),
            "camera" => SourceConfig::Camera,
            "camera-audio" => SourceConfig::CameraAudio,
            "screen-audio" => SourceConfig::ScreenAudio,
            other => {
                return Err(format!(
                    "unsupported POC_SOURCE `{other}`; expected one of screen, file, camera, camera-audio, screen-audio"
                ))
            }
        };
        Ok(Self { source })
    }
}

impl MediaPipeline {
    pub fn start_default() -> Result<Self, String> {
        Self::start(PipelineConfig::from_env()?)
    }

    pub fn start(config: PipelineConfig) -> Result<Self, String> {
        let (encoded_tx, _) = broadcast::channel::<EncodedAccessUnit>(STARTUP_CHANNEL_CAPACITY);
        let stats = Arc::new(PipelineStats::default());
        let latest_access_unit = Arc::new(Mutex::new(None));

        let source_name = match &config.source {
            SourceConfig::MacosScreen(_) => "screen",
            SourceConfig::FileH264(_) => "file",
            SourceConfig::Camera => "camera",
            SourceConfig::CameraAudio => "camera-audio",
            SourceConfig::ScreenAudio => "screen-audio",
        };

        match config.source {
            SourceConfig::MacosScreen(source_config) => macos_screen::spawn(
                source_config,
                encoded_tx.clone(),
                Arc::clone(&stats),
                Arc::clone(&latest_access_unit),
            ),
            SourceConfig::FileH264(source_config) => file_h264::spawn(
                source_config,
                encoded_tx.clone(),
                Arc::clone(&stats),
                Arc::clone(&latest_access_unit),
            ),
            SourceConfig::Camera => {
                return Err(
                    "POC_SOURCE=camera is not implemented yet; add an AVFoundation capture source module first"
                        .to_string(),
                )
            }
            SourceConfig::CameraAudio => {
                return Err(
                    "POC_SOURCE=camera-audio is not implemented yet; the server currently publishes H264 video only"
                        .to_string(),
                )
            }
            SourceConfig::ScreenAudio => {
                return Err(
                    "POC_SOURCE=screen-audio is not implemented yet; the server currently publishes H264 video only"
                        .to_string(),
                )
            }
        }

        Ok(Self {
            encoded_tx,
            stats,
            latest_access_unit,
            source_name,
        })
    }

    pub fn subscribe(
        &self,
    ) -> (
        broadcast::Receiver<EncodedAccessUnit>,
        Option<EncodedAccessUnit>,
    ) {
        let snapshot = self
            .latest_access_unit
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_else(|poisoned| poisoned.into_inner().clone());
        (self.encoded_tx.subscribe(), snapshot)
    }

    pub fn health(&self) -> PipelineHealth {
        PipelineHealth {
            source: self.source_name,
            pipeline_ready: self.stats.pipeline_ready.load(Ordering::Relaxed),
            raw_frames: self.stats.raw_frames.load(Ordering::Relaxed),
            encode_attempts: self.stats.encode_attempts.load(Ordering::Relaxed),
            encoded_frames: self.stats.encoded_frames.load(Ordering::Relaxed),
            dropped_frames: self.stats.dropped_frames.load(Ordering::Relaxed),
            last_raw_frame_at_ms: self.stats.last_raw_frame_at_ms.load(Ordering::Relaxed),
            last_encode_attempt_at_ms: self.stats.last_encode_attempt_at_ms.load(Ordering::Relaxed),
            last_encoded_at_ms: self.stats.last_encoded_at_ms.load(Ordering::Relaxed),
            last_error: self
                .stats
                .last_error
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_else(|poisoned| poisoned.into_inner().clone()),
        }
    }
}

pub(crate) fn env_i32(name: &str, default: i32) -> i32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i32>().ok())
        .unwrap_or(default)
}

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

pub(crate) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
