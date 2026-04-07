# Demystifying WebRTC

WebRTC (Web Real-Time Communication) is often seen as a black box of browser APIs. However, underneath those APIs lies a sophisticated collection of standards and protocols designed to solve the hard problem of low-latency, peer-to-peer communication over the open internet.

In this session, we will peel back the layers of the WebRTC stack, starting from the original needs that birthed it, moving through the foundational RFCs, and ending with a practical look at how to implement and secure these connections.

## Session Outline

### 1. The Need for WebRTC
* Why HTTP and WebSockets aren't enough for real-time media
* The quest for low-latency peer-to-peer (P2P) in the browser
* Use cases beyond video calls: Gaming, file sharing, and edge computing

### 2. Summarizing the RFCs

* **The Blueprint:**
  - SDP: [Session Description Protocol](https://datatracker.ietf.org/doc/html/rfc4566)
  - Understanding [RFC 8825](https://datatracker.ietf.org/doc/html/rfc8825) (Overview of the WebRTC Protocol Suite)
* **The Foundation:** Brief look at the underlying RFCs that make it work:
    * [RFC 8445](https://datatracker.ietf.org/doc/html/rfc8445) (ICE)
    * [RFC 8489](https://datatracker.ietf.org/doc/html/rfc8489) (STUN)
    * [RFC 8656](https://datatracker.ietf.org/doc/html/rfc8656) (TURN)
    * [RFC 3264](https://datatracker.ietf.org/doc/html/rfc3264) (Offer/Answer Model)

### 3. Understanding the Lifecycle
* The Signaling Phase: Exchanging metadata (not defined by WebRTC)
* The Handshake: Offer/Answer and SDP exchange
* Gathering ICE candidates and establishing connectivity
* The transition from "Connecting" to "Connected"

### 4. Code Walkthrough: Mirrorpad POC
* Initializing `RTCPeerConnection` and handling tracks
* Connecting the signaling channel (using a simple WebSocket example)

### 5. Media and Data
* **MediaStream API:** Tracks, sinks, and sources
* **RTCDataChannel:** Why it’s more than just a "socket"—ordered vs. unordered, reliable vs. unreliable delivery
* Managing bandwidth and congestion control

### 6. Connectivity: STUN, TURN, and ICE
* **STUN:** How to get a public IP when behind a NAT
* **TURN:** When P2P fails—using relays as a fallback
* **ICE:** The framework that coordinates everything to find the "shortest" path

### 7. Security
* **DTLS (Datagram Transport Layer Security):** Encrypting the data channel ([RFC 6347](https://datatracker.ietf.org/doc/html/rfc6347))
* **SRTP (Secure Real-time Transport Protocol):** Encrypting media streams ([RFC 3711](https://datatracker.ietf.org/doc/html/rfc3711))
* Why WebRTC is "Secure by Default" and what that means for developers

### 8. Practical Implementation & Trade-offs
* Choosing an architecture: Mesh vs. SFU (Selective Forwarding Unit) vs. MCU (Multipoint Control Unit)
* Scaling WebRTC: Handling more than a few participants
* Debugging tools: `about:webrtc` and `chrome://webrtc-internals`

## What You'll Take Away
* A solid understanding of the protocols that make WebRTC work under the hood
* The ability to read and understand WebRTC-related RFCs
* A practical framework for implementing and debugging P2P connections
