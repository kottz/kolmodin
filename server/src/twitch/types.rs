use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum TwitchChannelConnectionStatus {
    Initializing,
    Connecting {
        attempt: u32,
    },
    Authenticating {
        attempt: u32,
    },
    Connected,
    Reconnecting {
        reason: String,
        failed_attempt: u32,
        retry_in: Duration,
    },
    Disconnected {
        reason: String,
    },
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedTwitchMessage {
    pub channel: String,
    pub sender_username: String,
    pub sender_user_id: Option<String>,
    pub text: String,
    pub badges: Option<String>,
    pub is_moderator: bool,
    pub is_subscriber: bool,
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_irc_tags: Option<HashMap<String, String>>,
    pub timestamp: DateTime<Utc>,
}
