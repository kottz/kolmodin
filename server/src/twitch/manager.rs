use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot, watch};
use uuid::Uuid;

use super::auth::TokenProvider;
use super::channel::{ChannelActorState, TwitchChannelActorHandle};
use super::error::TwitchError;
use super::types::{ChannelTerminationInfo, ParsedTwitchMessage, TwitchChannelConnectionStatus};

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
    ChannelActorCompleted {
        channel_name: String,
        termination_info: ChannelTerminationInfo,
    },
}

#[derive(Clone, Debug)]
pub struct TwitchChatManagerActorHandle {
    sender: mpsc::Sender<TwitchChatManagerMessage>,
}

impl TwitchChatManagerActorHandle {
    pub fn new(
        token_provider: TokenProvider,
        manager_buffer_size: usize,
        channel_actor_buffer_size: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(manager_buffer_size);
        let actor = TwitchChatManagerActor::new(
            receiver,
            token_provider,
            channel_actor_buffer_size,
            sender.clone(),
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
}

pub struct TwitchChatManagerActor {
    receiver: mpsc::Receiver<TwitchChatManagerMessage>,
    active_channels: HashMap<String, ChannelActorState>,
    token_provider: TokenProvider,
    channel_actor_buffer_size: usize,
    self_sender: mpsc::Sender<TwitchChatManagerMessage>,
}

impl TwitchChatManagerActor {
    pub fn new(
        receiver: mpsc::Receiver<TwitchChatManagerMessage>,
        token_provider: TokenProvider,
        channel_actor_buffer_size: usize,
        self_sender: mpsc::Sender<TwitchChatManagerMessage>,
    ) -> Self {
        Self {
            receiver,
            active_channels: HashMap::new(),
            token_provider,
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
                self.handle_subscribe_to_channel(
                    channel_name,
                    lobby_id,
                    twitch_message_tx_for_lobby,
                    respond_to,
                )
                .await;
            }
            TwitchChatManagerMessage::UnsubscribeFromChannel {
                channel_name,
                lobby_id,
                respond_to,
            } => {
                self.handle_unsubscribe_from_channel(channel_name, lobby_id, respond_to)
                    .await;
            }
            TwitchChatManagerMessage::ChannelActorCompleted {
                channel_name,
                termination_info,
            } => {
                self.handle_channel_actor_completed(channel_name, termination_info)
                    .await;
            }
        }
    }

    async fn handle_subscribe_to_channel(
        &mut self,
        channel_name: String,
        lobby_id: Uuid,
        twitch_message_tx_for_lobby: mpsc::Sender<ParsedTwitchMessage>,
        respond_to: oneshot::Sender<
            Result<watch::Receiver<TwitchChannelConnectionStatus>, TwitchError>,
        >,
    ) {
        let normalized_channel_name = channel_name.to_lowercase();
        tracing::info!(
            channel.name = %normalized_channel_name,
            lobby.id = %lobby_id,
            "Received subscription request"
        );

        let mut create_new_actor = true;
        let mut obtained_actor_handle: Option<TwitchChannelActorHandle> = None;

        if let Some(existing_state) = self.active_channels.get(&normalized_channel_name) {
            let current_status = existing_state.handle.get_status_receiver().borrow().clone();

            if matches!(current_status, TwitchChannelConnectionStatus::Terminated) {
                tracing::info!(
                    channel.name = %normalized_channel_name,
                    "Removing terminated channel actor, will create new one"
                );
                self.active_channels.remove(&normalized_channel_name);
            } else {
                tracing::debug!(
                    channel.name = %normalized_channel_name,
                    channel.status = ?current_status,
                    "Reusing existing non-terminated channel actor"
                );
                obtained_actor_handle = Some(existing_state.handle.clone());
                create_new_actor = false;
            }
        }

        if create_new_actor {
            tracing::info!(
                channel.name = %normalized_channel_name,
                "Creating new TwitchChannelActor"
            );

            let (new_handle, join_handle) = TwitchChannelActorHandle::new(
                normalized_channel_name.clone(),
                self.token_provider.clone(),
                self.channel_actor_buffer_size,
            );

            self.active_channels.insert(
                normalized_channel_name.clone(),
                ChannelActorState {
                    handle: new_handle.clone(),
                },
            );

            // Spawn monitoring task for the channel actor's lifecycle
            let manager_sender = self.self_sender.clone();
            let channel_name_for_monitoring = normalized_channel_name.clone();
            tokio::spawn(async move {
                let termination_info = join_handle.await;
                match termination_info {
                    Ok(info) => {
                        tracing::debug!(
                            channel.name = %channel_name_for_monitoring,
                            "Channel actor completed normally, notifying manager"
                        );
                        let _ = manager_sender
                            .send(TwitchChatManagerMessage::ChannelActorCompleted {
                                channel_name: channel_name_for_monitoring,
                                termination_info: info,
                            })
                            .await;
                    }
                    Err(join_error) => {
                        tracing::error!(
                            channel.name = %channel_name_for_monitoring,
                            error = ?join_error,
                            "Channel actor panicked or was cancelled, notifying manager"
                        );
                        // Create a ChannelTerminationInfo for panicked/cancelled actors
                        let termination_info = ChannelTerminationInfo {
                            channel_name: channel_name_for_monitoring.clone(),
                            actor_id: uuid::Uuid::new_v4(), // We don't have the real actor_id
                            final_status: TwitchChannelConnectionStatus::Terminated,
                        };
                        let _ = manager_sender
                            .send(TwitchChatManagerMessage::ChannelActorCompleted {
                                channel_name: channel_name_for_monitoring,
                                termination_info,
                            })
                            .await;
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
                        lobby.id = %lobby_id,
                        channel.name = %normalized_channel_name,
                        "Successfully added lobby as subscriber to channel"
                    );
                    let _ = respond_to.send(Ok(final_actor_handle.get_status_receiver()));
                }
                Err(e) => {
                    tracing::error!(
                        lobby.id = %lobby_id,
                        channel.name = %normalized_channel_name,
                        error = ?e,
                        "Failed to add lobby as subscriber to channel"
                    );
                    let _ = respond_to.send(Err(e));
                }
            }
        } else {
            tracing::error!(
                channel.name = %normalized_channel_name,
                "Internal error: Failed to obtain or create actor handle for channel"
            );
            let error_msg = format!(
                "Internal error: Failed to obtain or create actor handle for channel '{}'",
                normalized_channel_name
            );
            let _ = respond_to.send(Err(TwitchError::InternalActorError(error_msg)));
        }
    }

    async fn handle_unsubscribe_from_channel(
        &mut self,
        channel_name: String,
        lobby_id: Uuid,
        respond_to: oneshot::Sender<Result<(), TwitchError>>,
    ) {
        let normalized_channel_name = channel_name.to_lowercase();
        tracing::info!(
            channel.name = %normalized_channel_name,
            lobby.id = %lobby_id,
            "Received unsubscribe request"
        );
        if let Some(channel_state) = self.active_channels.get(&normalized_channel_name) {
            match channel_state.handle.remove_subscriber(lobby_id).await {
                Ok(was_last_subscriber) => {
                    tracing::info!(
                        lobby.id = %lobby_id,
                        channel.name = %normalized_channel_name,
                        was_last = was_last_subscriber,
                        "Successfully unsubscribed lobby from channel"
                    );
                    let _ = respond_to.send(Ok(()));
                }
                Err(e) => {
                    tracing::error!(
                        lobby.id = %lobby_id,
                        channel.name = %normalized_channel_name,
                        error = ?e,
                        "Failed to unsubscribe lobby from channel"
                    );
                    let _ = respond_to.send(Err(e));
                }
            }
        } else {
            tracing::warn!(
                lobby.id = %lobby_id,
                channel.name = %normalized_channel_name,
                "Cannot unsubscribe lobby: Channel not found or not active"
            );
            let _ = respond_to.send(Ok(()));
        }
    }

    async fn handle_channel_actor_completed(
        &mut self,
        channel_name: String,
        termination_info: ChannelTerminationInfo,
    ) {
        tracing::info!(
            channel.name = %channel_name,
            termination_info = ?termination_info,
            "Channel actor completed with status"
        );

        if self.active_channels.remove(&channel_name).is_some() {
            tracing::debug!(
                channel.name = %channel_name,
                "Removed completed channel actor from active_channels"
            );
        } else {
            tracing::warn!(
                channel.name = %channel_name,
                "Received ChannelActorCompleted for channel, but it was not found in active_channels. It might have already been removed"
            );
        }
    }
}

pub async fn run_twitch_chat_manager_actor(mut actor: TwitchChatManagerActor) {
    tracing::info!("Twitch Chat Manager Actor started");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }
    tracing::info!("Twitch Chat Manager Actor stopped");
}
