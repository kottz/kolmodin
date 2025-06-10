use super::error::TwitchError;
use crate::config::TwitchConfig;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

impl std::fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenResponse")
            .field("access_token", &"***REDACTED***")
            .field("expires_in", &self.expires_in)
            .finish()
    }
}

#[derive(Clone)]
pub struct AppAccessToken {
    pub token: String,
    pub expires_at: Instant,
}

impl std::fmt::Debug for AppAccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppAccessToken")
            .field("token", &"***REDACTED***")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

pub async fn fetch_twitch_app_access_token(
    client_id: &str,
    client_secret: &str,
) -> Result<AppAccessToken, TwitchError> {
    tracing::info!("Fetching App Access Token");
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
                token.expires_in = token_data.expires_in,
                "Token expires_in is very short (<=10s). Using as is"
            );
        }

        let expires_at = Instant::now() + Duration::from_secs(effective_expires_in);
        tracing::info!(
            token.expires_in = token_data.expires_in,
            token.expires_at = ?expires_at,
            "App Access Token fetched successfully"
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
            http.status = %status,
            error.body = %error_body,
            "Failed to get App Access Token"
        );
        Err(TwitchError::TwitchAuth(format!(
            "Token fetch failed (HTTP {}): {}",
            status, error_body
        )))
    }
}

const TOKEN_REFRESH_GRACE_PERIOD: Duration = Duration::from_secs(60 * 60); // Refresh 1 hour before expiry

#[derive(Clone)]
pub struct TokenProvider {
    current_token: Arc<RwLock<AppAccessToken>>,
    twitch_config: Arc<TwitchConfig>,
    // Used to signal the refresh task to check/refresh immediately
    // Useful if an external event (like a 401) indicates the token is bad.
    force_refresh_trigger: Arc<tokio::sync::Notify>,
}

impl TokenProvider {
    pub async fn new(twitch_config: Arc<TwitchConfig>) -> Result<Self, TwitchError> {
        let initial_token =
            fetch_twitch_app_access_token(&twitch_config.client_id, &twitch_config.client_secret)
                .await?;

        let provider = Self {
            current_token: Arc::new(RwLock::new(initial_token)),
            twitch_config,
            force_refresh_trigger: Arc::new(tokio::sync::Notify::new()),
        };

        provider.spawn_refresh_task();
        Ok(provider)
    }

    pub async fn get_token(&self) -> String {
        self.current_token.read().await.token.clone()
    }

    /// Signals the background refresh task to attempt a token fetch immediately.
    pub fn signal_immediate_refresh(&self) {
        self.force_refresh_trigger.notify_one();
    }

    // Internal method to fetch a new token
    async fn fetch_new_token_and_update(&self) -> Result<(), TwitchError> {
        tracing::info!("Attempting to fetch new app access token");
        match fetch_twitch_app_access_token(
            &self.twitch_config.client_id,
            &self.twitch_config.client_secret,
        )
        .await
        {
            Ok(new_token_info) => {
                let mut token_wlock = self.current_token.write().await;
                *token_wlock = new_token_info;
                tracing::info!("App access token fetched/updated successfully");
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    error = ?e,
                    "Failed to fetch new app access token"
                );
                Err(e)
            }
        }
    }

    fn spawn_refresh_task(&self) {
        let self_clone = self.clone();
        tokio::spawn(async move {
            loop {
                let expires_at = self_clone.current_token.read().await.expires_at;
                let now = Instant::now();
                let time_to_expiry = expires_at.saturating_duration_since(now);

                let sleep_duration_until_grace = if time_to_expiry <= TOKEN_REFRESH_GRACE_PERIOD {
                    Duration::from_secs(0) // Already in grace period or expired, check immediately
                } else {
                    time_to_expiry - TOKEN_REFRESH_GRACE_PERIOD
                };

                tracing::debug!(
                    token.expires_at = ?expires_at,
                    time.to_expiry = ?time_to_expiry,
                    sleep.duration = ?sleep_duration_until_grace,
                    "Token refresh timing calculated"
                );

                // Wait until it's time to refresh OR an immediate refresh is signaled
                tokio::select! {
                    _ = sleep(sleep_duration_until_grace) => {
                        tracing::info!("Scheduled refresh period reached or token is near expiry");
                    }
                    _ = self_clone.force_refresh_trigger.notified() => {
                        tracing::info!("Immediate refresh signaled");
                    }
                }

                // Attempt to fetch a new token
                // Retry logic for fetching: simple fixed delay retry for a few times.
                let mut fetch_attempts = 0;
                loop {
                    fetch_attempts += 1;
                    match self_clone.fetch_new_token_and_update().await {
                        Ok(_) => break, // Success, continue the outer loop to wait for next expiry
                        Err(_) => {
                            if fetch_attempts >= 3 {
                                tracing::error!(
                                    attempts = fetch_attempts,
                                    "Failed to fetch new token after max attempts. Will retry later based on expiry"
                                );
                                // After max attempts, don't immediately retry.
                                // The outer loop will re-evaluate based on current token's (possibly old) expiry.
                                // A long sleep here prevents tight loops if Twitch is down.
                                sleep(Duration::from_secs(5 * 60)).await;
                                break;
                            }
                            tracing::warn!(
                                attempt = fetch_attempts,
                                "Token fetch attempt failed. Retrying in 30s"
                            );
                            sleep(Duration::from_secs(30)).await;
                        }
                    }
                }
            }
        });
    }

    pub fn get_irc_server_url(&self) -> &str {
        &self.twitch_config.irc_server_url
    }
}
