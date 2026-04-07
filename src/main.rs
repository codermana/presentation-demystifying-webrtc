mod h264;
mod pipeline;
mod web;
mod webrtc_sender;

use std::sync::Arc;

use pipeline::MediaPipeline;
use web::{run_server, AppState, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), String> {
    let pipeline = Arc::new(MediaPipeline::start_default()?);
    let state = AppState { pipeline };
    let config = ServerConfig::from_env()?;
    run_server(config, state).await
}
