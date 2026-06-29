// The entire signaling surface of the POC, from src/web.rs.
// One route. No WebSocket, no rooms, no re-negotiation.

// #region handler
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
// #endregion handler
