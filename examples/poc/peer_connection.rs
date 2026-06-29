// Server-side WebRTC, trimmed from src/webrtc_sender.rs for the slides.
// Error handling is simplified to `?` so the negotiation flow stays visible;
// the real source carries richer error context.

// #region build
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
// #endregion build

// #region codec
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
// #endregion codec

// #region accept
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
// #endregion accept
