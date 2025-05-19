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
                    normalized_channel_name,
                    lobby_id
                );

                let mut create_new_actor = true;
                let mut obtained_actor_handle: Option<TwitchChannelActorHandle> = None;

                if let Some(existing_handle) = self.active_channels.get(&normalized_channel_name) {
                    // Check the status of the existing actor.
                    // .borrow() gets a reference to the current value in the watch channel.
                    let current_status = existing_handle.get_status_receiver().borrow().clone();

                    if matches!(current_status, TwitchChannelConnectionStatus::Terminated) {
                        tracing::info!(
                            "[TWITCH_MANAGER] Existing TwitchChannelActor for '{}' is Terminated. Removing stale handle and will create a new one.",
                            normalized_channel_name
                        );
                        // The actor is terminated, so its handle is stale. Remove it.
                        self.active_channels.remove(&normalized_channel_name);
                        // create_new_actor remains true, so a new actor will be made.
                    } else {
                        tracing::debug!(
                            "[TWITCH_MANAGER] Found existing and non-Terminated TwitchChannelActor for '{}' (Status: {:?}). Reusing.",
                            normalized_channel_name,
                            current_status
                        );
                        obtained_actor_handle = Some(existing_handle.clone());
                        create_new_actor = false;
                    }
                }
                // If no existing handle was found, create_new_actor is still true.

                if create_new_actor {
                    tracing::info!(
                        "[TWITCH_MANAGER] Creating new TwitchChannelActor for '{}'.",
                        normalized_channel_name
                    );
                    let new_handle = TwitchChannelActorHandle::new(
                        normalized_channel_name.clone(),
                        Arc::clone(&self.app_oauth_token),
                        self.self_handle_for_children.clone(), // Manager's handle for the child
                        self.channel_actor_buffer_size,
                    );
                    // Store the new handle. If a previous stale handle was removed, this replaces it.
                    // If there was no handle, this inserts it.
                    self.active_channels
                        .insert(normalized_channel_name.clone(), new_handle.clone());
                    obtained_actor_handle = Some(new_handle);
                }

                // At this point, obtained_actor_handle should always be Some.
                // If it's not, it's a logic error in the above block.
                if let Some(final_actor_handle) = obtained_actor_handle {
                    // Now subscribe the lobby to this channel actor (either new or reused)
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
                            // Respond with the status receiver from the channel actor
                            let _ = respond_to.send(Ok(final_actor_handle.get_status_receiver()));
                        }
                        Err(e) => {
                            tracing::error!(
                                "[TWITCH_MANAGER] Failed to subscribe lobby '{}' to channel '{}': {:?}",
                                lobby_id,
                                normalized_channel_name,
                                e
                            );
                            // If adding subscriber fails, and this was a newly created channel_actor,
                            // the TwitchChannelActor itself should manage its lifecycle if it ends up
                            // with no subscribers or if its internal startup fails.
                            // The manager's primary role here is to report the error for this specific subscription.
                            let _ = respond_to.send(Err(e));
                        }
                    }
                } else {
                    // This case should ideally not be reached if the logic above is correct.
                    let error_msg = format!(
                        "Internal error: Failed to obtain or create actor handle for channel '{}'",
                        normalized_channel_name
                    );
                    tracing::error!("[TWITCH_MANAGER] {}", error_msg);
                    let _ = respond_to.send(Err(TwitchError::InternalActorError(error_msg)));
                }
            }

            // ... (other match arms for TwitchChatManagerMessage: UnsubscribeFromChannel, ChannelActorTerminated) ...
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

                if let Some(channel_actor_handle) =
                    self.active_channels.get(&normalized_channel_name)
                {
                    match channel_actor_handle.remove_subscriber(lobby_id).await {
                        Ok(_) => {
                            tracing::info!(
                                "[TWITCH_MANAGER] Successfully unsubscribed lobby '{}' from channel '{}'",
                                lobby_id,
                                normalized_channel_name
                            );
                            let _ = respond_to.send(Ok(()));
                            // Note: We don't remove the channel_actor_handle from active_channels here.
                            // The TwitchChannelActor itself will shut down its IRC if it has no subscribers,
                            // and then it will notify the manager via ChannelActorTerminated.
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
                        // Or a more specific "ChannelNotFound" error
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
                    "[TWITCH_MANAGER] TwitchChannelActor for '{}' (ID: {}) reported termination. Checking if it should be removed from active list.",
                    normalized_channel_name,
                    actor_id
                );

                // Only remove if the current handle (if any) matches the actor_id of the one terminating
                // and/or if its status is indeed Terminated. This prevents accidentally removing a *new* actor
                // for the same channel if a termination message for an *old* one arrives late.
                if let Some(active_handle) = self.active_channels.get(&normalized_channel_name) {
                    // To be absolutely sure, you could store the actor_id in TwitchChannelActorHandle
                    // or rely on the status. For simplicity here, we'll check status.
                    // A more robust check would involve comparing actor_id if you store it.
                    let current_status = active_handle.get_status_receiver().borrow().clone();
                    if matches!(current_status, TwitchChannelConnectionStatus::Terminated) {
                        // It's also good practice to check if the actor_id matches if you had access to it here
                        // from the active_handle. For now, we assume if status is Terminated, it's the one.
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
                            // This case should be rare if the get and remove are close.
                            tracing::warn!(
                                "[TWITCH_MANAGER] Tried to remove terminated channel '{}' but it was already gone.",
                                normalized_channel_name
                            );
                        }
                    } else {
                        tracing::warn!(
                            "[TWITCH_MANAGER] Received termination notice for channel '{}' (ID: {}), but current active actor for this channel is not Terminated (Status: {:?}). Ignoring this potentially stale termination notice.",
                            normalized_channel_name,
                            actor_id,
                            current_status
                        );
                    }
                } else {
                    tracing::warn!(
                        "[TWITCH_MANAGER] Received termination notice for unknown or already removed channel '{}' (ID: {}).",
                        normalized_channel_name,
                        actor_id
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
