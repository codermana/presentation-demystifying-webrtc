use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use tokio::sync::broadcast::{self, error::TryRecvError};
use tokio::time::{interval, MissedTickBehavior};
use webrtc::{
    api::{
        interceptor_registry::register_default_interceptors,
        media_engine::{MediaEngine, MIME_TYPE_H264},
        APIBuilder,
    },
    interceptor::registry::Registry,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
};

use crate::h264::EncodedAccessUnit;

pub async fn accept_offer(
    encoded_rx: broadcast::Receiver<EncodedAccessUnit>,
    initial_access_unit: Option<EncodedAccessUnit>,
    offer_sdp: String,
) -> Result<String, String> {
    WebRtcSession::new(encoded_rx, initial_access_unit)
        .await?
        .accept_offer(offer_sdp)
        .await
}

struct WebRtcSession {
    peer_connection: Arc<webrtc::peer_connection::RTCPeerConnection>,
    video_track: Arc<TrackLocalStaticSample>,
    encoded_rx: broadcast::Receiver<EncodedAccessUnit>,
    initial_access_unit: Option<EncodedAccessUnit>,
}

impl WebRtcSession {
    async fn new(
        encoded_rx: broadcast::Receiver<EncodedAccessUnit>,
        initial_access_unit: Option<EncodedAccessUnit>,
    ) -> Result<Self, String> {
        let peer_connection = build_peer_connection().await?;
        let video_track = Arc::new(TrackLocalStaticSample::new(
            video_codec(),
            "display".to_string(),
            "mirrorpad-poc".to_string(),
        ));

        Ok(Self {
            peer_connection,
            video_track,
            encoded_rx,
            initial_access_unit,
        })
    }

    async fn accept_offer(self, offer_sdp: String) -> Result<String, String> {
        let rtp_sender = self
            .peer_connection
            .add_track(Arc::clone(&self.video_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await
            .map_err(|err| format!("add_track failed: {err}"))?;

        self.spawn_rtcp_drain(rtp_sender);

        self.peer_connection
            .set_remote_description(
                RTCSessionDescription::offer(offer_sdp)
                    .map_err(|err| format!("invalid offer: {err}"))?,
            )
            .await
            .map_err(|err| format!("set_remote_description failed: {err}"))?;

        let answer = self
            .peer_connection
            .create_answer(None)
            .await
            .map_err(|err| format!("create_answer failed: {err}"))?;
        let mut gather_complete = self.peer_connection.gathering_complete_promise().await;
        self.peer_connection
            .set_local_description(answer)
            .await
            .map_err(|err| format!("set_local_description failed: {err}"))?;
        let _ = gather_complete.recv().await;

        let local = self
            .peer_connection
            .local_description()
            .await
            .ok_or_else(|| "local description missing".to_string())?;

        self.spawn_video_forwarder();

        Ok(local.sdp)
    }

    fn spawn_rtcp_drain(&self, rtp_sender: Arc<webrtc::rtp_transceiver::rtp_sender::RTCRtpSender>) {
        tokio::spawn(async move { while rtp_sender.read_rtcp().await.is_ok() {} });
    }

    fn spawn_video_forwarder(self) {
        let WebRtcSession {
            peer_connection,
            video_track,
            mut encoded_rx,
            initial_access_unit,
        } = self;

        tokio::spawn(async move {
            let mut latest_access_unit = initial_access_unit;
            let mut resend_interval = interval(Duration::from_millis(250));
            resend_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = resend_interval.tick() => {
                        let Some(latest) = latest_access_unit.clone() else {
                            continue;
                        };
                        if write_access_unit(&video_track, latest).await.is_err() {
                            break;
                        }
                    }
                    recv_result = encoded_rx.recv() => {
                        let Ok(access_unit) = recv_result else {
                            break;
                        };
                        let newest = drain_latest_access_unit(&mut encoded_rx, access_unit);
                        latest_access_unit = Some(newest.clone());
                        if write_access_unit(&video_track, newest).await.is_err() {
                            break;
                        }
                    }
                }
            }

            let _ = peer_connection.close().await;
        });
    }
}

async fn build_peer_connection() -> Result<Arc<webrtc::peer_connection::RTCPeerConnection>, String>
{
    let mut media_engine = MediaEngine::default();
    media_engine
        .register_default_codecs()
        .map_err(|err| format!("register_default_codecs failed: {err}"))?;
    let registry = register_default_interceptors(Registry::new(), &mut media_engine)
        .map_err(|err| format!("register_default_interceptors failed: {err}"))?;
    let api = APIBuilder::new()
        .with_media_engine(media_engine)
        .with_interceptor_registry(registry)
        .build();

    Ok(Arc::new(
        api.new_peer_connection(RTCConfiguration {
            ice_servers: vec![],
            ..Default::default()
        })
        .await
        .map_err(|err| format!("new_peer_connection failed: {err}"))?,
    ))
}

fn video_codec() -> RTCRtpCodecCapability {
    RTCRtpCodecCapability {
        mime_type: MIME_TYPE_H264.to_owned(),
        clock_rate: 90_000,
        channels: 0,
        sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
            .to_string(),
        rtcp_feedback: vec![],
    }
}

fn drain_latest_access_unit(
    encoded_rx: &mut broadcast::Receiver<EncodedAccessUnit>,
    mut newest: EncodedAccessUnit,
) -> EncodedAccessUnit {
    loop {
        match encoded_rx.try_recv() {
            Ok(next) => newest = next,
            Err(TryRecvError::Empty) => return newest,
            Err(TryRecvError::Lagged(_)) => continue,
            Err(TryRecvError::Closed) => return newest,
        }
    }
}

async fn write_access_unit(
    track: &Arc<TrackLocalStaticSample>,
    access_unit: EncodedAccessUnit,
) -> Result<(), ()> {
    track
        .write_sample(&webrtc::media::Sample {
            data: Bytes::from(access_unit.data.to_vec()),
            duration: access_unit.duration.max(Duration::from_millis(1)),
            ..Default::default()
        })
        .await
        .map_err(|_| ())
}
