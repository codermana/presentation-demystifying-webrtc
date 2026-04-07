use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;

use crate::{pipeline::MediaPipeline, webrtc_sender::accept_offer};

#[derive(Clone)]
pub struct AppState {
    pub pipeline: Arc<MediaPipeline>,
}

pub struct ServerConfig {
    pub bind_addr: std::net::SocketAddr,
}

impl ServerConfig {
    pub fn from_env() -> Result<Self, String> {
        let bind_addr = std::env::var("POC_SERVER_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:4060".to_string())
            .parse()
            .map_err(|err| format!("invalid bind addr: {err}"))?;
        Ok(Self { bind_addr })
    }
}

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    source: &'static str,
    pipeline_ready: bool,
    raw_frames: u64,
    encode_attempts: u64,
    encoded_frames: u64,
    dropped_frames: u64,
    last_raw_frame_at_ms: u64,
    last_encode_attempt_at_ms: u64,
    last_encoded_at_ms: u64,
    last_error: Option<String>,
}

#[derive(Deserialize)]
pub struct SessionOffer {
    pub sdp: String,
}

#[derive(Serialize)]
pub struct SessionAnswer {
    pub sdp: String,
}

pub async fn run_server(config: ServerConfig, state: AppState) -> Result<(), String> {
    let listener = TcpListener::bind(config.bind_addr)
        .await
        .map_err(|err| format!("bind failed: {err}"))?;
    let app = router(state);

    println!("macos-webrtc-h264 listening on http://{}", config.bind_addr);
    axum::serve(listener, app)
        .await
        .map_err(|err| format!("server failed: {err}"))
}

fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/health", get(health_handler))
        .route("/session", post(create_session_handler))
        .with_state(state)
}

pub async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let health = state.pipeline.health();
    Json(HealthResponse {
        status: "ok",
        source: health.source,
        pipeline_ready: health.pipeline_ready,
        raw_frames: health.raw_frames,
        encode_attempts: health.encode_attempts,
        encoded_frames: health.encoded_frames,
        dropped_frames: health.dropped_frames,
        last_raw_frame_at_ms: health.last_raw_frame_at_ms,
        last_encode_attempt_at_ms: health.last_encode_attempt_at_ms,
        last_encoded_at_ms: health.last_encoded_at_ms,
        last_error: health.last_error,
    })
}

pub async fn create_session_handler(
    State(state): State<AppState>,
    Json(offer): Json<SessionOffer>,
) -> Result<Json<SessionAnswer>, AppError> {
    let (encoded_rx, initial_access_unit) = state.pipeline.subscribe();
    let sdp = accept_offer(encoded_rx, initial_access_unit, offer.sdp)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(SessionAnswer { sdp }))
}

pub struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn internal(message: String) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.status, self.message).into_response()
    }
}

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>macOS WebRTC H264 POC</title>
  <style>
    html, body {
      margin: 0;
      width: 100%;
      height: 100%;
      background: #09101b;
      color: #d8e2f2;
      font-family: "Avenir Next", "Segoe UI", sans-serif;
    }
    body {
      display: grid;
      grid-template-rows: auto 1fr;
    }
    header {
      display: flex;
      gap: 16px;
      align-items: center;
      padding: 14px 18px;
      border-bottom: 1px solid rgba(255,255,255,0.08);
      background: rgba(7, 12, 22, 0.92);
      backdrop-filter: blur(12px);
    }
    .pill {
      border: 1px solid rgba(255,255,255,0.14);
      border-radius: 999px;
      padding: 6px 10px;
      font-size: 12px;
    }
    video {
      width: 100%;
      height: 100%;
      object-fit: contain;
      background: #02060d;
    }
  </style>
</head>
<body>
  <header>
    <strong>macOS WebRTC H264 POC</strong>
    <span class="pill" id="status">connecting</span>
  </header>
  <video id="viewer" autoplay playsinline muted></video>
  <script>
    const statusEl = document.getElementById("status");
    const videoEl = document.getElementById("viewer");

    async function waitForIceGatheringComplete(pc) {
      if (pc.iceGatheringState === "complete") {
        return;
      }
      await new Promise((resolve) => {
        const onStateChange = () => {
          if (pc.iceGatheringState === "complete") {
            pc.removeEventListener("icegatheringstatechange", onStateChange);
            resolve();
          }
        };
        pc.addEventListener("icegatheringstatechange", onStateChange);
      });
    }

    async function connect() {
      statusEl.textContent = "negotiating";
      const pc = new RTCPeerConnection({ iceServers: [] });
      pc.addTransceiver("video", { direction: "recvonly" });

      pc.ontrack = (event) => {
        const [stream] = event.streams;
        if (stream) {
          videoEl.srcObject = stream;
          statusEl.textContent = "playing";
        }
      };

      const offer = await pc.createOffer();
      await pc.setLocalDescription(offer);
      await waitForIceGatheringComplete(pc);

      const response = await fetch("/session", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ sdp: pc.localDescription.sdp }),
      });
      if (!response.ok) {
        statusEl.textContent = "server error";
        throw new Error(await response.text());
      }

      const answer = await response.json();
      await pc.setRemoteDescription({ type: "answer", sdp: answer.sdp });
      statusEl.textContent = "connected";
    }

    connect().catch((error) => {
      console.error(error);
      statusEl.textContent = "failed";
    });
  </script>
</body>
</html>
"##;
