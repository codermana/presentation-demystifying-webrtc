use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use tokio::sync::broadcast;

use crate::h264::{parse_annex_b_access_units, EncodedAccessUnit};

use super::{env_i32, env_path, now_unix_ms, PipelineStats, DEFAULT_TARGET_FPS};

pub(crate) struct FileH264Config {
    path: PathBuf,
    fps: i32,
    loop_forever: bool,
}

impl FileH264Config {
    pub(crate) fn from_env() -> Result<Self, String> {
        let path = env_path("POC_VIDEO_FILE")
            .ok_or_else(|| "POC_VIDEO_FILE is required when POC_SOURCE=file".to_string())?;
        let fps = env_i32("POC_FPS", DEFAULT_TARGET_FPS).max(1);
        let loop_forever = std::env::var("POC_FILE_LOOP")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
            .unwrap_or(true);

        if !path.exists() {
            return Err(format!("POC_VIDEO_FILE does not exist: {}", path.display()));
        }

        Ok(Self {
            path,
            fps,
            loop_forever,
        })
    }
}

pub(crate) fn spawn(
    config: FileH264Config,
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
) {
    thread::spawn(move || {
        if let Err(err) = run(
            config,
            encoded_tx,
            Arc::clone(&stats),
            Arc::clone(&latest_access_unit),
        ) {
            stats
                .pipeline_ready
                .store(false, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut last_error) = stats.last_error.lock() {
                *last_error = Some(err);
            }
        }
    });
}

fn run(
    config: FileH264Config,
    encoded_tx: broadcast::Sender<EncodedAccessUnit>,
    stats: Arc<PipelineStats>,
    latest_access_unit: Arc<Mutex<Option<EncodedAccessUnit>>>,
) -> Result<(), String> {
    let bytes = fs::read(&config.path)
        .map_err(|err| format!("failed to read {}: {err}", config.path.display()))?;
    let frame_duration = Duration::from_millis((1000 / config.fps.max(1)) as u64);
    let access_units = parse_annex_b_access_units(&bytes, frame_duration)?;
    if access_units.is_empty() {
        return Err(format!(
            "no access units found in {}; expected an Annex-B H264 elementary stream",
            config.path.display()
        ));
    }

    stats
        .pipeline_ready
        .store(true, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut last_error) = stats.last_error.lock() {
        *last_error = None;
    }

    loop {
        for access_unit in &access_units {
            let access_unit = access_unit.clone();
            if let Ok(mut latest) = latest_access_unit.lock() {
                *latest = Some(access_unit.clone());
            }
            let _ = encoded_tx.send(access_unit);
            stats
                .encoded_frames
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            stats
                .last_encoded_at_ms
                .store(now_unix_ms(), std::sync::atomic::Ordering::Relaxed);
            thread::sleep(frame_duration);
        }

        if !config.loop_forever {
            return Ok(());
        }
    }
}
