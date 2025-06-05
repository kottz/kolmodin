use crate::config::TwitchConfig;
use crate::twitch::auth::{AppAccessToken, fetch_twitch_app_access_token};
use crate::twitch::error::TwitchError;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

const TOKEN_REFRESH_GRACE_PERIOD: Duration = Duration::from_secs(60 * 60); // Refresh 1 hour before expiry
// const TOKEN_MIN_VALIDITY_FOR_STARTUP: Duration = Duration::from_secs(5 * 60); // Not strictly needed if first fetch works

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
        tracing::info!("[TOKEN_PROVIDER] Attempting to fetch new app access token.");
        match fetch_twitch_app_access_token(
            &self.twitch_config.client_id,
            &self.twitch_config.client_secret,
        )
        .await
        {
            Ok(new_token_info) => {
                let mut token_wlock = self.current_token.write().await;
                *token_wlock = new_token_info;
                tracing::info!("[TOKEN_PROVIDER] App access token fetched/updated successfully.");
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    "[TOKEN_PROVIDER] Failed to fetch new app access token: {:?}.",
                    e
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
                    "[TOKEN_PROVIDER] Token expires at: {:?}. Time to expiry: {:?}. Calculated sleep until grace: {:?}.",
                    expires_at,
                    time_to_expiry,
                    sleep_duration_until_grace
                );

                // Wait until it's time to refresh OR an immediate refresh is signaled
                tokio::select! {
                    _ = sleep(sleep_duration_until_grace) => {
                        tracing::info!("[TOKEN_PROVIDER] Scheduled refresh period reached or token is near expiry.");
                    }
                    _ = self_clone.force_refresh_trigger.notified() => {
                        tracing::info!("[TOKEN_PROVIDER] Immediate refresh signaled.");
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
                                    "[TOKEN_PROVIDER] Failed to fetch new token after {} attempts. Will retry later based on expiry.",
                                    fetch_attempts
                                );
                                // After max attempts, don't immediately retry.
                                // The outer loop will re-evaluate based on current token's (possibly old) expiry.
                                // A long sleep here prevents tight loops if Twitch is down.
                                sleep(Duration::from_secs(5 * 60)).await;
                                break;
                            }
                            tracing::warn!(
                                "[TOKEN_PROVIDER] Token fetch attempt {} failed. Retrying in 30s.",
                                fetch_attempts
                            );
                            sleep(Duration::from_secs(30)).await;
                        }
                    }
                }
            }
        });
    }
}
