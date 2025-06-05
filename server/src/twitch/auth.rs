use super::error::TwitchError;
use serde::Deserialize;
use std::time::{Duration, Instant};

#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct AppAccessToken {
    pub token: String,
    pub expires_at: Instant,
}

pub async fn fetch_twitch_app_access_token(
    client_id: &str,
    client_secret: &str,
) -> Result<AppAccessToken, TwitchError> {
    tracing::info!("[TWITCH_API] Fetching App Access Token...");
    let url = "https://id.twitch.tv/oauth2/token";
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "client_credentials"),
    ];
    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .form(&params)
        .send()
        .await
        .map_err(TwitchError::Reqwest)?;

    if response.status().is_success() {
        let token_data = response
            .json::<TokenResponse>()
            .await
            .map_err(TwitchError::Reqwest)?;

        // Add a small buffer to expires_in to account for network latency and clock skew,
        // ensuring we attempt refresh slightly before actual expiry.
        // For example, if expires_in is 3600, treat it as 3590.
        let effective_expires_in = token_data.expires_in.saturating_sub(10);
        if effective_expires_in == 0 {
            tracing::warn!(
                "[TWITCH_API] Token expires_in is very short (<=10s): {}. Using as is.",
                token_data.expires_in
            );
        }

        let expires_at = Instant::now() + Duration::from_secs(effective_expires_in);
        tracing::info!(
            "[TWITCH_API] App Access Token fetched successfully. Original expires_in: {}s. Effective expires_at: {:?}.",
            token_data.expires_in,
            expires_at
        );
        Ok(AppAccessToken {
            token: token_data.access_token,
            expires_at,
        })
    } else {
        let status = response.status();
        let error_body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error body".to_string());
        tracing::error!(
            "[TWITCH_API] Failed to get App Access Token (HTTP {}): {}",
            status,
            error_body
        );
        Err(TwitchError::TwitchAuth(format!(
            "Token fetch failed (HTTP {}): {}",
            status, error_body
        )))
    }
}
