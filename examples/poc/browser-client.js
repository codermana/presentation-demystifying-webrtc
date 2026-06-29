// The Mirrorpad viewer, lifted out of the INDEX_HTML string in src/web.rs.
// The browser is receive-only here: the Rust server is the sole publisher.

// #region connect
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
// #endregion connect

// #region ice-wait
// No trickle ICE: we wait for gathering to finish, then send ONE
// complete offer. Simple, at the cost of a little setup latency.
function waitForIceGatheringComplete(pc) {
  if (pc.iceGatheringState === "complete") return Promise.resolve();
  return new Promise((resolve) => {
    pc.addEventListener("icegatheringstatechange", function check() {
      if (pc.iceGatheringState === "complete") {
        pc.removeEventListener("icegatheringstatechange", check);
        resolve();
      }
    });
  });
}
// #endregion ice-wait
