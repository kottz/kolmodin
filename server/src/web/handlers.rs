use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
};
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

pub async fn refresh_words_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> WebResult<StatusCode> {
    tracing::info!("HTTP: Received refresh_words request");

    // admin_api_key is guaranteed to be non-empty if the app started.
    let expected_key = &app_state.server_config.admin_api_key;

    match headers.get(http::header::AUTHORIZATION) {
        Some(auth_header_val) => {
            let auth_header_str = auth_header_val.to_str().unwrap_or("");
            if let Some(provided_key) = auth_header_str.strip_prefix("ApiKey ") {
                if provided_key.trim() != expected_key.as_str() {
                    tracing::warn!(
                        "Unauthorized attempt to refresh words: Invalid API key provided."
                    );
                    return Err(WebError::Unauthorized("Invalid API key".to_string()));
                }
                tracing::info!(
                    "Admin API Key (ApiKey scheme) validated successfully for refresh_words."
                );
                // Authorized, proceed to the action
            } else {
                tracing::warn!(
                    "Unauthorized attempt to refresh words: Authorization header format incorrect. Expected 'ApiKey <key>'."
                );
                return Err(WebError::Unauthorized(
                    "Invalid Authorization header format. Expected 'ApiKey <key>'".to_string(),
                ));
            }
        }
        None => {
            tracing::warn!("Unauthorized attempt to refresh words: Missing Authorization header.");
            return Err(WebError::Unauthorized(
                "Missing Authorization header".to_string(),
            ));
        }
    }

    // If we reach here, authentication was successful.
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
