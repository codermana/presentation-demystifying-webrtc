layout: true

.signature[@algogrit]

---

class: center, middle

# Demystifying WebRTC

Gaurav Agarwal

---

class: center, middle

![Me](assets/images/me.png)

Software Engineer & Product Developer

Director of Engineering & Founder @ https://codermana.com

ex-Tarka Labs, ex-BrowserStack, ex-ThoughtWorks

---
class: center, middle

## Why WebRTC

---
class: center, middle

HTTP and WebSockets are enough for **data**, but not enough by themselves for **good real-time media** like voice, video, or screen sharing.

---
class: center, middle

### Why HTTP is not enough

---
class: center, middle

HTTP is request/response.

---

That means:

* client asks, server responds

* each exchange is discrete

---

* it is not built for a continuous low-latency stream

* retries and buffering are usually favored over timeliness

---
class: center, middle

For media, **late data is often useless**.

---
class: center, middle

A video frame that arrives 2 seconds late is worse than a dropped frame.

---
class: center, middle

HTTP is generally optimized for correctness and delivery, not “play it now or skip it.”

---
class: center, middle

### Why WebSockets are still not enough

---
class: center, middle

WebSockets improve a lot over HTTP because they give you a persistent full-duplex connection.

---
class: center, middle

That helps for signaling and live app events.

---
class: center, middle

But raw WebSockets still miss several things real-time media needs.

---

- They usually run over TCP

TCP guarantees ordered delivery. Real-time media often prefers **UDP-like behavior** for actual media transport.

- No built-in jitter handling or timing model

Media packets do not just need to arrive. They need to arrive with usable timing.

---

- No standard support for audio/video codecs and negotiation

WebSockets are just a transport pipe. They do not define a media session model.

- No NAT traversal solution

In real apps, two peers are often behind routers, firewalls, CGNAT, mobile networks, and enterprise networks.

---

- No built-in congestion control tuned for live media

- No standard echo cancellation, A/V sync, device integration, etc.

---
class: center, middle

### What real-time media usually needs instead

---
class: center, middle

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

* **HTTP** is good for setup, APIs, downloading, buffered streaming

* **WebSockets** are good for signaling, chat, presence, control messages

* **WebRTC** is good for live audio/video/screen media

---
class: center, middle

## Understanding the RFCs

---
class: center, middle

![WebRTC RFCs](assets/images/webrtc-all-rfcs.png)

.content-credits[https://webrtcforthecurious.com/docs/01-what-why-and-how/]

---

class: center, middle

Code
https://github.com/CoderMana/presentation-demystifying-webrtc

Slides
https://demystifying-webrtc.slides.algogrit.com
