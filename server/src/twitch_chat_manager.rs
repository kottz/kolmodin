use crate::twitch_integration::{
    ParsedTwitchMessage,
    TwitchChannelActorHandle,
    TwitchChannelConnectionStatus,
    TwitchError,
    // We need PlaceholderTwitchChatManagerActorHandle to pass to TwitchChannelActorHandle::new
    // This circular dependency is tricky. Let's define TwitchChatManagerActorHandle first.
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch}; // watch might be useful if manager exposes aggregated status
use uuid::Uuid;

// --- TwitchChatManagerActor Messages ---
#[derive(Debug)]
pub enum TwitchChatManagerMessage {
    SubscribeToChannel {
        channel_name: String,
        lobby_id: Uuid, // Identifier for the subscriber (e.g., lobby's ID)
        // Sender part of an MPSC channel for the LobbyActor to receive Twitch messages
        twitch_message_tx_for_lobby: mpsc::Sender<ParsedTwitchMessage>,
        // Oneshot sender to respond with success/failure or the status receiver
        respond_to:
            oneshot::Sender<Result<watch::Receiver<TwitchChannelConnectionStatus>, TwitchError>>,
    },
    UnsubscribeFromChannel {
        channel_name: String,
        lobby_id: Uuid, // Identifier for the subscriber to remove
        respond_to: oneshot::Sender<Result<(), TwitchError>>,
    },
    // Internal message from a TwitchChannelActor when it has fully shut down
    ChannelActorTerminated {
        channel_name: String,
        actor_id: Uuid, // For logging/verification
    },
    // Maybe a message to get status of a specific channel actor?
    // GetChannelStatus {
    //     channel_name: String,
    //     respond_to: oneshot::Sender<Option<watch::Receiver<TwitchChannelConnectionStatus>>>,
    // }
}

// --- TwitchChatManagerActor Handle ---
#[derive(Clone, Debug)]
pub struct TwitchChatManagerActorHandle {
    sender: mpsc::Sender<TwitchChatManagerMessage>,
}

impl TwitchChatManagerActorHandle {
    pub fn new(
        app_oauth_token: Arc<String>, // Global Twitch App Access Token
        manager_buffer_size: usize,
        channel_actor_buffer_size: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(manager_buffer_size);
        let actor = TwitchChatManagerActor::new(
            receiver,
            app_oauth_token,
            sender.clone(), // Pass a clone of the sender for self-referential calls if needed (e.g. for children)
            channel_actor_buffer_size,
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

    // This method would be used by TwitchChannelActor to notify the manager of its shutdown.
    // It's part of the TwitchChatManagerActorHandle's public API for its children.
    pub async fn notify_channel_actor_terminated(
        &self,
        channel_name: String,
        actor_id: Uuid,
    ) -> Result<(), TwitchError> {
        self.sender
            .send(TwitchChatManagerMessage::ChannelActorTerminated {
                channel_name,
                actor_id,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!(
                    "Failed to send ChannelActorTerminated to manager: {}",
                    e
                ))
            })
    }
}

// --- TwitchChatManagerActor Struct ---
struct TwitchChatManagerActor {
    receiver: mpsc::Receiver<TwitchChatManagerMessage>,
    // Map of channel names (lowercase) to their active actor handles
    active_channels: HashMap<String, TwitchChannelActorHandle>,
    app_oauth_token: Arc<String>,
    // Handle to itself, primarily for TwitchChannelActors to notify back.
    // TwitchChannelActorHandle::new needs a manager handle.
    self_handle_for_children: TwitchChatManagerActorHandle,
    channel_actor_buffer_size: usize,
}

impl TwitchChatManagerActor {
    fn new(
        receiver: mpsc::Receiver<TwitchChatManagerMessage>,
        app_oauth_token: Arc<String>,
        self_sender: mpsc::Sender<TwitchChatManagerMessage>, // Used to construct self_handle_for_children
        channel_actor_buffer_size: usize,
    ) -> Self {
        Self {
            receiver,
            active_channels: HashMap::new(),
            app_oauth_token,
            self_handle_for_children: TwitchChatManagerActorHandle {
                sender: self_sender,
            },
            channel_actor_buffer_size,
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
                    normalized_channel_name, lobby_id
                );

                let channel_actor_handle = if let Some(existing_handle) =
                    self.active_channels.get(&normalized_channel_name)
                {
                    tracing::debug!(
                        "[TWITCH_MANAGER] Found existing TwitchChannelActor for '{}'. Reusing.",
                        normalized_channel_name
                    );
                    existing_handle.clone()
                } else {
                    tracing::info!(
                        "[TWITCH_MANAGER] No existing TwitchChannelActor for '{}'. Creating new one.",
                        normalized_channel_name
                    );
                    // Pass the manager's *own* handle (self_handle_for_children) to the new ChannelActor
                    let new_handle = TwitchChannelActorHandle::new(
                        normalized_channel_name.clone(),
                        Arc::clone(&self.app_oauth_token),
                        self.self_handle_for_children.clone(), // This is the key part
                        self.channel_actor_buffer_size,
                    );
                    self.active_channels
                        .insert(normalized_channel_name.clone(), new_handle.clone());
                    new_handle
                };

                // Now subscribe the lobby to this channel actor
                match channel_actor_handle
                    .add_subscriber(lobby_id, twitch_message_tx_for_lobby)
                    .await
                {
                    Ok(_) => {
                        tracing::info!(
                            "[TWITCH_MANAGER] Successfully subscribed lobby '{}' to channel '{}'",
                            lobby_id,
                            normalized_channel_name
                        );
                        // Respond with the status receiver from the channel actor
                        let _ = respond_to.send(Ok(channel_actor_handle.get_status_receiver()));
                    }
                    Err(e) => {
                        tracing::error!(
                            "[TWITCH_MANAGER] Failed to subscribe lobby '{}' to channel '{}': {:?}",
                            lobby_id,
                            normalized_channel_name,
                            e
                        );
                        // If adding subscriber fails, and this was a newly created channel_actor,
                        // we might want to remove it from active_channels if it has no other subscribers.
                        // However, add_subscriber itself doesn't tell us if it was the *first* sub.
                        // The ChannelActor manages its own lifecycle based on subscriber count.
                        let _ = respond_to.send(Err(e));
                    }
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
                    normalized_channel_name, lobby_id
                );

                if let Some(channel_actor_handle) =
                    self.active_channels.get(&normalized_channel_name)
                {
                    match channel_actor_handle.remove_subscriber(lobby_id).await {
                        Ok(_) => {
                            tracing::info!(
                                "[TWITCH_MANAGER] Successfully unsubscribed lobby '{}' from channel '{}'",
                                lobby_id, normalized_channel_name
                            );
                            let _ = respond_to.send(Ok(()));
                            // Note: We don't remove the channel_actor_handle from active_channels here.
                            // The TwitchChannelActor itself will shut down its IRC if it has no subscribers,
                            // and then it will notify the manager via ChannelActorTerminated.
                        }
                        Err(e) => {
                            tracing::error!(
                                "[TWITCH_MANAGER] Failed to unsubscribe lobby '{}' from channel '{}': {:?}",
                                lobby_id, normalized_channel_name, e
                            );
                            let _ = respond_to.send(Err(e));
                        }
                    }
                } else {
                    tracing::warn!(
                        "[TWITCH_MANAGER] Cannot unsubscribe lobby '{}': Channel '{}' not found or not active.",
                        lobby_id, normalized_channel_name
                    );
                    let _ = respond_to.send(Err(TwitchError::ChannelActorTerminated(
                        normalized_channel_name,
                    )));
                }
            }

            TwitchChatManagerMessage::ChannelActorTerminated {
                channel_name,
                actor_id,
            } => {
                let normalized_channel_name = channel_name.to_lowercase();
                tracing::info!(
                    "[TWITCH_MANAGER] TwitchChannelActor for '{}' (ID: {}) reported termination. Removing from active list.",
                    normalized_channel_name, actor_id
                );
                // We could verify the actor_id if we stored it, but channel_name is usually sufficient.
                if self
                    .active_channels
                    .remove(&normalized_channel_name)
                    .is_some()
                {
                    tracing::debug!(
                        "[TWITCH_MANAGER] Removed handle for terminated channel '{}'.",
                        normalized_channel_name
                    );
                } else {
                    tracing::warn!("[TWITCH_MANAGER] Received termination notice for unknown or already removed channel '{}'.", normalized_channel_name);
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
