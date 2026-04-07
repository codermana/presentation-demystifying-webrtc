# Architecture

## Overview

This document covers only the Rust WebRTC demo server under `src/`.

The server is intentionally small. It does three jobs:

1. Serve a minimal HTML viewer and a health endpoint over HTTP
2. Produce encoded H264 access units from a selectable media source
3. Accept a browser SDP offer and answer with a single H264 video track

## Runtime Flow

At startup, [src/main.rs](/Users/gaurav/Developer/Presentations/webrtc/src/main.rs) constructs a `MediaPipeline`, builds `AppState`, loads server bind configuration, and starts the Axum server.

The browser loads `/`, creates an `RTCPeerConnection`, adds a recvonly video transceiver, creates an SDP offer, and posts it to `/session`.

The `/session` handler subscribes to the pipeline’s encoded frame broadcast channel and passes that receiver into the WebRTC sender. The sender creates a peer connection, attaches a local static sample track configured for H264, applies the remote offer, generates an answer, and starts forwarding encoded access units into the track.

## Module Map

### `src/main.rs`

The entrypoint is deliberately thin. Its only responsibility is wiring together pipeline startup and HTTP server startup.

### `src/web.rs`

This module owns the HTTP boundary:

* `ServerConfig` parses `POC_SERVER_ADDR`
* `run_server` binds a `TcpListener` and starts Axum
* `router` defines `/`, `/health`, and `/session`
* `AppState` carries the shared `MediaPipeline`
* `AppError` standardizes handler error responses

The inline HTML page is also embedded here. It performs the offer/answer exchange and plays the returned remote video stream.

### `src/webrtc_sender.rs`

This module owns WebRTC signaling and media publication. `accept_offer` is the public entrypoint used by the web layer.

`WebRtcSession` bundles:

* The `RTCPeerConnection`
* The local H264 `TrackLocalStaticSample`
* The broadcast receiver that yields encoded access units
* The last access unit snapshot used for periodic resend

Important behavior:

* The sender only publishes one video track
* No STUN or TURN servers are configured
* A resend timer pushes the latest access unit every 250ms so a new viewer can converge even if the source is currently idle
* The sender drains RTCP in a background task so the underlying library keeps the sender healthy

### `src/h264.rs`

This module handles H264 framing conversions.

It currently supports two related tasks:

* Convert VideoToolbox AVCC sample buffers into Annex-B access units with SPS/PPS prepended
* Parse an Annex-B H264 elementary stream from disk into a sequence of access units for replay

The file parser is intentionally narrow. It expects raw H264 bytes, not `.mp4` or `.mov` containers.

### `src/pipeline/mod.rs`

This is the abstraction boundary for media production.

`MediaPipeline` exposes:

* `start_default()` to build a pipeline from environment configuration
* `subscribe()` to obtain a broadcast receiver plus the latest cached frame
* `health()` to expose runtime counters and the active source name

Internally the module owns:

* The shared broadcast sender of `EncodedAccessUnit`
* `PipelineStats`
* The latest encoded access unit snapshot
* Source selection through `POC_SOURCE`

Supported `POC_SOURCE` values:

* `screen`
* `file`

Reserved but not implemented yet:

* `camera`
* `camera-audio`
* `screen-audio`

Those modes currently return explicit startup errors because the server still publishes video-only H264 and there is no AVFoundation capture module yet.

### `src/pipeline/macos_screen.rs`

This module contains the native macOS screen streaming path that used to live in the monolithic pipeline file.

Responsibilities:

* Query displays through ScreenCaptureKit
* Create and start an `SCStream`
* Receive raw screen sample buffers on a serial dispatch queue
* Feed those frames into a VideoToolbox H264 encoder
* Convert encoder output into `EncodedAccessUnit` values and broadcast them

This is the most platform-specific part of the codebase. It depends heavily on `objc2`, `ScreenCaptureKit`, and `VideoToolbox`.

### `src/pipeline/file_h264.rs`

This module replays a saved H264 elementary stream as if it were a live source.

Responsibilities:

* Read the file from `POC_VIDEO_FILE`
* Parse it into access units
* Send those access units over the broadcast channel at a configured frame rate
* Optionally loop forever via `POC_FILE_LOOP`

This backend is useful for testing WebRTC transport without screen capture permissions or macOS-specific capture behavior.

## Concurrency Model

There are three different concurrency domains in the current design:

* Tokio async tasks for HTTP serving and WebRTC session handling
* Standard threads for media source execution
* Apple callback-driven APIs for native screen capture and encoder output

The handoff between the source layer and the WebRTC layer is a Tokio `broadcast::Sender<EncodedAccessUnit>`. Each viewer gets its own receiver by calling `MediaPipeline::subscribe()`.

The pipeline also caches the latest access unit separately so a new session can immediately send the most recent frame before waiting for the next source event.

## Configuration

The main runtime knobs are environment variables.

Server:

* `POC_SERVER_ADDR`: HTTP bind address, default `0.0.0.0:4060`

Pipeline:

* `POC_SOURCE`: `screen` or `file`
* `POC_FPS`: target pacing for screen capture and file replay

Screen source:

* `POC_DISPLAY_ID`: preferred display ID, default first available display
* `POC_MAX_WIDTH`: scale-down ceiling before encoding
* `POC_TARGET_BITRATE_BPS`: VideoToolbox target bitrate

File source:

* `POC_VIDEO_FILE`: path to an Annex-B H264 elementary stream
* `POC_FILE_LOOP`: whether replay loops, defaults to true

## Health and Observability

`/health` exposes counters gathered in `PipelineStats`, including:

* Whether the pipeline is ready
* The active source name
* Raw frames seen
* Encode attempts
* Encoded frames sent
* Dropped frames
* Timestamps for the last raw frame, encode attempt, and encoded frame
* The last pipeline error string

These counters are source-agnostic enough to work for both the screen and file backends, though some are more meaningful for live capture than file replay.

## Current Limitations

The current architecture is intentionally narrow:

* Only one H264 video track is published
* Audio is not encoded or sent over WebRTC
* `file` mode only supports raw Annex-B H264 elementary streams
* `camera`, `camera-audio`, and `screen-audio` are planned extension points, not complete features
* The embedded HTML viewer is a minimal debugging surface, not a general signaling client

## Likely Next Extensions

If the codebase is extended further, the natural next steps are:

* Add an AVFoundation camera source module alongside `macos_screen` and `file_h264`
* Introduce an audio pipeline and publish an audio track from `webrtc_sender.rs`
* Support container demuxing for `.mp4` or `.mov` file playback rather than requiring raw H264
* Move the inline viewer HTML out of `src/web.rs` if the browser UI becomes more complex
