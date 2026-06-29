---
marp: true
theme: base
paginate: true
size: 16:9
transition: fade 0.4s
title: "Demystifying WebRTC"
description: "A technical talk explaining why WebRTC exists and what protocols make it work."
author: "Gaurav Agarwal"
footer: "![CoderMana](assets/codermana.svg)"
---

<!-- _class: title -->
<!-- _transition: coverflow 0.7s -->

<!-- deck:title:start -->

###### WebRTC

# Demystifying WebRTC

A technical talk explaining why WebRTC exists and what protocols make it work.

###### Gaurav Agarwal

<!-- deck:title:end -->

---

<!-- _class: cols-photo -->

<div class="cols">
<div class="col-media">

![Me](assets/images/me.png)

</div>
<div class="col-body">

## Gaurav Agarwal

Software Engineer & Product Developer

Director of Engineering & Founder @ https://codermana.com

ex-Tarka Labs, ex-BrowserStack, ex-ThoughtWorks

</div>
</div>

---

# Agenda

1. Why WebRTC exists
2. The RFCs that define it
3. The lifecycle of a connection
4. Code walkthrough — canonical API, then a real POC
5. Media & data channels
6. Connectivity: STUN, TURN, ICE
7. Security: secure by default
8. Architecture & trade-offs

> The value is the model. The APIs make sense once the model does.

---

<!-- _class: section -->

###### Start Here

# Why WebRTC

---

<!-- _class: quote -->

> HTTP and WebSockets are enough for **data**, but not enough by themselves for **good real-time media** like voice, video, or screen sharing.

---

<!-- _class: section -->

###### HTTP

# Why HTTP is not enough

---

HTTP is request/response.

---

That means:

* client asks, server responds
* each exchange is discrete

---

* it is not built for a continuous low-latency stream
* retries and buffering are usually favored over timeliness

---

<!-- _class: quote -->

> For media, **late data is often useless**.

---

<!-- _class: quote -->

> A video frame that arrives 2 seconds late is worse than a dropped frame.

---

HTTP is generally optimized for correctness and delivery, not “play it now or skip it.”

---

<!-- _class: section -->

###### WebSockets

# Why WebSockets are still not enough

---

WebSockets improve a lot over HTTP because they give you a persistent full-duplex connection.

---

That helps for signaling and live app events.

---

But raw WebSockets still miss several things real-time media needs.

---

<!-- _class: cards -->

# WebSockets miss media needs

| Transport | Timing | Session |
| --- | --- | --- |
| They usually run over TCP. TCP guarantees ordered delivery. Real-time media often prefers **UDP-like behavior** for actual media transport. | No built-in jitter handling or timing model. Media packets do not just need to arrive. They need to arrive with usable timing. | No standard support for audio/video codecs and negotiation. WebSockets are just a transport pipe. They do not define a media session model. |

---

<!-- _class: cards -->

# More gaps

| Connectivity | Congestion | Devices |
| --- | --- | --- |
| No NAT traversal solution. In real apps, two peers are often behind routers, firewalls, CGNAT, mobile networks, and enterprise networks. | No built-in congestion control tuned for live media. | No standard echo cancellation, A/V sync, device integration, etc. |

---

<!-- _class: section -->

###### Media Transport

# What real-time media usually needs instead

---

Real-time media needs a **media transport system**.

---

WebRTC adds the missing pieces:

* UDP-first transport
* RTP/RTCP style media handling
* jitter buffers
* packet loss recovery strategies
* congestion control
* codec negotiation
* NAT traversal via ICE/STUN/TURN
* encryption
* A/V synchronization
* browser and device support

---

<!-- _class: cards -->

# Where each tool fits

| HTTP | WebSockets | WebRTC |
| --- | --- | --- |
| **HTTP** is good for setup, APIs, downloading, buffered streaming. | **WebSockets** are good for signaling, chat, presence, control messages. | **WebRTC** is good for live audio/video/screen media. |

---

<!-- _class: cards -->

# Not just video calls

| Gaming | File & data | Edge & IoT |
| --- | --- | --- |
| Low-latency state, voice, and inputs between players — often peer-to-peer, no game server in the path. | Direct browser-to-browser transfer over `RTCDataChannel`, no upload-then-download round trip. | Camera feeds, robotics, and cloud rendering streamed straight to a browser with sub-second latency. |

---

<!-- _class: section -->

###### Standards

# The RFCs

---

<!-- _class: image image-credit -->

## WebRTC is a suite, not one spec

![WebRTC RFCs](assets/images/webrtc-all-rfcs.png)

https://webrtcforthecurious.com/docs/01-what-why-and-how/

---

<!-- _class: cards -->

# The RFC map

| The blueprint | Connectivity | Security |
| --- | --- | --- |
| SDP — session description (4566) · Offer/Answer (3264) · Overview (8825) | ICE (8445) · STUN (8489) · TURN (8656) | DTLS (6347) · SRTP (3711) |

---

###### Blueprint

## RFC 4566 — SDP

**Session Description Protocol.** A plain-text format describing a media session: media types, codecs, transport addresses, and parameters.

- It is what the offer and the answer actually *contain*
- Describes capabilities — it does not move media itself

https://datatracker.ietf.org/doc/html/rfc4566

---

###### Blueprint

## RFC 3264 — Offer / Answer

**The negotiation model.** One peer sends an SDP *offer*; the other replies with a compatible *answer*.

- Converges on a shared set of codecs and parameters
- WebRTC drives it through `setLocal` / `setRemoteDescription`

https://datatracker.ietf.org/doc/html/rfc3264

---

###### Blueprint

## RFC 8825 — WebRTC Overview

**The map of the suite.** Names the protocols WebRTC builds on and how they fit together.

- Your entry point into the whole spec family
- Points out to RTP, ICE, DTLS, SCTP, SDP, and more

https://datatracker.ietf.org/doc/html/rfc8825

---

###### Connectivity

## RFC 8445 — ICE

**Interactive Connectivity Establishment.** Gathers candidate addresses and probes pairs to find a path that works.

- Combines host, STUN-reflexive, and TURN-relay candidates
- Picks the best pair; supports incremental *trickle* ICE

https://datatracker.ietf.org/doc/html/rfc8445

---

###### Connectivity

## RFC 8489 — STUN

**Session Traversal Utilities for NAT.** Lets a peer discover its public, NAT-mapped address.

- Cheap and effectively stateless
- Answers "what address can others actually reach me on?"

https://datatracker.ietf.org/doc/html/rfc8489

---

###### Connectivity

## RFC 8656 — TURN

**Traversal Using Relays around NAT.** A relay that forwards media when no direct path can be found.

- Works even through symmetric NATs and strict firewalls
- Costs server bandwidth — the fallback, never the default

https://datatracker.ietf.org/doc/html/rfc8656

---

###### Security

## RFC 6347 — DTLS 1.2

**Datagram TLS.** A TLS handshake over UDP that agrees on keys before any media flows.

- The certificate *fingerprint* is carried inside the SDP
- Bootstraps the keys that SRTP then uses

https://datatracker.ietf.org/doc/html/rfc6347

---

###### Security

## RFC 3711 — SRTP

**Secure RTP.** Encrypts and authenticates every media packet.

- Keys come straight from the DTLS handshake (DTLS-SRTP)
- Mandatory — there is no unencrypted media path

https://datatracker.ietf.org/doc/html/rfc3711

---

<!-- _class: section -->

###### Lifecycle

# How a connection comes up

---

# The lifecycle

1. **Signaling** — swap setup metadata over *your own* channel
2. **Offer / Answer** — exchange SDP: codecs, tracks, parameters
3. **ICE** — gather candidates, probe paths, pick the best
4. **DTLS handshake** — agree on keys
5. **Connected** — media (SRTP) and data flow directly

---

<!-- _class: quote -->

> WebRTC defines everything **except** how two peers first find each other. Signaling is yours to build.

---

<!-- _class: section -->

###### Code

# Walkthrough

---

The canonical browser ↔ browser flow first.

Then a real one I built.

---

<!-- _class: code -->

## 1. Create the connection

<!-- snippet: examples/canonical/peer.js#create -->

```js
// 1. Create the connection. STUN lets each peer learn its public address.
const pc = new RTCPeerConnection({
  iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
});
```

> STUN servers let each peer discover its own public address.

---

<!-- _class: code -->

## 2. Add media, receive media

<!-- snippet: examples/canonical/peer.js#tracks -->

```js
// 2. Add local media, and render whatever the remote peer sends back.
const media = await navigator.mediaDevices.getUserMedia({
  audio: true,
  video: true,
});
media.getTracks().forEach((track) => pc.addTrack(track, media));

pc.ontrack = ({ streams: [remote] }) => {
  remoteVideo.srcObject = remote;
};
```

---

<!-- _class: code -->

## 3a. Caller: make an offer

<!-- snippet: examples/canonical/peer.js#offer -->

```js
// 5a. Caller: describe what we can send/receive, then ship the offer.
const offer = await pc.createOffer();
await pc.setLocalDescription(offer);
signaling.send({ sdp: offer });
```

---

<!-- _class: code -->

## 3b. Callee: answer it

<!-- snippet: examples/canonical/peer.js#answer -->

```js
// 5b. Callee: accept the offer, then answer with our own description.
await pc.setRemoteDescription(remoteOffer);
const answer = await pc.createAnswer();
await pc.setLocalDescription(answer);
signaling.send({ sdp: answer });
```

---

<!-- _class: code -->

## 4. Trickle ICE candidates

<!-- snippet: examples/canonical/peer.js#ice -->

```js
// 4. Trickle each ICE candidate to the peer as soon as it is discovered,
//    and feed in the ones they send us. No waiting for the full list.
pc.onicecandidate = ({ candidate }) => {
  if (candidate) signaling.send({ candidate });
};
signaling.on("candidate", (candidate) => pc.addIceCandidate(candidate));
```

> Send candidates as they arrive instead of waiting for the full list — the connection comes up sooner.

---

<!-- _class: section -->

###### Demo

# Mirrorpad

A real POC: a macOS screen, streamed to the browser over WebRTC.

---

# What Mirrorpad does

* **Server (Rust)** captures the screen, encodes **H.264**, and *publishes* it
* **Browser** is receive-only — it just renders the track
* **Signaling** is a single HTTP `POST /session`: offer in, answer out
* Built on the `webrtc` crate (a Rust port of pion/webrtc) + `axum`

> One-way and same-network on purpose — small enough to read end to end.

---

<!-- _class: code code-tight -->

## Browser side: connect()

<!-- snippet: examples/poc/browser-client.js#connect -->

```js
async function connect() {
  const pc = new RTCPeerConnection({ iceServers: [] });
  pc.addTransceiver("video", { direction: "recvonly" }); // receive-only
  pc.ontrack = ({ streams: [stream] }) => { viewer.srcObject = stream; };

  // Create our offer and gather ICE candidates before sending.
  await pc.setLocalDescription(await pc.createOffer());
  await waitForIceGatheringComplete(pc);

  // Signaling = one HTTP POST: offer up, answer back.
  const res = await fetch("/session", {
    method: "POST",
    body: JSON.stringify({ sdp: pc.localDescription.sdp }),
  });
  const answer = await res.json();
  await pc.setRemoteDescription({ type: "answer", sdp: answer.sdp });
}
```

---

<!-- _class: code -->

## Server side: open the connection

<!-- snippet: examples/poc/peer_connection.rs#build -->

```rust
async fn build_peer_connection() -> Result<Arc<RTCPeerConnection>> {
    let mut media_engine = MediaEngine::default();
    media_engine.register_default_codecs()?;
    let registry =
        register_default_interceptors(Registry::new(), &mut media_engine)?;
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();

    // No STUN/TURN: serves same-network viewers only.
    let config = RTCConfiguration { ice_servers: vec![], ..Default::default() };
    Ok(Arc::new(api.new_peer_connection(config).await?))
}
```

---

<!-- _class: code -->

## Declare the codec

<!-- snippet: examples/poc/peer_connection.rs#codec -->

```rust
// We publish a single H.264 video track.
fn video_codec() -> RTCRtpCodecCapability {
    RTCRtpCodecCapability {
        mime_type: MIME_TYPE_H264.to_owned(),
        clock_rate: 90_000,
        sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;\
                        profile-level-id=42e01f"
            .to_string(),
        ..Default::default()
    }
}
```

---

<!-- _class: code code-tight -->

## Negotiate: offer in, answer out

<!-- snippet: examples/poc/peer_connection.rs#accept -->

```rust
async fn accept_offer(self, offer_sdp: String) -> Result<String> {
    // 1. Publish our outbound media track.
    self.pc.add_track(self.video_track.clone()).await?;
    // 2. Apply the browser's offer.
    let offer = RTCSessionDescription::offer(offer_sdp)?;
    self.pc.set_remote_description(offer).await?;
    // 3. Build our answer.
    let answer = self.pc.create_answer(None).await?;
    // 4. Set it locally, then wait for ICE gathering.
    let mut gather_done = self.pc.gathering_complete_promise().await;
    self.pc.set_local_description(answer).await?;
    let _ = gather_done.recv().await;
    // 5. Return the gathered answer SDP.
    let local = self.pc.local_description().await.ok_or("no local desc")?;
    self.spawn_video_forwarder();
    Ok(local.sdp)
}
```

---

<!-- _class: code -->

## The entire signaling surface

<!-- snippet: examples/poc/signaling.rs#handler -->

```rust
// POST an SDP offer, get an SDP answer. That is the whole protocol.
async fn create_session_handler(
    State(state): State<AppState>,
    Json(offer): Json<SessionOffer>,
) -> Result<Json<SessionAnswer>, AppError> {
    // Subscribe to the live H.264 stream, then negotiate around it.
    let (encoded_rx, first_frame) = state.pipeline.subscribe();
    let sdp = accept_offer(encoded_rx, first_frame, offer.sdp)
        .await
        .map_err(AppError::internal)?;
    Ok(Json(SessionAnswer { sdp }))
}
```

---

<!-- _class: caveat -->

## What this POC does — and doesn't

It is a faithful, minimal **publisher**: real SDP, real ICE, real DTLS/SRTP.

> Caveat: it is one-way (server → browser), uses HTTP not WebSocket signaling, sends a media track not a data channel, and configures no STUN/TURN — so it only reaches viewers on the same network.

---

<!-- _class: section -->

###### Media & Data

# Two ways to move bytes

---

# MediaStream

* A `MediaStream` is a bundle of **tracks** (audio, video)
* **Sources** produce frames: a camera, a mic, a screen, a canvas
* **Sinks** consume them: a `<video>` element, a recorder, a peer
* Tracks are negotiated, encoded, and synced for you

---

<!-- _class: code -->

## RTCDataChannel: pick your guarantees

<!-- snippet: examples/canonical/peer.js#datachannel -->

```js
// 3. An optional data channel — you choose the delivery guarantees.
const chat = pc.createDataChannel("chat"); // reliable + ordered (default)
const moves = pc.createDataChannel("moves", {
  ordered: false,
  maxRetransmits: 0, // fire-and-forget, like raw UDP
});
pc.ondatachannel = ({ channel }) => {
  channel.onmessage = (e) => console.log(channel.label, e.data);
};
```

---

<!-- _class: cards -->

# Data channel delivery modes

| Reliable + ordered | Unordered | Unreliable |
| --- | --- | --- |
| Default. Like TCP — every message arrives, in order. Good for chat and state sync. | `ordered: false`. Arrives, but maybe out of order. Lower head-of-line blocking. | `maxRetransmits: 0` or a deadline. Like UDP — drop rather than wait. Good for game inputs. |

---

Media gets **congestion control** too: WebRTC watches loss and delay, then adapts bitrate and resolution to fit the path.

---

<!-- _class: section -->

###### Connectivity

# STUN, TURN, ICE

---

<!-- _class: quote -->

> Two peers behind home routers have **no idea** what public address the other can be reached at.

---

<!-- _class: cards -->

# Three tools, one job

| STUN | TURN | ICE |
| --- | --- | --- |
| "What's my public address?" A peer asks a STUN server and learns its NAT-mapped IP/port. | When direct paths all fail, relay through a TURN server. Always works, costs bandwidth. | The framework that gathers every candidate and probes pairs to find the best working path. |

---

# ICE candidate types

* **host** — your local LAN address
* **srflx** (server-reflexive) — your public address, via STUN
* **relay** — a TURN relay, the last-resort fallback

ICE pairs them up, runs connectivity checks, and promotes the best pair. With **trickle ICE**, it does this while candidates are still arriving.

---

<!-- _class: section -->

###### Security

# Secure by default

---

# There is no unencrypted WebRTC

* Every connection does a **DTLS** handshake before any media flows
* Media is encrypted with **SRTP**; data channels ride DTLS (over SCTP)
* The DTLS certificate **fingerprint** travels inside the SDP — so signaling integrity protects the keys
* Encryption is mandatory: there is no "off" switch

---

<!-- _class: cards -->

# The encryption stack

| DTLS | SRTP | Fingerprint |
| --- | --- | --- |
| Handshake over UDP that agrees on keys (RFC 6347). | Encrypts and authenticates each media packet (RFC 3711). | The cert hash in the SDP binds the handshake to the peer you negotiated with. |

---

<!-- _class: section -->

###### Architecture

# Trade-offs at scale

---

<!-- _class: cards -->

# Mesh vs SFU vs MCU

| Mesh | SFU | MCU |
| --- | --- | --- |
| Every peer connects to every other. Simple; uploads explode past ~4 peers. | A server *forwards* each stream selectively. Scales well; clients decode many streams. | A server *mixes* streams into one. Cheapest client, heaviest server, highest latency. |

---

# Scaling & debugging

* Past a handful of participants, move from **mesh** to an **SFU**
* Watch the live state in the browser:
  - `chrome://webrtc-internals`
  - `about:webrtc` (Firefox)
* Inspect candidates, selected pair, bitrate, loss, and jitter in real time

---

<!-- _class: takeaway -->

# Takeaways

* WebRTC exists because HTTP/WebSockets can't do **real-time media** well
* It's a **suite of RFCs** — SDP, ICE, STUN/TURN, DTLS, SRTP — not one API
* The hard parts are **connectivity** and **timing**; signaling is yours
* It is **encrypted by default**, always

---

<!-- deck:resources:start -->

## Resources

Code

https://github.com/codermana/presentation-demystifying-webrtc

Slides

https://demystifying-webrtc.slides.algogrit.com

<!-- deck:resources:end -->
