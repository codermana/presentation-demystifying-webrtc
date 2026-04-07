mod h264;
mod pipeline;
mod web;
mod webrtc_sender;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    routing::{get, post},
    Router,
};
use pipeline::MacVideoPipeline;
use tokio::net::TcpListener;
use web::{create_session_handler, health_handler, index_handler, AppState};

#[tokio::main]
async fn main() -> Result<(), String> {
    let pipeline = Arc::new(MacVideoPipeline::start_default()?);
    let state = AppState { pipeline };
    let bind_addr: SocketAddr = "0.0.0.0:4060"
        .parse()
        .map_err(|err| format!("invalid bind addr: {err}"))?;

    let app = Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/session", post(create_session_handler))
        .with_state(state);

    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|err| format!("bind failed: {err}"))?;

    println!("macos-webrtc-h264 listening on http://{bind_addr}");
    axum::serve(listener, app)
        .await
        .map_err(|err| format!("server failed: {err}"))
}
