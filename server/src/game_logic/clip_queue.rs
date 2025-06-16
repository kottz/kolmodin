use axum::extract::ws;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use uuid::Uuid;

use crate::config::AppSettings;
use crate::game_logic::messages::{
    ClientToServerMessage as GenericClientToServerMessage,
    ServerToClientMessage as GenericServerToClientMessage,
};
use crate::game_logic::{EventHandlingResult, GameLogic};
use crate::twitch::ParsedTwitchMessage;
use std::sync::Arc;
use tracing::{error, info, warn};

const GAME_TYPE_ID_CLIP_QUEUE: &str = "ClipQueue";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipInfo {
    pub video_id: String,
    pub title: String,
    pub channel_title: String,
    pub duration_iso8601: String,
    pub thumbnail_url: String,
    pub submitted_by_username: String,
    pub submitted_at_timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipQueueSettings {
    pub submissions_open: bool,
    pub allow_duplicates: bool,
    pub max_clip_duration_seconds: u32,
}

impl Default for ClipQueueSettings {
    fn default() -> Self {
        Self {
            submissions_open: true,
            allow_duplicates: false,
            max_clip_duration_seconds: 600,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipQueueGameState {
    pub clip_queue: Vec<ClipInfo>,
    pub removed_by_admin_clip_ids: HashSet<String>,
    pub settings: ClipQueueSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command")]
pub enum AdminCommand {
    RemoveClipFromQueue { video_id: String },
    UpdateSettings { new_settings: ClipQueueSettings },
    ResetQueue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type", content = "data")]
pub enum GameEvent {
    ClipAdded {
        clip: ClipInfo,
    },
    ClipRemoved {
        video_id: String,
        removed_by_admin: bool,
    },
    SettingsChanged {
        new_settings: ClipQueueSettings,
    },
    QueueWasReset,
    ClipSubmissionRejected {
        submitted_by_username: String,
        input_text: String,
        reason: String,
    },
    ConfigError {
        message: String,
    },
    FullStateUpdate {
        state: ClipQueueGameState,
    },
}

#[derive(Debug)]
pub struct ClipQueueGame {
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,
    state: ClipQueueGameState,
    app_settings: Arc<AppSettings>,
    youtube_url_regex: Regex,
}

impl ClipQueueGame {
    pub fn new(app_settings: Arc<AppSettings>) -> Self {
        let state = ClipQueueGameState {
            clip_queue: Vec::new(),
            removed_by_admin_clip_ids: HashSet::new(),
            settings: ClipQueueSettings::default(),
        };

        // Regex to extract YouTube video IDs from various URL formats
        let youtube_url_regex = Regex::new(
            r"(?:(?:https?://)?(?:www\.)?(?:youtube\.com/(?:watch\?v=|embed/|v/)|youtu\.be/))([a-zA-Z0-9_-]{11})"
        ).expect("Failed to compile YouTube URL regex");

        Self {
            clients: HashMap::new(),
            state,
            app_settings,
            youtube_url_regex,
        }
    }

    fn extract_video_id(&self, input: &str) -> Option<String> {
        // First try regex for URLs
        if let Some(captures) = self.youtube_url_regex.captures(input) {
            return Some(captures[1].to_string());
        }

        // Then check if input is already a video ID (11 characters, alphanumeric + - and _)
        let trimmed = input.trim();
        if trimmed.len() == 11
            && trimmed
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            return Some(trimmed.to_string());
        }

        None
    }

    fn is_duplicate(&self, video_id: &str) -> bool {
        // Check if already in queue
        self.state
            .clip_queue
            .iter()
            .any(|clip| clip.video_id == video_id)
    }

    async fn validate_youtube_video(&self, video_id: &str) -> Result<ClipInfo, String> {
        let youtube_config = match &self.app_settings.youtube {
            Some(config) => config,
            None => {
                error!(
                    "YouTube API not configured. Set KOLMODIN__YOUTUBE__API_KEY environment variable."
                );
                return Err(
                    "YouTube API not configured. Please contact the administrator.".to_string(),
                );
            }
        };

        if youtube_config.api_key.is_empty() {
            error!("YouTube API key is empty");
            return Err("YouTube API key not properly configured".to_string());
        }

        // Make YouTube API request
        let url = format!(
            "https://www.googleapis.com/youtube/v3/videos?id={}&part=snippet,contentDetails,status&key={}",
            video_id, youtube_config.api_key
        );

        info!("Making YouTube API request for video ID: {}", video_id);

        let response = reqwest::get(&url)
            .await
            .map_err(|e| format!("API request failed: {}", e))?;

        let status = response.status();
        if !status.is_success() {
            error!(
                "YouTube API returned error status: {} for video {}",
                status, video_id
            );

            // Try to get error details from response body
            let error_body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read error response".to_string());
            warn!("YouTube API error response: {}", error_body);

            return match status.as_u16() {
                403 => Err(
                    "YouTube API access forbidden. Check API key permissions and quota."
                        .to_string(),
                ),
                404 => Err("Video not found or not accessible".to_string()),
                _ => Err(format!("YouTube API error: {}", status)),
            };
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let items = json["items"]
            .as_array()
            .ok_or("Invalid API response format")?;

        if items.is_empty() {
            return Err("Video not found".to_string());
        }

        let video = &items[0];

        // Check if video is embeddable and public
        let status = &video["status"];
        if status["embeddable"].as_bool() != Some(true) {
            return Err("Video is not embeddable".to_string());
        }
        if status["privacyStatus"].as_str() != Some("public") {
            return Err("Video is not public".to_string());
        }

        let snippet = &video["snippet"];
        let content_details = &video["contentDetails"];

        let title = snippet["title"].as_str().ok_or("Missing video title")?;
        let channel_title = snippet["channelTitle"]
            .as_str()
            .ok_or("Missing channel title")?;
        let duration_iso8601 = content_details["duration"]
            .as_str()
            .ok_or("Missing video duration")?;
        let thumbnail_url = snippet["thumbnails"]["default"]["url"]
            .as_str()
            .ok_or("Missing thumbnail URL")?;

        // Parse duration and check against max length
        let duration_seconds =
            parse_iso8601_duration(duration_iso8601).ok_or("Invalid duration format")?;

        if duration_seconds > self.state.settings.max_clip_duration_seconds {
            return Err(format!(
                "Video too long ({} seconds, max: {})",
                duration_seconds, self.state.settings.max_clip_duration_seconds
            ));
        }

        Ok(ClipInfo {
            video_id: video_id.to_string(),
            title: title.to_string(),
            channel_title: channel_title.to_string(),
            duration_iso8601: duration_iso8601.to_string(),
            thumbnail_url: thumbnail_url.to_string(),
            submitted_by_username: String::new(), // Will be set by caller
            submitted_at_timestamp: 0,            // Will be set by caller
        })
    }

    async fn broadcast_event(&self, event: &GameEvent) {
        let server_message = match GenericServerToClientMessage::new_game_specific_event(
            GAME_TYPE_ID_CLIP_QUEUE.to_string(),
            event,
        ) {
            Ok(msg) => msg,
            Err(e) => {
                error!("Failed to serialize ClipQueue event: {:?}", e);
                return;
            }
        };

        let ws_message = match server_message.to_ws_text() {
            Ok(msg) => msg,
            Err(e) => {
                error!(
                    "Failed to convert ClipQueue event to WebSocket message: {:?}",
                    e
                );
                return;
            }
        };

        for (client_id, client_tx) in &self.clients {
            if let Err(e) = client_tx.send(ws_message.clone()).await {
                warn!(
                    "Failed to send ClipQueue event to client {}: {:?}",
                    client_id, e
                );
            }
        }
    }

    async fn broadcast_full_state(&self) {
        let event = GameEvent::FullStateUpdate {
            state: self.state.clone(),
        };
        self.broadcast_event(&event).await;
    }

    async fn handle_admin_command(&mut self, command: AdminCommand) {
        let mut events_to_broadcast = Vec::new();

        match command {
            AdminCommand::RemoveClipFromQueue { video_id } => {
                // Remove from queue
                self.state
                    .clip_queue
                    .retain(|clip| clip.video_id != video_id);

                // Add to removed list
                self.state
                    .removed_by_admin_clip_ids
                    .insert(video_id.clone());

                events_to_broadcast.push(GameEvent::ClipRemoved {
                    video_id,
                    removed_by_admin: true,
                });
            }

            AdminCommand::UpdateSettings { new_settings } => {
                self.state.settings = new_settings.clone();
                events_to_broadcast.push(GameEvent::SettingsChanged { new_settings });
            }

            AdminCommand::ResetQueue => {
                self.state.clip_queue.clear();
                self.state.removed_by_admin_clip_ids.clear();

                events_to_broadcast.push(GameEvent::QueueWasReset);
            }
        }

        // Broadcast events
        for event in events_to_broadcast {
            self.broadcast_event(&event).await;
        }

        // Always send full state update after admin commands
        self.broadcast_full_state().await;
    }
}

// Helper function to parse ISO 8601 duration (PT3M32S) to seconds
fn parse_iso8601_duration(duration: &str) -> Option<u32> {
    let re = Regex::new(r"PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+)S)?").ok()?;
    let captures = re.captures(duration)?;

    let hours: u32 = captures
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    let minutes: u32 = captures
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);
    let seconds: u32 = captures
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0);

    Some(hours * 3600 + minutes * 60 + seconds)
}

impl GameLogic for ClipQueueGame {
    async fn client_connected(&mut self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        info!("ClipQueue: Client {} connected", client_id);
        self.clients.insert(client_id, client_tx);

        // Send current state to new client
        self.broadcast_full_state().await;
    }

    async fn client_disconnected(&mut self, client_id: Uuid) {
        info!("ClipQueue: Client {} disconnected", client_id);
        self.clients.remove(&client_id);
    }

    async fn handle_event(
        &mut self,
        _client_id: Uuid,
        message: GenericClientToServerMessage,
    ) -> EventHandlingResult {
        match message {
            GenericClientToServerMessage::GameSpecificCommand {
                game_type_id,
                command_data,
            } => {
                if game_type_id != GAME_TYPE_ID_CLIP_QUEUE {
                    warn!(
                        "ClipQueue received command for wrong game type: {}",
                        game_type_id
                    );
                    return EventHandlingResult::Handled;
                }

                match serde_json::from_value::<AdminCommand>(command_data) {
                    Ok(command) => {
                        info!("ClipQueue: Processing admin command: {:?}", command);
                        self.handle_admin_command(command).await;
                    }
                    Err(e) => {
                        error!("ClipQueue: Failed to deserialize admin command: {:?}", e);
                    }
                }
            }
            GenericClientToServerMessage::LeaveLobby => {
                return EventHandlingResult::DisconnectClient;
            }
            _ => {
                // Handle other message types if needed
            }
        }

        EventHandlingResult::Handled
    }

    async fn handle_twitch_message(&mut self, message: ParsedTwitchMessage) {
        // Check for !clip command
        if !message.text.starts_with("!clip ") {
            return;
        }

        let input = message.text.strip_prefix("!clip ").unwrap_or("").trim();
        if input.is_empty() {
            let event = GameEvent::ClipSubmissionRejected {
                submitted_by_username: message.sender_username.clone(),
                input_text: message.text.clone(),
                reason: "No URL or video ID provided".to_string(),
            };
            self.broadcast_event(&event).await;
            return;
        }

        // Check if submissions are open
        if !self.state.settings.submissions_open {
            let event = GameEvent::ClipSubmissionRejected {
                submitted_by_username: message.sender_username.clone(),
                input_text: message.text.clone(),
                reason: "Submissions are currently closed".to_string(),
            };
            self.broadcast_event(&event).await;
            return;
        }

        // Extract video ID
        let video_id = match self.extract_video_id(input) {
            Some(id) => id,
            None => {
                let event = GameEvent::ClipSubmissionRejected {
                    submitted_by_username: message.sender_username.clone(),
                    input_text: message.text.clone(),
                    reason: "Invalid YouTube URL or video ID".to_string(),
                };
                self.broadcast_event(&event).await;
                return;
            }
        };

        // Check if admin removed this clip
        if self.state.removed_by_admin_clip_ids.contains(&video_id) {
            let event = GameEvent::ClipSubmissionRejected {
                submitted_by_username: message.sender_username.clone(),
                input_text: message.text.clone(),
                reason: "This clip was removed by the admin".to_string(),
            };
            self.broadcast_event(&event).await;
            return;
        }

        // Check for duplicates if not allowed
        if !self.state.settings.allow_duplicates && self.is_duplicate(&video_id) {
            let event = GameEvent::ClipSubmissionRejected {
                submitted_by_username: message.sender_username.clone(),
                input_text: message.text.clone(),
                reason: "Duplicate clips are not allowed".to_string(),
            };
            self.broadcast_event(&event).await;
            return;
        }

        // Validate with YouTube API
        match self.validate_youtube_video(&video_id).await {
            Ok(mut clip_info) => {
                // Set submission details
                clip_info.submitted_by_username = message.sender_username.clone();
                clip_info.submitted_at_timestamp = chrono::Utc::now().timestamp();

                // Add to queue
                self.state.clip_queue.push(clip_info.clone());

                let event = GameEvent::ClipAdded { clip: clip_info };
                self.broadcast_event(&event).await;
                self.broadcast_full_state().await;
            }
            Err(reason) => {
                let event = GameEvent::ClipSubmissionRejected {
                    submitted_by_username: message.sender_username.clone(),
                    input_text: message.text.clone(),
                    reason,
                };
                self.broadcast_event(&event).await;
            }
        }
    }

    fn is_empty(&self) -> bool {
        self.clients.is_empty()
    }

    fn game_type_id(&self) -> String {
        GAME_TYPE_ID_CLIP_QUEUE.to_string()
    }

    fn get_client_tx(&self, client_id: Uuid) -> Option<TokioMpscSender<ws::Message>> {
        self.clients.get(&client_id).cloned()
    }

    fn get_all_client_ids(&self) -> Vec<Uuid> {
        self.clients.keys().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServerConfig, TwitchConfig, GamesConfig, DatabaseConfig, YouTubeConfig, DataSourceType};

    fn create_test_config() -> Arc<AppSettings> {
        Arc::new(AppSettings {
            server: ServerConfig {
                port: 3000,
                cors_origins: vec!["http://localhost:5173".to_string()],
                admin_api_key: "test_key".to_string(),
            },
            twitch: TwitchConfig {
                client_id: "test_client_id".to_string(),
                client_secret: "test_client_secret".to_string(),
                irc_server_url: "irc.chat.twitch.tv:6667".to_string(),
            },
            games: GamesConfig {
                enabled_types: std::collections::HashSet::new(),
            },
            database: DatabaseConfig {
                source_type: DataSourceType::File,
                file_path: Some("/tmp/test.db".to_string()),
                http_url: None,
            },
            youtube: Some(YouTubeConfig {
                api_key: "test_youtube_api_key".to_string(),
            }),
        })
    }

    #[test]
    fn test_parse_iso8601_duration() {
        assert_eq!(parse_iso8601_duration("PT3M32S"), Some(212)); // 3*60 + 32 = 212
        assert_eq!(parse_iso8601_duration("PT1H2M3S"), Some(3723)); // 1*3600 + 2*60 + 3 = 3723
        assert_eq!(parse_iso8601_duration("PT45S"), Some(45)); // 45 seconds
        assert_eq!(parse_iso8601_duration("PT5M"), Some(300)); // 5*60 = 300
        assert_eq!(parse_iso8601_duration("PT2H"), Some(7200)); // 2*3600 = 7200
        assert_eq!(parse_iso8601_duration("PT0S"), Some(0)); // 0 seconds
        assert_eq!(parse_iso8601_duration("invalid"), None); // Invalid format
    }

    #[test]
    fn test_extract_video_id() {
        let config = create_test_config();
        let game = ClipQueueGame::new(config);

        // Test various YouTube URL formats
        assert_eq!(
            game.extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            game.extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            game.extract_video_id("dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            game.extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=123"),
            Some("dQw4w9WgXcQ".to_string())
        );

        // Test invalid inputs
        assert_eq!(game.extract_video_id("invalid"), None);
        assert_eq!(game.extract_video_id("https://example.com"), None);
        assert_eq!(game.extract_video_id(""), None);
    }

    #[test]
    fn test_is_duplicate() {
        let config = create_test_config();
        let mut game = ClipQueueGame::new(config);

        // Add a clip to the queue
        let clip = ClipInfo {
            video_id: "dQw4w9WgXcQ".to_string(),
            title: "Test Video".to_string(),
            channel_title: "Test Channel".to_string(),
            duration_iso8601: "PT3M32S".to_string(),
            thumbnail_url: "https://example.com/thumb.jpg".to_string(),
            submitted_by_username: "testuser".to_string(),
            submitted_at_timestamp: 0,
        };
        game.state.clip_queue.push(clip);

        // Test duplicate detection
        assert!(game.is_duplicate("dQw4w9WgXcQ"));
        assert!(!game.is_duplicate("different_video_id"));
    }

    #[test]
    fn test_default_settings() {
        let settings = ClipQueueSettings::default();
        assert!(settings.submissions_open);
        assert!(!settings.allow_duplicates);
        assert_eq!(settings.max_clip_duration_seconds, 600);
    }

    #[test]
    fn test_game_type_id() {
        let config = create_test_config();
        let game = ClipQueueGame::new(config);
        assert_eq!(game.game_type_id(), "ClipQueue");
    }

    #[test]
    fn test_is_empty_initially() {
        let config = create_test_config();
        let game = ClipQueueGame::new(config);
        assert!(game.is_empty());
    }
}
