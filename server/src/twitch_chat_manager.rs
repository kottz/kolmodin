use crate::twitch_integration::{
    ChannelTerminationInfo, ParsedTwitchMessage, TwitchChannelActorHandle,
    TwitchChannelConnectionStatus, TwitchError,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

// --- TwitchChatManagerActor Messages ---
#[derive(Debug)]
pub enum TwitchChatManagerMessage {
    SubscribeToChannel {
        channel_name: String,
        lobby_id: Uuid,
        twitch_message_tx_for_lobby: mpsc::Sender<ParsedTwitchMessage>,
        respond_to:
            oneshot::Sender<Result<watch::Receiver<TwitchChannelConnectionStatus>, TwitchError>>,
    },
    UnsubscribeFromChannel {
        channel_name: String,
        lobby_id: Uuid,
        respond_to: oneshot::Sender<Result<(), TwitchError>>,
    },
    // NEW: Replaces ChannelActorTerminated
    ChannelActorCompleted {
        channel_name: String,
        termination_info: ChannelTerminationInfo,
    },
}

// --- TwitchChatManagerActor Handle ---
#[derive(Clone, Debug)]
pub struct TwitchChatManagerActorHandle {
    sender: mpsc::Sender<TwitchChatManagerMessage>,
}

impl TwitchChatManagerActorHandle {
    pub fn new(
        app_oauth_token: Arc<String>,
        manager_buffer_size: usize,
        channel_actor_buffer_size: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(manager_buffer_size);
        let actor = TwitchChatManagerActor::new(
            receiver,
            app_oauth_token,
            channel_actor_buffer_size,
            sender.clone(), // Pass sender for monitor tasks
        );
        tokio::spawn(run_twitch_chat_manager_actor(actor));
        Self { sender }
    }

    pub async fn subscribe_to_channel(
        &self,
        channel_name: String,
        lobby_id: Uuid,
        twitch_message_tx_for_lobby: mpsc::Sender<ParsedTwitchMessage>,
    ) -> Result<watch::Receiver<TwitchChannelConnectionStatus>, TwitchError> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchChatManagerMessage::SubscribeToChannel {
                channel_name,
                lobby_id,
                twitch_message_tx_for_lobby,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!(
                    "Failed to send SubscribeToChannel to manager: {}",
                    e
                ))
            })?;

        respond_to_rx.await.map_err(|e| {
            TwitchError::ActorComm(format!(
                "Manager failed to respond to SubscribeToChannel: {}",
                e
            ))
        })?
    }

    pub async fn unsubscribe_from_channel(
        &self,
        channel_name: String,
        lobby_id: Uuid,
    ) -> Result<(), TwitchError> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchChatManagerMessage::UnsubscribeFromChannel {
                channel_name,
                lobby_id,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!(
                    "Failed to send UnsubscribeFromChannel to manager: {}",
                    e
                ))
            })?;

        respond_to_rx.await.map_err(|e| {
            TwitchError::ActorComm(format!(
                "Manager failed to respond to UnsubscribeFromChannel: {}",
                e
            ))
        })?
    }

    // REMOVED: notify_channel_actor_terminated method
}

// --- TwitchChatManagerActor Struct ---
struct ChannelActorState {
    handle: TwitchChannelActorHandle,
    // REMOVED: join_handle field
}

struct TwitchChatManagerActor {
    receiver: mpsc::Receiver<TwitchChatManagerMessage>,
    active_channels: HashMap<String, ChannelActorState>,
    app_oauth_token: Arc<String>,
    channel_actor_buffer_size: usize,
    self_sender: mpsc::Sender<TwitchChatManagerMessage>, // For spawning monitor tasks
}

impl TwitchChatManagerActor {
    fn new(
        receiver: mpsc::Receiver<TwitchChatManagerMessage>,
        app_oauth_token: Arc<String>,
        channel_actor_buffer_size: usize,
        self_sender: mpsc::Sender<TwitchChatManagerMessage>,
    ) -> Self {
        Self {
            receiver,
            active_channels: HashMap::new(),
            app_oauth_token,
            channel_actor_buffer_size,
            self_sender,
        }
    }

    async fn handle_message(&mut self, msg: TwitchChatManagerMessage) {
        match msg {
            TwitchChatManagerMessage::SubscribeToChannel {
                channel_name,
                lobby_id,
                twitch_message_tx_for_lobby,
                respond_to,
            } => {
                let normalized_channel_name = channel_name.to_lowercase();
                tracing::info!(
                    "[TWITCH_MANAGER] Received subscription request for channel '{}' from lobby '{}'",
                    normalized_channel_name,
                    lobby_id
                );

                let mut create_new_actor = true;
                let mut obtained_actor_handle: Option<TwitchChannelActorHandle> = None;

                if let Some(existing_state) = self.active_channels.get(&normalized_channel_name) {
                    let current_status =
                        existing_state.handle.get_status_receiver().borrow().clone();

                    if matches!(current_status, TwitchChannelConnectionStatus::Terminated) {
                        tracing::info!(
                            "[TWITCH_MANAGER] Existing TwitchChannelActor for '{}' is Terminated. Removing stale handle and will create a new one.",
                            normalized_channel_name
                        );
                        self.active_channels.remove(&normalized_channel_name);
                    } else {
                        tracing::debug!(
                            "[TWITCH_MANAGER] Found existing and non-Terminated TwitchChannelActor for '{}' (Status: {:?}). Reusing.",
                            normalized_channel_name,
                            current_status
                        );
                        obtained_actor_handle = Some(existing_state.handle.clone());
                        create_new_actor = false;
                    }
                }

                if create_new_actor {
                    tracing::info!(
                        "[TWITCH_MANAGER] Creating new TwitchChannelActor for '{}'.",
                        normalized_channel_name
                    );

                    // NEW: Get both handle and JoinHandle
                    let (new_handle, join_handle) = TwitchChannelActorHandle::new(
                        normalized_channel_name.clone(),
                        Arc::clone(&self.app_oauth_token),
                        self.channel_actor_buffer_size,
                    );

                    // Store just the handle
                    self.active_channels.insert(
                        normalized_channel_name.clone(),
                        ChannelActorState {
                            handle: new_handle.clone(),
                        },
                    );

                    // NEW: Spawn monitor task for the JoinHandle
                    let manager_tx = self.self_sender.clone();
                    let channel_name_for_monitor = normalized_channel_name.clone();
                    tokio::spawn(async move {
                        match join_handle.await {
                            Ok(termination_info) => {
                                let _ = manager_tx
                                    .send(TwitchChatManagerMessage::ChannelActorCompleted {
                                        channel_name: channel_name_for_monitor.clone(),
                                        termination_info,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                tracing::error!(
                                    "[TWITCH_MANAGER] Channel actor '{}' panicked: {:?}",
                                    channel_name_for_monitor,
                                    e
                                );
                                // Could send a specific panic message if needed
                            }
                        }
                    });

                    obtained_actor_handle = Some(new_handle);
                }

                if let Some(final_actor_handle) = obtained_actor_handle {
                    match final_actor_handle
                        .add_subscriber(lobby_id, twitch_message_tx_for_lobby)
                        .await
                    {
                        Ok(_) => {
                            tracing::info!(
                                "[TWITCH_MANAGER] Successfully subscribed lobby '{}' to channel '{}'",
                                lobby_id,
                                normalized_channel_name
                            );
                            let _ = respond_to.send(Ok(final_actor_handle.get_status_receiver()));
                        }
                        Err(e) => {
                            tracing::error!(
                                "[TWITCH_MANAGER] Failed to subscribe lobby '{}' to channel '{}': {:?}",
                                lobby_id,
                                normalized_channel_name,
                                e
                            );
                            let _ = respond_to.send(Err(e));
                        }
                    }
                } else {
                    let error_msg = format!(
                        "Internal error: Failed to obtain or create actor handle for channel '{}'",
                        normalized_channel_name
                    );
                    tracing::error!("[TWITCH_MANAGER] {}", error_msg);
                    let _ = respond_to.send(Err(TwitchError::InternalActorError(error_msg)));
                }
            }

            TwitchChatManagerMessage::UnsubscribeFromChannel {
                channel_name,
                lobby_id,
                respond_to,
            } => {
                let normalized_channel_name = channel_name.to_lowercase();
                tracing::info!(
                    "[TWITCH_MANAGER] Received unsubscribe request for channel '{}' from lobby '{}'",
                    normalized_channel_name,
                    lobby_id
                );

                if let Some(channel_state) = self.active_channels.get(&normalized_channel_name) {
                    match channel_state.handle.remove_subscriber(lobby_id).await {
                        Ok(_) => {
                            tracing::info!(
                                "[TWITCH_MANAGER] Successfully unsubscribed lobby '{}' from channel '{}'",
                                lobby_id,
                                normalized_channel_name
                            );
                            let _ = respond_to.send(Ok(()));
                        }
                        Err(e) => {
                            tracing::error!(
                                "[TWITCH_MANAGER] Failed to unsubscribe lobby '{}' from channel '{}': {:?}",
                                lobby_id,
                                normalized_channel_name,
                                e
                            );
                            let _ = respond_to.send(Err(e));
                        }
                    }
                } else {
                    tracing::warn!(
                        "[TWITCH_MANAGER] Cannot unsubscribe lobby '{}': Channel '{}' not found or not active.",
                        lobby_id,
                        normalized_channel_name
                    );
                    let _ = respond_to.send(Err(TwitchError::ChannelActorTerminated(
                        normalized_channel_name,
                    )));
                }
            }

            // NEW: Replaces ChannelActorTerminated
            TwitchChatManagerMessage::ChannelActorCompleted {
                channel_name,
                termination_info,
            } => {
                let normalized_channel_name = channel_name.to_lowercase();
                tracing::info!(
                    "[TWITCH_MANAGER] Channel actor '{}' (ID: {}) completed with status: {:?}",
                    normalized_channel_name,
                    termination_info.actor_id,
                    termination_info.final_status
                );

                // Simply remove from active channels
                if self
                    .active_channels
                    .remove(&normalized_channel_name)
                    .is_some()
                {
                    tracing::debug!(
                        "[TWITCH_MANAGER] Removed completed channel '{}'.",
                        normalized_channel_name
                    );
                } else {
                    tracing::warn!(
                        "[TWITCH_MANAGER] Channel '{}' was already removed from active channels.",
                        normalized_channel_name
                    );
                }
            }
        }
    }
}

// --- Run Function for the Actor ---
async fn run_twitch_chat_manager_actor(mut actor: TwitchChatManagerActor) {
    tracing::info!("[TWITCH_MANAGER] Twitch Chat Manager Actor started.");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }
    tracing::info!("[TWITCH_MANAGER] Twitch Chat Manager Actor stopped.");
}
