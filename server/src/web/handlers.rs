use axum::{extract::State, http::StatusCode, response::Json}; // Added StatusCode
use serde::Deserialize;

use super::error::{Result as WebResult, WebError};
use crate::lobby::LobbyDetails;
use crate::state::AppState;

#[derive(Deserialize, Debug, Default)]
pub struct CreateLobbyRequest {
    pub game_type: Option<String>,
    pub twitch_channel: Option<String>,
}

pub async fn create_lobby_handler(
    State(app_state): State<AppState>,
    Json(payload): Json<CreateLobbyRequest>,
) -> WebResult<Json<LobbyDetails>> {
    tracing::info!("HTTP: Received create_lobby request: {:?}", payload);

    let details = app_state
        .lobby_manager
        .create_lobby(payload.game_type, payload.twitch_channel)
        .await
        .map_err(|e| {
            tracing::error!("Failed to create lobby: {}", e);
            WebError::InternalServerError(e)
        })?;

    Ok(Json(details))
}

pub async fn refresh_words_handler(State(app_state): State<AppState>) -> WebResult<StatusCode> {
    tracing::info!("HTTP: Received refresh_words request");
    app_state
        .word_list_manager
        .refresh_med_andra_ord_words()
        .await
        .map_err(|e| {
            tracing::error!("Failed to refresh words: {:?}", e);
            WebError::InternalServerError(format!("Failed to refresh words: {}", e))
        })?;
    Ok(StatusCode::OK)
}
