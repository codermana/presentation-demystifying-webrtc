// A canonical browser <-> browser WebRTC flow, trimmed for teaching.
//
// `signaling` is any channel you bring yourself (WebSocket, fetch, even a
// QR code): WebRTC defines everything EXCEPT how the two peers first reach
// each other to swap an offer, an answer, and ICE candidates.

// #region create
// 1. Create the connection. STUN lets each peer learn its public address.
const pc = new RTCPeerConnection({
  iceServers: [{ urls: "stun:stun.l.google.com:19302" }],
});
// #endregion create

// #region tracks
// 2. Add local media, and render whatever the remote peer sends back.
const media = await navigator.mediaDevices.getUserMedia({
  audio: true,
  video: true,
});
media.getTracks().forEach((track) => pc.addTrack(track, media));

pc.ontrack = ({ streams: [remote] }) => {
  remoteVideo.srcObject = remote;
};
// #endregion tracks

// #region datachannel
// 3. An optional data channel — you choose the delivery guarantees.
const chat = pc.createDataChannel("chat"); // reliable + ordered (default)
const moves = pc.createDataChannel("moves", {
  ordered: false,
  maxRetransmits: 0, // fire-and-forget, like raw UDP
});
pc.ondatachannel = ({ channel }) => {
  channel.onmessage = (e) => console.log(channel.label, e.data);
};
// #endregion datachannel

// #region ice
// 4. Trickle each ICE candidate to the peer as soon as it is discovered,
//    and feed in the ones they send us. No waiting for the full list.
pc.onicecandidate = ({ candidate }) => {
  if (candidate) signaling.send({ candidate });
};
signaling.on("candidate", (candidate) => pc.addIceCandidate(candidate));
// #endregion ice

// #region offer
// 5a. Caller: describe what we can send/receive, then ship the offer.
const offer = await pc.createOffer();
await pc.setLocalDescription(offer);
signaling.send({ sdp: offer });
// #endregion offer

// #region answer
// 5b. Callee: accept the offer, then answer with our own description.
await pc.setRemoteDescription(remoteOffer);
const answer = await pc.createAnswer();
await pc.setLocalDescription(answer);
signaling.send({ sdp: answer });
// #endregion answer
