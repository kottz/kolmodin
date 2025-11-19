use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use serde::{Deserialize, Serialize};

use super::error::{Result as WebResult, WebError};
use crate::lobby::LobbyDetails;
use crate::state::AppState;

#[derive(Deserialize, Debug, Default)]
pub struct CreateLobbyRequest {
    pub game_type: Option<String>,
    pub twitch_channel: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct AllowedChannelsResponse {
    pub channels: Vec<String>,
}

#[tracing::instrument(skip(app_state), fields(
    http.method = "POST",
    http.path = "/api/create-lobby",
    request.game_type = ?payload.game_type,
    request.twitch_channel = ?payload.twitch_channel
))]
pub async fn create_lobby_handler(
    State(app_state): State<AppState>,
    Json(payload): Json<CreateLobbyRequest>,
) -> WebResult<Json<LobbyDetails>> {
    tracing::debug!("Processing create lobby request");

    let details = app_state
        .create_lobby(payload.game_type, payload.twitch_channel)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to create lobby");
            WebError::InternalServerError(e)
        })?;

    tracing::info!(
        lobby.id = %details.lobby_id,
        lobby.game_type = %details.game_type_created,
        lobby.twitch_channel = ?details.twitch_channel_subscribed,
        "Lobby created successfully"
    );

    Ok(Json(details))
}

#[tracing::instrument(skip(app_state, headers), fields(
    http.method = "GET",
    http.path = "/api/refresh-words"
))]
pub async fn refresh_words_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> WebResult<StatusCode> {
    tracing::debug!("Processing refresh words request");

    let expected_key = &app_state.server_config.admin_api_key;

    match headers.get(http::header::AUTHORIZATION) {
        Some(auth_header_val) => {
            let auth_header_str = auth_header_val.to_str().unwrap_or("");
            if let Some(provided_key) = auth_header_str.strip_prefix("ApiKey ") {
                if provided_key.trim() != expected_key.as_str() {
                    tracing::warn!(
                        reason = "invalid_api_key",
                        "Unauthorized attempt to refresh words"
                    );
                    return Err(WebError::Unauthorized("Invalid API key".to_string()));
                }
                tracing::debug!("Admin API key validated successfully");
            } else {
                tracing::warn!(
                    reason = "invalid_auth_format",
                    "Unauthorized attempt to refresh words: Authorization header format incorrect"
                );
                return Err(WebError::Unauthorized(
                    "Invalid Authorization header format. Expected 'ApiKey <key>'".to_string(),
                ));
            }
        }
        None => {
            tracing::warn!(
                reason = "missing_auth_header",
                "Unauthorized attempt to refresh words"
            );
            return Err(WebError::Unauthorized(
                "Missing Authorization header".to_string(),
            ));
        }
    }

    app_state
        .game_content_cache
        .refresh_all_content()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to refresh words");
            WebError::InternalServerError(format!("Failed to refresh words: {}", e))
        })?;

    tracing::info!("Words refreshed successfully");
    Ok(StatusCode::OK)
}

#[tracing::instrument(skip(app_state), fields(
    http.method = "GET",
    http.path = "/api/allowed-channels"
))]
pub async fn get_allowed_channels_handler(
    State(app_state): State<AppState>,
) -> WebResult<Json<AllowedChannelsResponse>> {
    tracing::debug!("Processing get allowed channels request");

    let channels: Vec<String> = app_state
        .game_content_cache
        .twitch_whitelist()
        .await
        .iter()
        .cloned()
        .collect();

    tracing::debug!(
        channels.count = channels.len(),
        "Retrieved allowed channels"
    );

    Ok(Json(AllowedChannelsResponse { channels }))
}
