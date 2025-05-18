// src/twitch_integration.rs

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

// This now refers to the actual handle defined in the (soon to be created) twitch_chat_manager module.
// Ensure your project structure has `src/twitch_chat_manager.rs` and it's declared in `lib.rs` or `main.rs` (e.g. `mod twitch_chat_manager;`)
use crate::twitch_chat_manager::TwitchChatManagerActorHandle;

// --- Error Definitions ---
#[derive(Error, Debug)]
pub enum TwitchError {
    #[error("Environment variable not set: {0}")]
    EnvVarError(String),
    #[error("HTTP request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON deserialization failed: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Twitch IRC authentication/API error: {0}")]
    TwitchAuth(String),
    #[error("IRC message parsing error: {0}")]
    ParseError(String),
    #[error("Capability negotiation failed: NAK received for {0:?}")]
    CapabilityNak(Vec<String>),
    #[error("Missing access token in API response")]
    MissingToken,
    #[error("Actor communication error: {0}")]
    ActorComm(String),
    #[error("Twitch Channel Actor for {0} shut down or failed to start")]
    ChannelActorTerminated(String),
    #[error("Failed to send to subscriber for lobby {0}: {1}")]
    SubscriberSendError(Uuid, String),
    #[error("Channel actor internal error: {0}")]
    InternalActorError(String),
    #[error("IRC Task failed to send to actor: {0}")]
    IrcTaskSendError(String),
}
pub type Result<T, E = TwitchError> = std::result::Result<T, E>;

// --- Connection Status Enum for a Twitch Channel ---
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

// --- Parsed Twitch Message (New Data Structure) ---
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
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// --- IRC Message Parser ---
#[derive(Debug, Default)]
pub struct IrcMessage<'a> {
    raw: &'a str,
    tags: Option<&'a str>,
    prefix: Option<&'a str>,
    command: Option<&'a str>,
    params: Vec<&'a str>,
}

impl<'a> IrcMessage<'a> {
    pub fn parse(line: &'a str) -> Self {
        let mut message = IrcMessage {
            raw: line,
            ..Default::default()
        };
        let mut remainder = line.trim_end_matches(['\r', '\n']);

        if remainder.starts_with('@') {
            if let Some(space_idx) = remainder.find(' ') {
                message.tags = Some(&remainder[1..space_idx]);
                remainder = &remainder[space_idx + 1..];
            } else {
                message.tags = Some(&remainder[1..]);
                return message;
            }
        }
        if remainder.starts_with(':') {
            if let Some(space_idx) = remainder.find(' ') {
                message.prefix = Some(&remainder[1..space_idx]);
                remainder = &remainder[space_idx + 1..];
            } else {
                message.prefix = Some(&remainder[1..]);
                return message;
            }
        }
        if let Some(trail_marker_idx) = remainder.find(" :") {
            let command_and_middle_params_part = &remainder[..trail_marker_idx];
            let trailing_param = &remainder[trail_marker_idx + 2..];
            let mut parts = command_and_middle_params_part.split(' ');
            message.command = parts.next().filter(|s| !s.is_empty());
            for p_str in parts {
                if !p_str.is_empty() {
                    message.params.push(p_str);
                }
            }
            message.params.push(trailing_param);
        } else {
            let mut parts = remainder.split(' ');
            message.command = parts.next().filter(|s| !s.is_empty());
            for p_str in parts {
                if !p_str.is_empty() {
                    message.params.push(p_str);
                }
            }
        }
        message
    }

    pub fn get_tag_value(&self, key_to_find: &str) -> Option<&'a str> {
        self.tags.and_then(|tags_str| {
            tags_str.split(';').find_map(|component| {
                let mut parts = component.splitn(2, '=');
                let key = parts.next()?;
                if key == key_to_find {
                    parts.next().or(Some(""))
                } else {
                    None
                }
            })
        })
    }

    pub fn get_display_name(&self) -> Option<&'a str> {
        self.get_tag_value("display-name")
    }

    pub fn get_prefix_username(&self) -> Option<&'a str> {
        self.prefix.and_then(|p| p.split('!').next())
    }

    pub fn get_privmsg_text_content(&self) -> Option<&'a str> {
        if self.command == Some("PRIVMSG") && self.params.len() > 1 {
            self.params.last().copied()
        } else {
            None
        }
    }

    pub fn to_parsed_twitch_message(&self, channel_name_str: &str) -> Option<ParsedTwitchMessage> {
        if self.command != Some("PRIVMSG") {
            return None;
        }
        let target_channel_in_msg = self.params.get(0)?.trim_start_matches('#');
        if !target_channel_in_msg.eq_ignore_ascii_case(channel_name_str) {
            return None;
        }

        // Original text extraction
        let initial_text_content = self.get_privmsg_text_content()?.to_string();

        // --- BEGIN TEXT CLEANUP ---
        // Step 1: Standard trim for leading/trailing common whitespace (ASCII space, tab, newline etc.)
        let mut cleaned_text_content = initial_text_content.trim().to_string();

        // Step 2: Iteratively remove problematic trailing Unicode characters.
        // This targets characters like U+200B (Zero Width Space), U+FE0F (Variation Selector),
        // control characters, format characters, and the specific PUA/Tag range observed (U+E0000-U+E007F).
        while let Some(last_char) = cleaned_text_content.chars().last() {
            let char_unicode_val = last_char as u32;

            // Check for various categories of non-content characters
            if last_char.is_control() ||  // Catches Unicode control characters (Cc, Cf categories)
               last_char.is_whitespace() || // Catches broader Unicode whitespace (Zs, Zl, Zp categories)
               (char_unicode_val >= 0xE0000 && char_unicode_val <= 0xE007F) || // Unicode Tag characters (often used as invisible markers)
               char_unicode_val == 0x200B || // Zero Width Space
               char_unicode_val == 0xFE0F || // Variation Selector 16 (used with emojis, can be appended)
               char_unicode_val == 0x200C || // Zero Width Non-Joiner
               char_unicode_val == 0x200D
            // Zero Width Joiner
            // Add other specific Unicode points or small ranges if more are identified
            {
                cleaned_text_content.pop(); // Remove the last character
            } else {
                // If the last character is not one of the types we want to remove, stop.
                break;
            }
        }
        // --- END TEXT CLEANUP ---

        // Use the fully cleaned text_content for the ParsedTwitchMessage
        let text = cleaned_text_content;

        let sender_username = self
            .get_display_name()
            .or_else(|| self.get_prefix_username())
            .unwrap_or("unknown_user")
            .to_string();
        let sender_user_id = self.get_tag_value("user-id").map(str::to_string);
        let badges_str = self.get_tag_value("badges").map(str::to_string);
        let message_id = self.get_tag_value("id").map(str::to_string);

        let is_moderator = self.get_tag_value("mod") == Some("1")
            || badges_str
                .as_ref()
                .map_or(false, |b| b.contains("moderator"));
        let is_subscriber = self.get_tag_value("subscriber") == Some("1")
            || self
                .get_tag_value("badges")
                .map_or(false, |b| b.contains("subscriber/"));

        let mut raw_tags_map = HashMap::new();
        if let Some(tags_str) = self.tags {
            for component in tags_str.split(';') {
                let mut parts = component.splitn(2, '=');
                if let Some(key) = parts.next() {
                    raw_tags_map.insert(key.to_string(), parts.next().unwrap_or("").to_string());
                }
            }
        }

        Some(ParsedTwitchMessage {
            channel: channel_name_str.to_string(),
            sender_username,
            sender_user_id,
            text, // Use the cleaned text here
            badges: badges_str,
            is_moderator,
            is_subscriber,
            message_id,
            raw_irc_tags: if raw_tags_map.is_empty() {
                None
            } else {
                Some(raw_tags_map)
            },
            timestamp: Utc::now(),
        })
    }
}

// --- TwitchChannelActor: Messages, Handle, and Struct ---
#[derive(Debug)]
pub enum TwitchChannelActorMessage {
    AddSubscriber {
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
        respond_to: oneshot::Sender<Result<()>>,
    },
    RemoveSubscriber {
        lobby_id: Uuid,
        respond_to: oneshot::Sender<()>,
    },
    InternalIrcLineReceived {
        line: String,
    },
    InternalConnectionStatusChanged {
        new_status: TwitchChannelConnectionStatus,
    },
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct TwitchChannelActorHandle {
    pub sender: mpsc::Sender<TwitchChannelActorMessage>,
    pub channel_name: String,
    status_rx: watch::Receiver<TwitchChannelConnectionStatus>,
}

impl TwitchChannelActorHandle {
    pub fn new(
        channel_name: String,
        oauth_token: Arc<String>,
        manager_handle: TwitchChatManagerActorHandle, // Using the actual (to be defined) manager handle
        actor_buffer_size: usize,
    ) -> Self {
        let (actor_tx, actor_rx) = mpsc::channel(actor_buffer_size);
        let (status_tx, status_rx) = watch::channel(TwitchChannelConnectionStatus::Initializing);

        let actor = TwitchChannelActor::new(
            actor_rx,
            actor_tx.clone(),
            channel_name.clone(),
            oauth_token,
            manager_handle,
            status_tx.clone(),
        );

        tokio::spawn(run_twitch_channel_actor(actor));

        Self {
            sender: actor_tx,
            channel_name,
            status_rx,
        }
    }

    pub async fn add_subscriber(
        &self,
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
    ) -> Result<()> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchChannelActorMessage::AddSubscriber {
                lobby_id,
                subscriber_tx,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| TwitchError::ActorComm(format!("Failed to send AddSubscriber: {}", e)))?;

        respond_to_rx
            .await
            .map_err(|e| TwitchError::ActorComm(format!("AddSubscriber response error: {}", e)))?
    }

    pub async fn remove_subscriber(&self, lobby_id: Uuid) -> Result<()> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchChannelActorMessage::RemoveSubscriber {
                lobby_id,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!("Failed to send RemoveSubscriber: {}", e))
            })?;

        respond_to_rx
            .await
            .map_err(|e| TwitchError::ActorComm(format!("RemoveSubscriber response error: {}", e)))
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.sender
            .send(TwitchChannelActorMessage::Shutdown)
            .await
            .map_err(|e| TwitchError::ActorComm(format!("Failed to send Shutdown: {}", e)))
    }

    pub fn get_status_receiver(&self) -> watch::Receiver<TwitchChannelConnectionStatus> {
        self.status_rx.clone()
    }
}

pub struct TwitchChannelActor {
    receiver: mpsc::Receiver<TwitchChannelActorMessage>,
    self_sender_for_irc_task: mpsc::Sender<TwitchChannelActorMessage>,
    actor_id: Uuid,
    channel_name: String,
    oauth_token: Arc<String>,
    subscribers: HashMap<Uuid, mpsc::Sender<ParsedTwitchMessage>>,
    irc_connection_task_handle: Option<JoinHandle<()>>,
    irc_task_shutdown_tx: Option<oneshot::Sender<()>>,
    status_tx: watch::Sender<TwitchChannelConnectionStatus>,
    manager_handle: TwitchChatManagerActorHandle, // Using the actual manager handle
}

impl TwitchChannelActor {
    fn new(
        receiver: mpsc::Receiver<TwitchChannelActorMessage>,
        self_sender_for_irc_task: mpsc::Sender<TwitchChannelActorMessage>,
        channel_name: String,
        oauth_token: Arc<String>,
        manager_handle: TwitchChatManagerActorHandle,
        status_tx: watch::Sender<TwitchChannelConnectionStatus>,
    ) -> Self {
        let actor_id = Uuid::new_v4();
        update_channel_status(
            &channel_name,
            actor_id,
            &status_tx,
            TwitchChannelConnectionStatus::Initializing,
        );

        Self {
            receiver,
            self_sender_for_irc_task,
            actor_id,
            channel_name,
            oauth_token,
            subscribers: HashMap::new(),
            irc_connection_task_handle: None,
            irc_task_shutdown_tx: None,
            status_tx,
            manager_handle,
        }
    }

    async fn handle_message(&mut self, msg: TwitchChannelActorMessage) {
        match msg {
            TwitchChannelActorMessage::AddSubscriber {
                lobby_id,
                subscriber_tx,
                respond_to,
            } => {
                tracing::info!(
                    "[TWITCH][ACTOR][{}][{}] Adding subscriber for lobby {}",
                    self.channel_name,
                    self.actor_id,
                    lobby_id
                );
                self.subscribers.insert(lobby_id, subscriber_tx);

                // Check if the IRC task needs to be started/restarted.
                let task_is_truly_stopped_or_never_started = self
                    .irc_connection_task_handle
                    .as_ref()
                    .map_or(true, |h| h.is_finished());

                let current_actor_status = self.status_tx.borrow().clone();

                if matches!(
                    current_actor_status,
                    TwitchChannelConnectionStatus::Terminated
                ) {
                    tracing::warn!(
                        "[TWITCH][ACTOR][{}][{}] AddSubscriber received, but actor status is Terminated. Responding with error.",
                        self.channel_name, self.actor_id
                    );
                    // Actor is shutting down, new subscriptions are not meaningful.
                    let _ = respond_to.send(Err(TwitchError::ChannelActorTerminated(
                        self.channel_name.clone(),
                    )));
                } else if task_is_truly_stopped_or_never_started {
                    tracing::info!(
                        "[TWITCH][ACTOR][{}][{}] IRC task is finished or was never started. Calling start_irc_connection_task.",
                        self.channel_name, self.actor_id
                    );
                    self.start_irc_connection_task(); // This will handle logic for existing finished handles.
                    let _ = respond_to.send(Ok(()));
                } else {
                    // Task exists and is not finished, and actor is not terminated.
                    // This means it's either Connected, Connecting, Authenticating, or Reconnecting.
                    // No need to start a new task.
                    tracing::debug!(
                        "[TWITCH][ACTOR][{}][{}] IRC task is active (status: {:?}, handle exists and not finished). Not starting new task.",
                        self.channel_name, self.actor_id, current_actor_status
                    );
                    let _ = respond_to.send(Ok(()));
                }
            }
            TwitchChannelActorMessage::RemoveSubscriber {
                lobby_id,
                respond_to,
            } => {
                tracing::info!(
                    "[TWITCH][ACTOR][{}][{}] Removing subscriber for lobby {}",
                    self.channel_name,
                    self.actor_id,
                    lobby_id
                );
                self.subscribers.remove(&lobby_id);
                let _ = respond_to.send(());

                if self.subscribers.is_empty() && self.irc_connection_task_handle.is_some() {
                    tracing::info!(
                        "[TWITCH][ACTOR][{}][{}] No more subscribers. Signalling IRC task to shutdown.",
                        self.channel_name, self.actor_id
                    );
                    self.signal_irc_task_shutdown();
                }
            }
            TwitchChannelActorMessage::InternalIrcLineReceived { line } => {
                let irc_msg = IrcMessage::parse(&line);

                if let Some(parsed_message) = irc_msg.to_parsed_twitch_message(&self.channel_name) {
                    let mut disconnected_subscribers = Vec::new();
                    for (lobby_id, tx) in &self.subscribers {
                        if tx.try_send(parsed_message.clone()).is_err() {
                            tracing::warn!(
                                "[TWITCH][ACTOR][{}][{}] Failed to send message to subscriber lobby {} (channel full or closed). Removing.",
                                self.channel_name, self.actor_id, lobby_id
                            );
                            disconnected_subscribers.push(*lobby_id);
                        }
                    }
                    for lobby_id in disconnected_subscribers {
                        self.subscribers.remove(&lobby_id);
                    }
                    if self.subscribers.is_empty() && self.irc_connection_task_handle.is_some() {
                        tracing::info!(
                            "[TWITCH][ACTOR][{}][{}] All subscribers disconnected after send failures. Signalling IRC task to shutdown.",
                            self.channel_name, self.actor_id
                        );
                        self.signal_irc_task_shutdown();
                    }
                } else {
                    if !line.trim().is_empty()
                        && (irc_msg.command.is_some() || irc_msg.prefix.is_some())
                        && !matches!(
                            irc_msg.command,
                            Some("PING")
                                | Some("PONG")
                                | Some("CAP")
                                | Some("001")
                                | Some("002")
                                | Some("003")
                                | Some("004")
                                | Some("372")
                                | Some("375")
                                | Some("376")
                                | Some("JOIN")
                                | Some("PART")
                                | Some("NOTICE")
                                | Some("GLOBALUSERSTATE")
                                | Some("ROOMSTATE")
                                | Some("USERSTATE")
                                | Some("CLEARCHAT")
                                | Some("CLEARMSG")
                                | Some("USERNOTICE")
                                | Some("RECONNECT")
                                | None
                        )
                    {
                        tracing::trace!(
                            "[TWITCH][ACTOR][{}][{}] Received unhandled/non-chat IRC line: {}",
                            self.channel_name,
                            self.actor_id,
                            line.trim()
                        );
                    }
                }
            }
            TwitchChannelActorMessage::InternalConnectionStatusChanged { new_status } => {
                // Store old status for comparison if needed, though not strictly used in this revised logic
                // let old_status = self.status_tx.borrow().clone();
                update_channel_status(
                    &self.channel_name,
                    self.actor_id,
                    &self.status_tx,
                    new_status.clone(),
                );

                match new_status {
                    TwitchChannelConnectionStatus::Disconnected { ref reason } => {
                        tracing::info!(
                            "[TWITCH][ACTOR][{}][{}] Received Disconnected status. Reason: '{}'",
                            self.channel_name,
                            self.actor_id,
                            reason
                        );

                        // Check if the JoinHandle itself indicates the task has exited.
                        // This is important because run_irc_connection_loop might send Disconnected
                        // for a failed attempt but intends to retry. The JoinHandle is for the whole loop.
                        // If the loop *itself* has exited, then the JoinHandle will be finished.
                        let irc_loop_task_has_exited = self
                            .irc_connection_task_handle
                            .as_ref()
                            .map_or(true, |h| h.is_finished()); // True if no handle or handle is finished

                        if irc_loop_task_has_exited {
                            tracing::info!(
                                "[TWITCH][ACTOR][{}][{}] The IRC connection loop task has exited.",
                                self.channel_name,
                                self.actor_id
                            );
                            // Clear the handle since the task it refers to is gone.
                            self.irc_connection_task_handle.take();
                            self.irc_task_shutdown_tx.take(); // Also clear the shutdown sender.

                            // Now decide if the actor itself should shut down.
                            if reason.contains("Persistent Auth Failure")
                                || reason.contains("Actor channel closed")
                            // Should not happen if actor_tx is used by loop
                            {
                                tracing::error!(
                                    "[TWITCH][ACTOR][{}][{}] Critical IRC error after loop exit: '{}'. Actor shutting down.",
                                    self.channel_name, self.actor_id, reason
                                );
                                self.initiate_actor_shutdown().await;
                            } else if self.subscribers.is_empty() {
                                tracing::info!(
                                    "[TWITCH][ACTOR][{}][{}] IRC loop exited (reason: '{}') and no subscribers. Actor shutting down.",
                                    self.channel_name, self.actor_id, reason
                                );
                                self.initiate_actor_shutdown().await;
                            } else {
                                // IRC loop exited, but there are subscribers. This implies something external
                                // stopped the loop (like a shutdown signal it obeyed), or it encountered an
                                // unrecoverable error not caught as "critical" above.
                                // We should attempt to restart the IRC task for the existing subscribers.
                                tracing::warn!(
                                    "[TWITCH][ACTOR][{}][{}] IRC loop exited (reason: '{}') but actor has subscribers. Attempting to restart IRC task.",
                                    self.channel_name, self.actor_id, reason
                                );
                                self.start_irc_connection_task();
                            }
                        } else {
                            // IRC task is still running (e.g., run_irc_connection_loop sent Disconnected for a failed *attempt* but will retry).
                            // The actor should stay alive. The loop will manage further status updates.
                            tracing::debug!(
                                "[TWITCH][ACTOR][{}][{}] IRC connection attempt failed (reason: '{}'), but IRC loop is still active and will retry. Actor remains active.",
                                self.channel_name, self.actor_id, reason
                            );
                        }
                    }
                    TwitchChannelConnectionStatus::Terminated => {
                        // Actor is already shutting down or has shut down. Clean up task handles just in case.
                        if let Some(handle) = self.irc_connection_task_handle.take() {
                            if !handle.is_finished() {
                                handle.abort();
                            }
                        }
                        self.irc_task_shutdown_tx.take();
                    }
                    _ => {
                        // Other statuses (Connecting, Authenticating, Connected, Reconnecting)
                        // imply the IRC loop is active or trying. No specific actor shutdown action.
                    }
                }
            }
            TwitchChannelActorMessage::Shutdown => {
                self.initiate_actor_shutdown().await; // Changed from stop_irc_connection_task_and_shutdown_actor
            }
        }
    }

    async fn initiate_actor_shutdown(&mut self) {
        tracing::info!(
            "[TWITCH][ACTOR][{}][{}] Initiating actor shutdown sequence.",
            self.channel_name,
            self.actor_id
        );
        self.signal_irc_task_shutdown();
        self.await_irc_task_completion().await;
        update_channel_status(
            &self.channel_name,
            self.actor_id,
            &self.status_tx,
            TwitchChannelConnectionStatus::Terminated,
        );
        self.receiver.close();
    }

    fn start_irc_connection_task(&mut self) {
        // Check if there's an existing handle and if the task it points to is still running.
        if let Some(handle) = &self.irc_connection_task_handle {
            if !handle.is_finished() {
                tracing::warn!(
                "[TWITCH][ACTOR][{}][{}] Attempted to start IRC task, but an active (non-finished) one is already running.",
                self.channel_name,
                self.actor_id
            );
                return;
            }
            // If handle exists but task is finished, take it so we can replace it.
            tracing::debug!(
            "[TWITCH][ACTOR][{}][{}] Existing IRC task handle is for a finished task. Clearing it.",
            self.channel_name, self.actor_id
        );
            self.irc_connection_task_handle.take();
        }
        // If self.irc_connection_task_handle was None, or was Some but finished, it's now None.

        tracing::info!(
            "[TWITCH][ACTOR][{}][{}] Starting new IRC connection task.",
            self.channel_name,
            self.actor_id
        );

        let (irc_shutdown_tx, irc_shutdown_rx) = oneshot::channel();
        self.irc_task_shutdown_tx = Some(irc_shutdown_tx);

        let channel_name_clone = self.channel_name.clone();
        let oauth_token_clone = Arc::clone(&self.oauth_token);
        let actor_message_tx_clone = self.self_sender_for_irc_task.clone();
        let actor_id_clone = self.actor_id;

        let irc_task = tokio::spawn(run_irc_connection_loop(
            channel_name_clone,
            oauth_token_clone,
            actor_message_tx_clone,
            irc_shutdown_rx,
            actor_id_clone,
        ));
        self.irc_connection_task_handle = Some(irc_task);
    }

    fn signal_irc_task_shutdown(&mut self) {
        if let Some(shutdown_tx) = self.irc_task_shutdown_tx.take() {
            tracing::debug!(
                "[TWITCH][ACTOR][{}][{}] Sending shutdown signal to IRC task.",
                self.channel_name,
                self.actor_id
            );
            let _ = shutdown_tx.send(());
        }
    }

    async fn await_irc_task_completion(&mut self) {
        if let Some(handle) = self.irc_connection_task_handle.take() {
            tracing::info!(
                "[TWITCH][ACTOR][{}][{}] Waiting for IRC task to complete...",
                self.channel_name,
                self.actor_id
            );
            if let Err(e) = handle.await {
                tracing::error!(
                    "[TWITCH][ACTOR][{}][{}] IRC task panicked or was cancelled: {:?}",
                    self.channel_name,
                    self.actor_id,
                    e
                );
            } else {
                tracing::info!(
                    "[TWITCH][ACTOR][{}][{}] IRC task completed.",
                    self.channel_name,
                    self.actor_id
                );
            }
        }
        let current_status = self.status_tx.borrow().clone();
        if current_status != TwitchChannelConnectionStatus::Terminated
            && !matches!(
                current_status,
                TwitchChannelConnectionStatus::Disconnected { .. }
            )
        {
            update_channel_status(
                &self.channel_name,
                self.actor_id,
                &self.status_tx,
                TwitchChannelConnectionStatus::Disconnected {
                    reason: "IRC task stopped or completed".to_string(),
                },
            );
        }
    }
}

pub async fn run_twitch_channel_actor(mut actor: TwitchChannelActor) {
    tracing::info!(
        "[TWITCH][ACTOR][{}][{}] Actor started.",
        actor.channel_name,
        actor.actor_id
    );

    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }

    actor.signal_irc_task_shutdown();
    actor.await_irc_task_completion().await;

    tracing::info!(
        "[TWITCH][ACTOR][{}][{}] Actor stopped.",
        actor.channel_name,
        actor.actor_id
    );

    if let Err(e) = actor
        .manager_handle
        .notify_channel_actor_terminated(actor.channel_name.clone(), actor.actor_id)
        .await
    {
        tracing::error!(
            "[TWITCH][ACTOR][{}][{}] Failed to notify manager of shutdown: {:?}",
            actor.channel_name,
            actor.actor_id,
            e
        );
    }
}

// --- IRC Connection Logic ---
async fn run_irc_connection_loop(
    channel_name: String,
    oauth_token: Arc<String>,
    actor_tx: mpsc::Sender<TwitchChannelActorMessage>,
    mut shutdown_rx: oneshot::Receiver<()>,
    actor_id_for_logging: Uuid,
) {
    tracing::info!(
        "[TWITCH][IRC_LOOP][{}][{}] IRC connection management task started.",
        channel_name,
        actor_id_for_logging
    );
    let mut reconnect_attempts = 0u32;

    loop {
        reconnect_attempts += 1;
        if actor_tx
            .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                new_status: TwitchChannelConnectionStatus::Connecting {
                    attempt: reconnect_attempts,
                },
            })
            .await
            .is_err()
        {
            tracing::error!("[TWITCH][IRC_LOOP][{}][{}] Actor channel closed (Connecting). IRC loop shutting down.", channel_name, actor_id_for_logging);
            return;
        }

        let connection_result = tokio::select! {
            biased;
            _ = &mut shutdown_rx => {
                tracing::info!("[TWITCH][IRC_LOOP][{}][{}] Shutdown signal received. Terminating connection attempt.", channel_name, actor_id_for_logging);
                let _ = actor_tx.send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                    new_status: TwitchChannelConnectionStatus::Disconnected { reason: "Shutdown signal received".to_string() }
                }).await; // Attempt to notify actor
                return;
            }
            res = connect_and_listen_irc_single_attempt_adapted(
                channel_name.clone(),
                oauth_token.to_string(),
                actor_tx.clone(),
                reconnect_attempts,
                actor_id_for_logging,
            ) => res,
        };

        let (reason_for_disconnect, delay_seconds, should_terminate_loop) = match connection_result
        {
            Ok(_) => {
                tracing::info!("[TWITCH][IRC_LOOP][{}][{}] Connection closed/ended gracefully. Will attempt to reconnect.", channel_name, actor_id_for_logging);
                reconnect_attempts = 0;
                ("Graceful disconnect or RECONNECT".to_string(), 5u64, false)
            }
            Err(e) => {
                tracing::error!(
                    "[TWITCH][IRC_LOOP][{}][{}] Connection attempt {} failed: {}",
                    channel_name,
                    actor_id_for_logging,
                    reconnect_attempts,
                    e
                );
                match e {
                    TwitchError::TwitchAuth(_) => {
                        // For auth errors, send specific disconnected status and terminate loop. Actor will handle its own shutdown.
                        let _ = actor_tx
                            .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                                new_status: TwitchChannelConnectionStatus::Disconnected {
                                    reason: format!("Persistent Auth Failure: {}", e),
                                },
                            })
                            .await;
                        tracing::error!("[TWITCH][IRC_LOOP][{}][{}] Persistent authentication failure. IRC loop will terminate.", channel_name, actor_id_for_logging);
                        (e.to_string(), 0u64, true)
                    }
                    TwitchError::IrcTaskSendError(_) => {
                        tracing::error!("[TWITCH][IRC_LOOP][{}][{}] Failed to send to actor. IRC loop shutting down.", channel_name, actor_id_for_logging);
                        // No need to send status, actor is already gone.
                        (e.to_string(), 0u64, true)
                    }
                    _ => {
                        // Other errors, try to reconnect with backoff
                        let base_delay = 5u64;
                        let backoff_delay =
                            base_delay * 2u64.pow(reconnect_attempts.saturating_sub(1).min(6));
                        (e.to_string(), u64::min(backoff_delay, 300), false)
                    }
                }
            }
        };

        if should_terminate_loop {
            return; // Exit the IRC connection loop
        }

        let reconnect_delay = Duration::from_secs(delay_seconds);
        if actor_tx
            .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                new_status: TwitchChannelConnectionStatus::Reconnecting {
                    reason: reason_for_disconnect,
                    failed_attempt: reconnect_attempts,
                    retry_in: reconnect_delay,
                },
            })
            .await
            .is_err()
        {
            tracing::error!("[TWITCH][IRC_LOOP][{}][{}] Actor channel closed (Reconnecting). IRC loop shutting down.", channel_name, actor_id_for_logging);
            return;
        }

        tokio::select! {
            biased;
            _ = &mut shutdown_rx => {
                tracing::info!("[TWITCH][IRC_LOOP][{}][{}] Shutdown signal received during reconnect delay. Terminating.", channel_name, actor_id_for_logging);
                 let _ = actor_tx.send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                    new_status: TwitchChannelConnectionStatus::Disconnected { reason: "Shutdown signal received".to_string() }
                }).await;
                return;
            }
            _ = tokio::time::sleep(reconnect_delay) => { /* Continue to next attempt */ }
        }
    }
}

async fn connect_and_listen_irc_single_attempt_adapted(
    channel_name: String,
    oauth_token: String,
    actor_tx: mpsc::Sender<TwitchChannelActorMessage>,
    connection_attempt_count: u32,
    actor_id_for_logging: Uuid,
) -> Result<()> {
    let host = "irc.chat.twitch.tv";
    let port = 6667;
    let addr = format!("{}:{}", host, port);
    let bot_nickname = format!("justinfan{}", rand::random::<u32>() % 80000 + 1000);

    if actor_tx
        .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
            new_status: TwitchChannelConnectionStatus::Authenticating {
                attempt: connection_attempt_count,
            },
        })
        .await
        .is_err()
    {
        return Err(TwitchError::IrcTaskSendError(
            "Actor channel closed (Authenticating)".to_string(),
        ));
    }

    tracing::info!(
        "[TWITCH][IRC_CONNECT][{}][{}] Attempt {}: Connecting to {} as {}...",
        channel_name,
        actor_id_for_logging,
        connection_attempt_count,
        addr,
        bot_nickname
    );

    let stream = TcpStream::connect(&addr).await.map_err(|e| {
        tracing::error!(
            "[TWITCH][IRC_CONNECT][{}][{}] TCP connection failed: {}",
            channel_name,
            actor_id_for_logging,
            e
        );
        TwitchError::Io(e)
    })?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);

    tracing::info!(
        "[TWITCH][IRC_CONNECT][{}][{}] TCP connected. Requesting capabilities and authenticating...",
        channel_name, actor_id_for_logging
    );

    writer
        .write_all(b"CAP REQ :twitch.tv/membership twitch.tv/tags twitch.tv/commands\r\n")
        .await?;
    writer
        .write_all(format!("PASS oauth:{}\r\n", oauth_token).as_bytes())
        .await?;
    writer
        .write_all(format!("NICK {}\r\n", bot_nickname).as_bytes())
        .await?;
    writer.flush().await?;

    let mut line_buffer = String::new();

    loop {
        line_buffer.clear();
        match tokio::time::timeout(
            Duration::from_secs(360),
            buf_reader.read_line(&mut line_buffer),
        )
        .await
        {
            Ok(Ok(0)) => {
                tracing::info!(
                    "[TWITCH][IRC_READ][{}][{}] Connection closed by Twitch (EOF).",
                    channel_name,
                    actor_id_for_logging
                );
                return Ok(());
            }
            Ok(Ok(_)) => { /* Process line */ }
            Ok(Err(e)) => {
                tracing::error!(
                    "[TWITCH][IRC_READ][{}][{}] Error reading from chat: {}",
                    channel_name,
                    actor_id_for_logging,
                    e
                );
                return Err(TwitchError::Io(e));
            }
            Err(_) => {
                tracing::error!(
                    "[TWITCH][IRC_READ][{}][{}] Timeout reading from socket. Closing connection.",
                    channel_name,
                    actor_id_for_logging
                );
                return Err(TwitchError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Read timeout",
                )));
            }
        }

        let message_line_owned = line_buffer.clone();

        if !message_line_owned.trim().is_empty() {
            if actor_tx
                .send(TwitchChannelActorMessage::InternalIrcLineReceived {
                    line: message_line_owned.clone(),
                })
                .await
                .is_err()
            {
                return Err(TwitchError::IrcTaskSendError(
                    "Actor channel closed (InternalIrcLineReceived)".to_string(),
                ));
            }
        }

        let parsed_for_task_logic = IrcMessage::parse(&message_line_owned);

        match parsed_for_task_logic.command {
            Some("PING") => {
                let pong_target = parsed_for_task_logic
                    .params
                    .get(0)
                    .unwrap_or(&":tmi.twitch.tv");
                writer
                    .write_all(format!("PONG {}\r\n", pong_target).as_bytes())
                    .await
                    .map_err(TwitchError::Io)?;
                writer.flush().await.map_err(TwitchError::Io)?;
            }
            Some("001") => {
                tracing::info!(
                    "[TWITCH][IRC_AUTH][{}][{}] Authenticated successfully (RPL_WELCOME).",
                    channel_name,
                    actor_id_for_logging
                );
                if actor_tx
                    .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                        new_status: TwitchChannelConnectionStatus::Connected,
                    })
                    .await
                    .is_err()
                {
                    return Err(TwitchError::IrcTaskSendError(
                        "Actor channel closed (Connected)".to_string(),
                    ));
                }
                writer
                    .write_all(format!("JOIN #{}\r\n", channel_name.to_lowercase()).as_bytes())
                    .await
                    .map_err(TwitchError::Io)?;
                writer.flush().await.map_err(TwitchError::Io)?;
            }
            Some("JOIN") => {
                let joining_user = parsed_for_task_logic
                    .get_prefix_username()
                    .unwrap_or_default();
                let joined_chan = parsed_for_task_logic
                    .params
                    .get(0)
                    .map(|s| s.trim_start_matches('#'))
                    .unwrap_or_default();
                if joined_chan.eq_ignore_ascii_case(&channel_name)
                    && joining_user.eq_ignore_ascii_case(&bot_nickname)
                {
                    tracing::info!(
                        "[TWITCH][IRC_JOIN][{}][{}] Successfully JOINED #{} as {}",
                        channel_name,
                        actor_id_for_logging,
                        joined_chan,
                        bot_nickname
                    );
                }
            }
            Some("NOTICE") => {
                let notice_text = parsed_for_task_logic
                    .get_privmsg_text_content()
                    .or_else(|| parsed_for_task_logic.params.get(1).map(|v| &**v))
                    .unwrap_or_default();

                if notice_text.contains("Login authentication failed")
                    || notice_text.contains("Improperly formatted auth")
                {
                    tracing::error!(
                        "[TWITCH][IRC_AUTH_FAIL][{}][{}] Authentication failed via NOTICE: {}",
                        channel_name,
                        actor_id_for_logging,
                        notice_text
                    );
                    return Err(TwitchError::TwitchAuth(format!(
                        "Authentication failed: {}",
                        notice_text
                    )));
                }
            }
            Some("RECONNECT") => {
                tracing::info!(
                    "[TWITCH][IRC_RECONNECT][{}][{}] Received RECONNECT command.",
                    channel_name,
                    actor_id_for_logging
                );
                return Ok(());
            }
            Some("CAP") => {
                let ack_type = parsed_for_task_logic
                    .params
                    .get(1)
                    .copied()
                    .unwrap_or_default();
                let capabilities = parsed_for_task_logic
                    .params
                    .get(2)
                    .copied()
                    .unwrap_or_default();
                if ack_type == "NAK" {
                    tracing::error!("[TWITCH][IRC_CAP_NAK][{}][{}] Capability NAK: {}. This could affect functionality.", channel_name, actor_id_for_logging, capabilities);
                } else if ack_type == "ACK" {
                    tracing::info!(
                        "[TWITCH][IRC_CAP_ACK][{}][{}] Capability ACK: {}",
                        channel_name,
                        actor_id_for_logging,
                        capabilities
                    );
                }
            }
            _ => { /* Other messages are parsed by the actor */ }
        }
    }
}

// --- Standalone function to update status and log ---
fn update_channel_status(
    channel_name: &str,
    actor_id: Uuid,
    status_tx: &watch::Sender<TwitchChannelConnectionStatus>,
    new_status: TwitchChannelConnectionStatus,
) {
    if *status_tx.borrow() == new_status
        && new_status != TwitchChannelConnectionStatus::Initializing
    {
        return;
    }
    tracing::info!(
        "[TWITCH][STATUS][{}][{}] New status: {:?}",
        channel_name,
        actor_id,
        new_status
    );
    if status_tx.send(new_status).is_err() {
        tracing::error!(
            "[TWITCH][CRITICAL][{}][{}] Failed to update channel status, receiver dropped.",
            channel_name,
            actor_id
        );
    }
}
