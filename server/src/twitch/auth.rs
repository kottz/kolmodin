use super::error::TwitchError;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
}

pub async fn fetch_twitch_app_access_token(
    client_id: &str,
    client_secret: &str,
) -> Result<String, TwitchError> {
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
        Ok(token_data.access_token)
    } else {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error body".to_string());
        tracing::error!(
            "[TWITCH_API] Failed to get App Access Token (HTTP {}): {}",
            status,
            error_text
        );
        Err(TwitchError::TwitchAuth(format!(
            "Token fetch failed (HTTP {}): {}",
            status, error_text
        )))
    }
}
