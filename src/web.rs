use std::sync::Arc;

use axum::{extract::State, response::Html, Json};
use serde::{Deserialize, Serialize};

use crate::{pipeline::MacVideoPipeline, webrtc_sender::accept_offer};

#[derive(Clone)]
pub struct AppState {
    pub pipeline: Arc<MacVideoPipeline>,
}

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
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

pub async fn index_handler() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn health_handler(State(state): State<AppState>) -> Json<HealthResponse> {
    let health = state.pipeline.health();
    Json(HealthResponse {
        status: "ok",
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
) -> Result<Json<SessionAnswer>, (axum::http::StatusCode, String)> {
    let (encoded_rx, initial_access_unit) = state.pipeline.subscribe();
    let sdp = accept_offer(encoded_rx, initial_access_unit, offer.sdp)
        .await
        .map_err(|err| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(SessionAnswer { sdp }))
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
