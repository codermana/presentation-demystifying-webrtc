# Architecture

## Scope

This document covers only the Rust code under `src/`. It explains how the server starts, how media is produced, how encoded H264 reaches the browser, and where the current extension points and limitations are.

## System Intent

The Rust server is a compact WebRTC publisher. It is not a general conferencing server, SFU, or media router. The design goal is much narrower:

1. Produce a stream of H264 access units from some source
2. Expose a tiny HTTP signaling surface
3. Negotiate a single browser WebRTC session
4. Push the H264 stream into that session as a video track

Everything in the codebase supports one of those four steps.

## High-Level Architecture

At a high level, the Rust code is split into four layers:

* Bootstrap and process startup in `main.rs`
* HTTP and signaling boundary in `web.rs`
* WebRTC session setup and track publication in `webrtc_sender.rs`
* Media production and encoding in `pipeline/` plus H264 helpers in `h264.rs`

The main data type that connects the media side to the WebRTC side is `EncodedAccessUnit` from [src/h264.rs](/Users/gaurav/Developer/Presentations/webrtc/src/h264.rs). That struct is the contract between “something that can produce H264” and “something that can publish H264 over WebRTC”.

## End-to-End Runtime Flow

### 1. Process startup

[src/main.rs](/Users/gaurav/Developer/Presentations/webrtc/src/main.rs) is intentionally thin. On startup it:

* Builds a `MediaPipeline` from environment configuration
* Wraps that pipeline in `Arc` so request handlers can share it
* Loads `ServerConfig`
* Hands control to the Axum server

This keeps process bootstrap separate from protocol logic.

### 2. Media pipeline startup

`MediaPipeline::start_default()` in [src/pipeline/mod.rs](/Users/gaurav/Developer/Presentations/webrtc/src/pipeline/mod.rs) reads `POC_SOURCE` and chooses one source backend.

Today the real backends are:

* `screen`: live macOS screen capture through ScreenCaptureKit and VideoToolbox
* `file`: replay of a saved Annex-B H264 elementary stream

The pipeline allocates shared state before spawning the backend:

* A Tokio broadcast channel for encoded frames
* Shared pipeline statistics
* A cached copy of the latest access unit

The backend then runs independently in its own thread and publishes `EncodedAccessUnit` values into the broadcast channel.

### 3. Browser loads the viewer page

The `/` route in [src/web.rs](/Users/gaurav/Developer/Presentations/webrtc/src/web.rs) serves a minimal HTML page embedded directly in Rust source. That page:

* Creates an `RTCPeerConnection`
* Adds one recvonly video transceiver
* Builds an SDP offer
* Waits for local ICE gathering to complete
* POSTs the offer SDP to `/session`
* Applies the returned answer SDP
* Attaches the remote track to a `<video>` element

This page is intentionally minimal and acts as a built-in integration client.

### 4. HTTP signaling request arrives

The `/session` handler in `web.rs` performs the bridge from HTTP into WebRTC. It does not negotiate media itself. Instead it:

* Calls `pipeline.subscribe()`
* Receives a fresh broadcast receiver for future frames
* Receives an optional snapshot of the latest encoded access unit
* Passes all of that into `accept_offer` in `webrtc_sender.rs`

That separation matters because the handler does not know or care where the video came from. It only depends on the generic encoded-frame interface.

### 5. WebRTC answer generation

`accept_offer` in [src/webrtc_sender.rs](/Users/gaurav/Developer/Presentations/webrtc/src/webrtc_sender.rs) constructs a `WebRtcSession`, sets the remote description from the browser offer, creates an answer, waits for ICE gathering, and returns the resulting answer SDP.

At this point, the browser and server have a negotiated single-track video session.

### 6. Encoded frame forwarding

Once negotiation has completed, `WebRtcSession` starts a background forwarder task that reads `EncodedAccessUnit` values from the broadcast receiver and writes them into a `TrackLocalStaticSample`.

That task does two things in parallel:

* Forward newly received access units
* Periodically resend the latest cached access unit every 250ms

The resend behavior is a pragmatic choice. It helps a newly connected viewer recover even if the source is quiet or if a key access unit is not immediately followed by fresh frames.

## Module-by-Module Explanation

## `src/main.rs`

This file is pure composition logic. It contains almost no policy.

Why it is small:

* Configuration logic belongs with the subsystem that uses it
* HTTP route definitions belong in the web layer
* Media source selection belongs in the pipeline layer

That makes `main.rs` a stable assembly point rather than a file that accumulates cross-cutting logic.

## `src/web.rs`

This module is the HTTP boundary and the only place that knows about Axum-specific request and response types.

### Main responsibilities

* Parse server bind configuration
* Construct the router
* Hold shared application state
* Return the embedded viewer page
* Expose health data as JSON
* Accept session offers and return session answers

### `AppState`

`AppState` is currently just an `Arc<MediaPipeline>`. This is deliberately narrow. The web layer depends on the pipeline abstraction, not on any screen-capture or file-playback implementation details.

### `run_server`

`run_server` binds the configured socket and starts Axum. There is no additional lifecycle manager or supervisor layer right now, so if binding fails or Axum returns an error, the process exits with a string error.

### `health_handler`

`/health` simply projects `PipelineHealth` into JSON. It does not attempt to compute derived metrics or guess liveness. The source modules own the counters; the web layer only serializes them.

### `create_session_handler`

This route is the narrow signaling API. The request body contains only an SDP string. The server response contains only an SDP string. There is no trickle ICE API, websocket signaling channel, authentication, or session registry.

That simplicity matches the current deployment model: one process, one viewer page, one direct offer/answer exchange.

### Embedded HTML

Keeping the viewer page inline is a tradeoff:

* Advantage: no asset pipeline or templating system is needed
* Cost: the web module mixes transport logic with a chunk of static UI

For a proof of concept this is reasonable. If the browser client grows, the first cleanup would be to move the HTML and JS out of this file.

## `src/webrtc_sender.rs`

This module owns the server-side WebRTC session.

### Core idea

The media pipeline produces encoded H264 access units. This module translates that stream into a WebRTC-compatible local track and handles SDP negotiation.

### `accept_offer`

This is the public entrypoint. It takes:

* A broadcast receiver for future access units
* An optional latest access unit snapshot
* The remote offer SDP string

It returns the server’s answer SDP string.

That function is deliberately framed in terms of plain data and one receiver. The web layer does not have to deal with peer connection objects.

### `WebRtcSession`

`WebRtcSession` is a short-lived object that bundles everything needed for one viewer:

* `peer_connection`
* `video_track`
* `encoded_rx`
* `initial_access_unit`

This object is not stored globally. A fresh one is created for each `/session` request.

### Peer connection construction

`build_peer_connection()` sets up the `webrtc` crate API stack:

* Register default codecs
* Register default interceptors
* Build the API object
* Create the `RTCPeerConnection`

No ICE servers are configured, which means this is optimized for local or straightforward connectivity rather than general NAT traversal.

### Codec declaration

`video_codec()` declares H264 with:

* `clock_rate` 90000
* `packetization-mode=1`
* `profile-level-id=42e01f`
* `level-asymmetry-allowed=1`

This is the codec capability advertised for the outgoing track. The rest of the server assumes the media pipeline is actually producing compatible H264.

### Why `TrackLocalStaticSample`

The module uses `TrackLocalStaticSample` rather than manually writing RTP packets. That means the source side only has to provide timestamped chunks of encoded media as samples, and the WebRTC library handles the packetization layer.

This is an important simplification in the architecture: the pipeline produces encoded media samples, not RTP.

### RTCP draining

After `add_track`, the returned RTP sender is used in a background task that repeatedly reads RTCP packets.

This is a standard requirement with the `webrtc` crate. If RTCP is ignored, the sender side can stall or misbehave because control feedback is not being consumed.

### Forwarding loop

The forwarding loop is the heart of the runtime bridge between pipeline and WebRTC:

* Wait on either a periodic resend tick or a new frame from the broadcast receiver
* When a new frame arrives, aggressively drain any extra queued frames to keep only the newest
* Write that newest frame to the local track
* If writing fails, close the peer connection

The drain behavior is deliberate. The server prefers freshness over perfect delivery of every single encoded frame. That is a good fit for live screen-style video where stale frames are worse than dropped frames.

### Latest frame resend

The resend timer republishes the latest access unit every 250ms. This is a recovery mechanism rather than a throughput mechanism. It helps:

* New subscribers start rendering sooner
* Idle sources still provide something to decode
* Sessions recover from timing gaps without waiting indefinitely for new source output

## `src/h264.rs`

This module defines the encoded media contract and the format conversion helpers around it.

## `EncodedAccessUnit`

This struct is intentionally small:

* `data`: encoded bytes
* `duration`: intended sample duration

The rest of the system does not need to know whether those bytes came from VideoToolbox or disk replay.

### AVCC to Annex-B conversion

The macOS encoder produces H264 in AVCC form. WebRTC track writing in this design expects access-unit payloads in Annex-B form, so `sample_buffer_to_access_unit()`:

* Reads the encoded block buffer
* Determines the AVCC NAL length prefix size
* Copies the encoded bytes out of CoreMedia memory
* Prepends SPS/PPS parameter sets
* Rewrites the AVCC length-prefixed NAL layout into Annex-B start-code layout

This step is one of the key format boundaries in the system.

### Annex-B access-unit parsing for file replay

`parse_annex_b_access_units()` is used only by the file backend. It scans raw H264 bytes and tries to group NAL units into access units.

It supports two heuristics:

* If AUD NALs are present, use them as explicit access-unit boundaries
* Otherwise, derive coarse access units from common NAL types like SPS, PPS, IDR, and non-IDR slices

This parser is intentionally limited. It is sufficient for simple H264 elementary streams, but it is not a full demuxer or standards-complete bitstream parser.

## `src/pipeline/mod.rs`

This module is the boundary between “media source implementations” and “the rest of the app”.

### Architectural role

It hides source selection and source-specific startup behind one public type: `MediaPipeline`.

The rest of the server only asks the pipeline for three things:

* Start
* Subscribe
* Report health

That is the central modularity point in the current design.

### Shared state owned by `MediaPipeline`

`MediaPipeline` owns:

* `encoded_tx`: the broadcast fanout for encoded frames
* `stats`: shared counters and error state
* `latest_access_unit`: a cached copy of the most recent frame
* `source_name`: a label for observability

### Why a broadcast channel

A Tokio broadcast channel is a natural fit for one producer and multiple viewers:

* Each viewer gets an independent receiver
* The pipeline does not need to manage per-viewer queues
* A slow viewer can lag without blocking the source thread

The tradeoff is that lagging receivers can drop frames. That is acceptable for the current use case because the system already prefers freshness over complete delivery.

### Why cache the latest access unit separately

The broadcast channel alone is not enough for good startup behavior. A new subscriber only sees future sends. If the source is temporarily idle, a new viewer could wait too long before receiving anything useful.

The separate `latest_access_unit` cache solves that by providing an immediate bootstrap frame to new sessions.

### Source selection

`PipelineConfig::from_env()` chooses the source backend through `POC_SOURCE`.

Implemented backends:

* `screen`
* `file`

Reserved but unimplemented:

* `camera`
* `camera-audio`
* `screen-audio`

Those unimplemented modes currently fail early during startup with explicit errors. That is better than silently selecting the wrong source or partially enabling unsupported behavior.

## `src/pipeline/macos_screen.rs`

This is the platform-specific live capture backend. It is the most complex module because it bridges Rust to Apple’s callback-driven Objective-C APIs.

### Main responsibilities

* Discover the capture target display
* Configure ScreenCaptureKit
* Receive raw screen frames
* Feed frames into VideoToolbox
* Convert encoded output into `EncodedAccessUnit`
* Publish encoded frames and update stats

### Configuration

`MacosScreenConfig` reads:

* `POC_DISPLAY_ID`
* `POC_FPS`
* `POC_MAX_WIDTH`
* `POC_TARGET_BITRATE_BPS`

These settings shape both capture and encoding behavior.

### Capture setup

The backend first asks ScreenCaptureKit for shareable content, then chooses a display. It builds an `SCContentFilter` and `SCStreamConfiguration`, scales the output dimensions to respect `POC_MAX_WIDTH`, and starts an `SCStream`.

The scaling step matters because the encoder is configured against the capture size. Reducing width lowers CPU/GPU cost and bitrate pressure.

### Why a sync channel between capture and encoding

Raw sample buffers from ScreenCaptureKit are pushed into a bounded `mpsc::sync_channel` of capacity 2.

This is an explicit backpressure boundary:

* If capture outruns encoding, the queue fills
* New raw frames are dropped rather than allowing unbounded memory growth
* The system biases toward keeping up with “now” instead of preserving all frames

That is the same freshness-over-completeness policy seen elsewhere in the codebase.

### `FrameStreamOutput`

`FrameStreamOutput` is an Objective-C class defined from Rust with `define_class!`. It implements `SCStreamOutput` so ScreenCaptureKit can call back into Rust for each frame.

Its job is intentionally minimal:

* Ignore non-screen outputs
* Ignore sample buffers that are not ready
* Increment raw-frame stats
* Retain the CoreMedia sample buffer
* Push a pointer token into the sync channel

It does not encode directly on the callback queue. That keeps the callback path short and avoids making the Apple capture queue do heavyweight work.

### VideoToolbox encoding

`VideoToolboxEncoder` wraps a `VTCompressionSession`.

Its constructor:

* Creates the hardware-accelerated encoder session
* Configures required low-latency properties
* Configures optional bitrate and GOP properties
* Prepares the session for frame encoding

Notable encoder choices:

* `RealTime=true`
* `AllowFrameReordering=false`
* Baseline profile preference
* Short keyframe interval

These choices bias toward interoperability and lower latency rather than maximum compression efficiency.

### Encoder callback

Encoded output returns through `video_toolbox_output_callback`, not from the call to `encode_frame()`.

That callback:

* Detects dropped or failed encodes
* Converts the `CMSampleBuffer` into `EncodedAccessUnit`
* Updates the latest-frame cache
* Broadcasts the encoded frame
* Updates counters and timestamps

This is the exact point where native macOS encoding becomes generic pipeline output.

### Memory management constraints

This module contains the most unsafe and ownership-sensitive code because Apple APIs cross ARC-managed Objective-C objects, CoreFoundation retained values, and Rust ownership.

Patterns to note:

* Pointers are retained before crossing thread boundaries
* Retained pointers are reconstituted on the receiving side
* The encoder output context is heap allocated and manually freed in `Drop`

The code is careful to keep this complexity isolated inside the source module so the rest of the server can remain ordinary Rust.

## `src/pipeline/file_h264.rs`

This is the simplest source backend and an important architectural contrast to the macOS screen backend.

### What it does

* Reads all bytes from a configured file
* Parses them into access units
* Emits them on a fixed cadence
* Optionally loops forever

### Why it exists

This backend provides a non-native source that still satisfies the same `EncodedAccessUnit` contract. It proves that the rest of the server is not inherently tied to ScreenCaptureKit.

### Threading model

Like the screen source, the file source runs on its own standard thread. This keeps source execution independent from the async HTTP/WebRTC runtime and preserves a consistent model where source modules own their own timing loops.

### Operational limitations

The parser expects an Annex-B H264 elementary stream. Containerized formats like `.mp4` are out of scope because this module does not demux or decode container metadata.

## Concurrency Model

The codebase mixes three concurrency styles.

### Tokio async

Used for:

* HTTP serving
* WebRTC negotiation
* Background RTCP draining
* Background forwarding from broadcast receiver to track

### Standard threads

Used for:

* Source backends that maintain their own blocking loops
* Isolation of native capture and file playback from async runtime concerns

### Apple callback queues

Used for:

* ScreenCaptureKit frame delivery
* VideoToolbox encode completion callbacks

The architecture is effective because each layer communicates across narrow handoff points:

* ScreenCaptureKit callback to sync channel
* Sync channel to encode loop
* Encoder callback to broadcast sender
* Broadcast receiver to WebRTC track writer

## Data Flow Details

The key payload flow for `POC_SOURCE=screen` is:

1. ScreenCaptureKit emits raw `CMSampleBuffer`
2. The callback retains it and pushes it into a bounded queue
3. The encode loop extracts the image buffer and calls VideoToolbox
4. VideoToolbox returns an encoded `CMSampleBuffer`
5. `h264.rs` converts it to Annex-B `EncodedAccessUnit`
6. The pipeline caches and broadcasts that access unit
7. A WebRTC session receives it and writes it as a media sample
8. The `webrtc` crate packetizes and transports it to the browser

For `POC_SOURCE=file`, steps 1 through 4 are replaced by “read bytes and parse access units from disk,” but steps 5 through 8 remain conceptually the same. That is the main architectural success of the refactor: both sources converge on the same encoded-frame abstraction.

## Health and Observability

`PipelineStats` tracks both activity and failure state.

Important fields:

* `pipeline_ready`: source initialized successfully
* `raw_frames`: frames observed before encoding
* `encode_attempts`: frames submitted to the encoder
* `encoded_frames`: successful encoded access units emitted
* `dropped_frames`: source or encoder loss events
* `last_*_at_ms`: coarse activity timestamps
* `last_error`: most recent pipeline error

Not every counter is equally meaningful for every backend. For example, `raw_frames` is useful for the screen backend but mostly irrelevant for file replay. The structure is still shared because the current number of sources is small and the uniform schema keeps the health endpoint simple.

## Configuration Surface

### Server config

* `POC_SERVER_ADDR`: bind address for the HTTP server

### Shared pipeline config

* `POC_SOURCE`: selects `screen` or `file`
* `POC_FPS`: target pacing used by both current backends

### Screen backend config

* `POC_DISPLAY_ID`: choose a specific display if present
* `POC_MAX_WIDTH`: cap capture width before encoding
* `POC_TARGET_BITRATE_BPS`: encoder bitrate target

### File backend config

* `POC_VIDEO_FILE`: required path to raw H264
* `POC_FILE_LOOP`: whether playback loops forever

## Design Tradeoffs

### Favoring encoded media over raw media abstractions

The system boundary between pipeline and WebRTC sits after encoding, not before. That keeps WebRTC publication simple and allows very different source backends to share the same interface. The tradeoff is that each backend must produce already-compatible H264.

### Favoring freshness over completeness

This principle appears in multiple places:

* Bounded raw-frame queue
* Broadcast channel behavior
* Receiver draining to newest frame
* Latest-frame resend logic

The system is optimized for “show me the most recent view” rather than “preserve every frame”.

### Favoring narrow interfaces over feature completeness

The public interfaces are small:

* `MediaPipeline`
* `accept_offer`
* Simple HTTP handlers

That keeps the code understandable, but it also means features like session management, ICE trickling, multi-track publication, and audio are not yet represented in the architecture.

## Current Limitations

The current implementation is intentionally constrained:

* Only one H264 video track is published
* Audio is not captured, encoded, or signaled
* No STUN/TURN configuration is exposed
* No persistent session registry exists
* `file` mode requires raw Annex-B H264
* `camera`, `camera-audio`, and `screen-audio` are configuration placeholders only

## Natural Next Steps

If this server evolves, the most coherent next changes would be:

* Add an AVFoundation camera backend under `pipeline/`
* Introduce an audio abstraction parallel to `EncodedAccessUnit`
* Extend `webrtc_sender.rs` to publish an audio track
* Add container demuxing for file playback
* Separate the embedded viewer HTML from `web.rs`
* Add more explicit lifecycle and shutdown coordination for source threads
