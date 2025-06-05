use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::error::{Result as TwitchResult, TwitchError};
use super::irc_parser::IrcMessage;
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
        app_oauth_token: Arc<String>,
        manager_buffer_size: usize,
        channel_actor_buffer_size: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(manager_buffer_size);
        let actor = TwitchChatManagerActor::new(
            receiver,
            app_oauth_token,
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

struct ChannelActorState {
    handle: TwitchChannelActorHandle,
}

struct TwitchChatManagerActor {
    receiver: mpsc::Receiver<TwitchChatManagerMessage>,
    active_channels: HashMap<String, ChannelActorState>,
    app_oauth_token: Arc<String>,
    channel_actor_buffer_size: usize,
    self_sender: mpsc::Sender<TwitchChatManagerMessage>,
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

                    let (new_handle, join_handle) = TwitchChannelActorHandle::new(
                        normalized_channel_name.clone(),
                        Arc::clone(&self.app_oauth_token),
                        self.channel_actor_buffer_size,
                    );

                    self.active_channels.insert(
                        normalized_channel_name.clone(),
                        ChannelActorState {
                            handle: new_handle.clone(),
                        },
                    );

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

async fn run_twitch_chat_manager_actor(mut actor: TwitchChatManagerActor) {
    tracing::info!("[TWITCH_MANAGER] Twitch Chat Manager Actor started.");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }
    tracing::info!("[TWITCH_MANAGER] Twitch Chat Manager Actor stopped.");
}

#[derive(Debug)]
pub enum TwitchChannelActorMessage {
    AddSubscriber {
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
        respond_to: oneshot::Sender<TwitchResult<()>>,
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
        actor_buffer_size: usize,
    ) -> (Self, JoinHandle<ChannelTerminationInfo>) {
        let (actor_tx, actor_rx) = mpsc::channel(actor_buffer_size);
        let (status_tx, status_rx) = watch::channel(TwitchChannelConnectionStatus::Initializing);

        let actor = TwitchChannelActor::new(
            actor_rx,
            actor_tx.clone(),
            channel_name.clone(),
            oauth_token,
            status_tx.clone(),
        );

        let join_handle = tokio::spawn(run_twitch_channel_actor(actor));

        let handle = Self {
            sender: actor_tx,
            channel_name,
            status_rx,
        };

        (handle, join_handle)
    }

    pub async fn add_subscriber(
        &self,
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
    ) -> TwitchResult<()> {
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

    pub async fn remove_subscriber(&self, lobby_id: Uuid) -> TwitchResult<()> {
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

    pub async fn shutdown(&self) -> TwitchResult<()> {
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
}

impl TwitchChannelActor {
    fn new(
        receiver: mpsc::Receiver<TwitchChannelActorMessage>,
        self_sender_for_irc_task: mpsc::Sender<TwitchChannelActorMessage>,
        channel_name: String,
        oauth_token: Arc<String>,
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

                let task_is_truly_stopped_or_never_started = self
                    .irc_connection_task_handle
                    .as_ref()
                    .is_none_or(|h| h.is_finished());

                let current_actor_status = self.status_tx.borrow().clone();

                if matches!(
                    current_actor_status,
                    TwitchChannelConnectionStatus::Terminated
                ) {
                    tracing::warn!(
                        "[TWITCH][ACTOR][{}][{}] AddSubscriber received, but actor status is Terminated. Responding with error.",
                        self.channel_name,
                        self.actor_id
                    );
                    let _ = respond_to.send(Err(TwitchError::ChannelActorTerminated(
                        self.channel_name.clone(),
                    )));
                } else if task_is_truly_stopped_or_never_started {
                    tracing::info!(
                        "[TWITCH][ACTOR][{}][{}] IRC task is finished or was never started. Calling start_irc_connection_task.",
                        self.channel_name,
                        self.actor_id
                    );
                    self.start_irc_connection_task();
                    let _ = respond_to.send(Ok(()));
                } else {
                    tracing::debug!(
                        "[TWITCH][ACTOR][{}][{}] IRC task is active (status: {:?}, handle exists and not finished). Not starting new task.",
                        self.channel_name,
                        self.actor_id,
                        current_actor_status
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
                        self.channel_name,
                        self.actor_id
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
                                self.channel_name,
                                self.actor_id,
                                lobby_id
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
                            self.channel_name,
                            self.actor_id
                        );
                        self.signal_irc_task_shutdown();
                    }
                } else if !line.trim().is_empty()
                    && (irc_msg.command().is_some() || irc_msg.prefix().is_some())
                    && !matches!(
                        irc_msg.command(),
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
            TwitchChannelActorMessage::InternalConnectionStatusChanged { new_status } => {
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

                        let irc_loop_task_has_exited = self
                            .irc_connection_task_handle
                            .as_ref()
                            .is_none_or(|h| h.is_finished());

                        if irc_loop_task_has_exited {
                            tracing::info!(
                                "[TWITCH][ACTOR][{}][{}] The IRC connection loop task has exited.",
                                self.channel_name,
                                self.actor_id
                            );
                            self.irc_connection_task_handle.take();
                            self.irc_task_shutdown_tx.take();

                            if reason.contains("Persistent Auth Failure")
                                || reason.contains("Actor channel closed")
                            {
                                tracing::error!(
                                    "[TWITCH][ACTOR][{}][{}] Critical IRC error after loop exit: '{}'. Actor shutting down.",
                                    self.channel_name,
                                    self.actor_id,
                                    reason
                                );
                                self.initiate_actor_shutdown().await;
                            } else if self.subscribers.is_empty() {
                                tracing::info!(
                                    "[TWITCH][ACTOR][{}][{}] IRC loop exited (reason: '{}') and no subscribers. Actor shutting down.",
                                    self.channel_name,
                                    self.actor_id,
                                    reason
                                );
                                self.initiate_actor_shutdown().await;
                            } else {
                                tracing::warn!(
                                    "[TWITCH][ACTOR][{}][{}] IRC loop exited (reason: '{}') but actor has subscribers. Attempting to restart IRC task.",
                                    self.channel_name,
                                    self.actor_id,
                                    reason
                                );
                                self.start_irc_connection_task();
                            }
                        } else {
                            tracing::debug!(
                                "[TWITCH][ACTOR][{}][{}] IRC connection attempt failed (reason: '{}'), but IRC loop is still active and will retry. Actor remains active.",
                                self.channel_name,
                                self.actor_id,
                                reason
                            );
                        }
                    }
                    TwitchChannelConnectionStatus::Terminated => {
                        if let Some(handle) = self.irc_connection_task_handle.take() {
                            if !handle.is_finished() {
                                handle.abort();
                            }
                        }
                        self.irc_task_shutdown_tx.take();
                    }
                    _ => {}
                }
            }
            TwitchChannelActorMessage::Shutdown => {
                self.initiate_actor_shutdown().await;
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
        if let Some(handle) = &self.irc_connection_task_handle {
            if !handle.is_finished() {
                tracing::warn!(
                    "[TWITCH][ACTOR][{}][{}] Attempted to start IRC task, but an active (non-finished) one is already running.",
                    self.channel_name,
                    self.actor_id
                );
                return;
            }
            tracing::debug!(
                "[TWITCH][ACTOR][{}][{}] Existing IRC task handle is for a finished task. Clearing it.",
                self.channel_name,
                self.actor_id
            );
            self.irc_connection_task_handle.take();
        }

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

pub async fn run_twitch_channel_actor(mut actor: TwitchChannelActor) -> ChannelTerminationInfo {
    let channel_name = actor.channel_name.clone();
    let actor_id = actor.actor_id;

    tracing::info!(
        "[TWITCH][ACTOR][{}][{}] Actor started.",
        channel_name,
        actor_id
    );

    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }

    actor.signal_irc_task_shutdown();
    actor.await_irc_task_completion().await;

    let final_status = actor.status_tx.borrow().clone();

    tracing::info!(
        "[TWITCH][ACTOR][{}][{}] Actor stopped with status: {:?}",
        channel_name,
        actor_id,
        final_status
    );

    ChannelTerminationInfo {
        channel_name,
        actor_id,
        final_status,
    }
}

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
            tracing::error!(
                "[TWITCH][IRC_LOOP][{}][{}] Actor channel closed (Connecting). IRC loop shutting down.",
                channel_name,
                actor_id_for_logging
            );
            return;
        }

        let connection_result = tokio::select! {
            biased;
            _ = &mut shutdown_rx => {
                tracing::info!("[TWITCH][IRC_LOOP][{}][{}] Shutdown signal received. Terminating connection attempt.", channel_name, actor_id_for_logging);
                let _ = actor_tx.send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                    new_status: TwitchChannelConnectionStatus::Disconnected { reason: "Shutdown signal received".to_string() }
                }).await;
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
                tracing::info!(
                    "[TWITCH][IRC_LOOP][{}][{}] Connection closed/ended gracefully. Will attempt to reconnect.",
                    channel_name,
                    actor_id_for_logging
                );
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
                        let _ = actor_tx
                            .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                                new_status: TwitchChannelConnectionStatus::Disconnected {
                                    reason: format!("Persistent Auth Failure: {}", e),
                                },
                            })
                            .await;
                        tracing::error!(
                            "[TWITCH][IRC_LOOP][{}][{}] Persistent authentication failure. IRC loop will terminate.",
                            channel_name,
                            actor_id_for_logging
                        );
                        (e.to_string(), 0u64, true)
                    }
                    TwitchError::IrcTaskSendError(_) => {
                        tracing::error!(
                            "[TWITCH][IRC_LOOP][{}][{}] Failed to send to actor. IRC loop shutting down.",
                            channel_name,
                            actor_id_for_logging
                        );
                        (e.to_string(), 0u64, true)
                    }
                    _ => {
                        let base_delay = 5u64;
                        let backoff_delay =
                            base_delay * 2u64.pow(reconnect_attempts.saturating_sub(1).min(6));
                        (e.to_string(), u64::min(backoff_delay, 300), false)
                    }
                }
            }
        };

        if should_terminate_loop {
            return;
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
            tracing::error!(
                "[TWITCH][IRC_LOOP][{}][{}] Actor channel closed (Reconnecting). IRC loop shutting down.",
                channel_name,
                actor_id_for_logging
            );
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
            _ = tokio::time::sleep(reconnect_delay) => {}
        }
    }
}

async fn connect_and_listen_irc_single_attempt_adapted(
    channel_name: String,
    oauth_token: String,
    actor_tx: mpsc::Sender<TwitchChannelActorMessage>,
    connection_attempt_count: u32,
    actor_id_for_logging: Uuid,
) -> TwitchResult<()> {
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
        channel_name,
        actor_id_for_logging
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

    let mut last_server_activity = tokio::time::Instant::now();
    let mut last_health_check = tokio::time::Instant::now();
    let mut pending_health_check = false;
    let mut authenticated = false;

    let health_check_interval = Duration::from_secs(60);
    let health_check_timeout = Duration::from_secs(10);
    let server_activity_timeout = Duration::from_secs(400);
    let read_timeout = Duration::from_secs(5);

    let mut health_check_start_time = tokio::time::Instant::now();

    let mut message_timestamps: Vec<tokio::time::Instant> = Vec::new();
    let rate_window = Duration::from_secs(30);
    let min_messages_for_rate_detection = 10;
    let rate_drop_threshold = 0.3;
    let mut last_rate_check = tokio::time::Instant::now();
    let rate_check_interval = Duration::from_secs(10);
    let mut last_triggered_rate_ping = tokio::time::Instant::now();
    let min_time_between_rate_pings = Duration::from_secs(15);

    loop {
        line_buffer.clear();

        if authenticated {
            let now = tokio::time::Instant::now();

            if !pending_health_check
                && now.duration_since(last_health_check) >= health_check_interval
            {
                tracing::debug!(
                    "[TWITCH][IRC_HEALTH][{}][{}] Sending health check PING",
                    channel_name,
                    actor_id_for_logging
                );

                match writer.write_all(b"PING :health-check\r\n").await {
                    Ok(_) => {
                        if let Err(e) = writer.flush().await {
                            tracing::error!(
                                "[TWITCH][IRC_HEALTH][{}][{}] Failed to flush health check PING: {}",
                                channel_name,
                                actor_id_for_logging,
                                e
                            );
                            return Err(TwitchError::Io(e));
                        }
                        pending_health_check = true;
                        health_check_start_time = now;
                        last_health_check = now;
                    }
                    Err(e) => {
                        tracing::error!(
                            "[TWITCH][IRC_HEALTH][{}][{}] Failed to send health check PING: {}",
                            channel_name,
                            actor_id_for_logging,
                            e
                        );
                        return Err(TwitchError::Io(e));
                    }
                }
            }

            if pending_health_check
                && now.duration_since(health_check_start_time) >= health_check_timeout
            {
                tracing::warn!(
                    "[TWITCH][IRC_HEALTH][{}][{}] Health check PING timeout - no PONG received in {:?}. Connection dead.",
                    channel_name,
                    actor_id_for_logging,
                    health_check_timeout
                );
                return Err(TwitchError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Health check timeout - connection dead",
                )));
            }

            if now.duration_since(last_server_activity) >= server_activity_timeout {
                tracing::warn!(
                    "[TWITCH][IRC_HEALTH][{}][{}] No server activity in {:?}. Connection appears dead.",
                    channel_name,
                    actor_id_for_logging,
                    server_activity_timeout
                );
                return Err(TwitchError::Io(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "No server activity - connection dead",
                )));
            }

            // Message rate-based health check: Detect sudden drops in active chats
            if now.duration_since(last_rate_check) >= rate_check_interval {
                message_timestamps
                    .retain(|&timestamp| now.duration_since(timestamp) <= rate_window);

                let current_message_count = message_timestamps.len();

                if current_message_count >= min_messages_for_rate_detection {
                    let current_rate = current_message_count as f64 / rate_window.as_secs_f64();

                    let recent_cutoff = now - Duration::from_secs(10);
                    let recent_messages = message_timestamps
                        .iter()
                        .filter(|&&timestamp| timestamp >= recent_cutoff)
                        .count();
                    let recent_rate = recent_messages as f64 / 10.0;

                    if recent_rate < (current_rate * rate_drop_threshold)
                        && !pending_health_check
                        && now.duration_since(last_triggered_rate_ping)
                            >= min_time_between_rate_pings
                    {
                        tracing::info!(
                            "[TWITCH][IRC_RATE][{}][{}] Message rate drop detected: {:.2} -> {:.2} msg/s ({}% drop). Triggering immediate health check.",
                            channel_name,
                            actor_id_for_logging,
                            current_rate,
                            recent_rate,
                            ((1.0 - recent_rate / current_rate) * 100.0) as u32
                        );

                        match writer.write_all(b"PING :health-check\r\n").await {
                            Ok(_) => {
                                if let Err(e) = writer.flush().await {
                                    tracing::error!(
                                        "[TWITCH][IRC_RATE][{}][{}] Failed to flush rate-based PING: {}",
                                        channel_name,
                                        actor_id_for_logging,
                                        e
                                    );
                                    return Err(TwitchError::Io(e));
                                }
                                pending_health_check = true;
                                health_check_start_time = now;
                                last_triggered_rate_ping = now;
                                last_health_check = now;
                            }
                            Err(e) => {
                                tracing::error!(
                                    "[TWITCH][IRC_RATE][{}][{}] Failed to send rate-based PING: {}",
                                    channel_name,
                                    actor_id_for_logging,
                                    e
                                );
                                return Err(TwitchError::Io(e));
                            }
                        }
                    }
                }

                last_rate_check = now;
            }
        }

        match tokio::time::timeout(read_timeout, buf_reader.read_line(&mut line_buffer)).await {
            Ok(Ok(0)) => {
                tracing::info!(
                    "[TWITCH][IRC_READ][{}][{}] Connection closed by Twitch (EOF).",
                    channel_name,
                    actor_id_for_logging
                );
                return Ok(());
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                tracing::error!(
                    "[TWITCH][IRC_READ][{}][{}] Error reading from chat: {}",
                    channel_name,
                    actor_id_for_logging,
                    e
                );
                return Err(TwitchError::Io(e));
            }
            Err(_) => continue,
        }

        let message_line_owned = line_buffer.clone();

        if !message_line_owned.trim().is_empty() {
            last_server_activity = tokio::time::Instant::now();

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

        match parsed_for_task_logic.command() {
            Some("PING") => {
                let pong_target = parsed_for_task_logic
                    .params()
                    .first()
                    .unwrap_or(&":tmi.twitch.tv");

                tracing::debug!(
                    "[TWITCH][IRC_PING][{}][{}] Received server PING, responding with PONG",
                    channel_name,
                    actor_id_for_logging
                );

                writer
                    .write_all(format!("PONG {}\r\n", pong_target).as_bytes())
                    .await
                    .map_err(TwitchError::Io)?;
                writer.flush().await.map_err(TwitchError::Io)?;
            }
            Some("PONG") => {
                let pong_content = parsed_for_task_logic
                    .params()
                    .get(1)
                    .map(|s| &**s)
                    .unwrap_or("");

                if pending_health_check && pong_content.contains("health-check") {
                    let response_time =
                        tokio::time::Instant::now().duration_since(health_check_start_time);
                    tracing::debug!(
                        "[TWITCH][IRC_HEALTH][{}][{}] Health check PONG received in {:?}",
                        channel_name,
                        actor_id_for_logging,
                        response_time
                    );
                    pending_health_check = false;
                } else {
                    tracing::debug!(
                        "[TWITCH][IRC_PONG][{}][{}] Received PONG (not health check): {}",
                        channel_name,
                        actor_id_for_logging,
                        message_line_owned.trim()
                    );
                }
            }
            Some("001") => {
                tracing::info!(
                    "[TWITCH][IRC_AUTH][{}][{}] Authenticated successfully (RPL_WELCOME).",
                    channel_name,
                    actor_id_for_logging
                );

                authenticated = true;

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
                    .params()
                    .first()
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
                    .or_else(|| parsed_for_task_logic.params().get(1).map(|v| &**v))
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
                    .params()
                    .get(1)
                    .copied()
                    .unwrap_or_default();
                let capabilities = parsed_for_task_logic
                    .params()
                    .get(2)
                    .copied()
                    .unwrap_or_default();
                if ack_type == "NAK" {
                    tracing::error!(
                        "[TWITCH][IRC_CAP_NAK][{}][{}] Capability NAK: {}. This could affect functionality.",
                        channel_name,
                        actor_id_for_logging,
                        capabilities
                    );
                } else if ack_type == "ACK" {
                    tracing::info!(
                        "[TWITCH][IRC_CAP_ACK][{}][{}] Capability ACK: {}",
                        channel_name,
                        actor_id_for_logging,
                        capabilities
                    );
                }
            }
            Some("PRIVMSG") => {
                message_timestamps.push(tokio::time::Instant::now());

                if message_timestamps.len() > 1000 {
                    let cutoff = tokio::time::Instant::now() - rate_window * 2;
                    message_timestamps.retain(|&timestamp| timestamp >= cutoff);
                }
            }
            _ => {}
        }
    }
}

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

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use tokio::time::Instant;

    fn create_test_timestamps(base_time: Instant, intervals: &[u64]) -> Vec<Instant> {
        intervals
            .iter()
            .map(|&seconds| base_time + Duration::from_secs(seconds))
            .collect()
    }

    fn should_trigger_rate_check(
        message_timestamps: &[Instant],
        now: Instant,
        rate_window: Duration,
        min_messages_for_rate_detection: usize,
        rate_drop_threshold: f64,
    ) -> bool {
        let active_timestamps: Vec<Instant> = message_timestamps
            .iter()
            .filter(|&&timestamp| now.duration_since(timestamp) <= rate_window)
            .copied()
            .collect();

        let current_message_count = active_timestamps.len();

        if current_message_count < min_messages_for_rate_detection {
            return false;
        }

        let current_rate = current_message_count as f64 / rate_window.as_secs_f64();

        let recent_cutoff = now - Duration::from_secs(10);
        let recent_messages = active_timestamps
            .iter()
            .filter(|&&timestamp| timestamp >= recent_cutoff)
            .count();
        let recent_rate = recent_messages as f64 / 10.0;

        recent_rate < (current_rate * rate_drop_threshold)
    }

    #[tokio::test]
    async fn test_message_rate_tracking_no_trigger_insufficient_messages() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let timestamps = create_test_timestamps(base_time, &[1, 2, 3, 4, 5]);
        let now = base_time + Duration::from_secs(15);

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            !should_trigger,
            "Should not trigger with insufficient messages"
        );
    }

    #[tokio::test]
    async fn test_message_rate_tracking_no_trigger_steady_rate() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let timestamps = create_test_timestamps(
            base_time,
            &[0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28],
        );
        let now = base_time + Duration::from_secs(30);

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            !should_trigger,
            "Should not trigger with steady message rate"
        );
    }

    #[tokio::test]
    async fn test_message_rate_tracking_triggers_on_rate_drop() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let timestamps = create_test_timestamps(
            base_time,
            &[
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
            ],
        );
        let now = base_time + Duration::from_secs(30);

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            should_trigger,
            "Should trigger when message rate drops significantly"
        );
    }

    #[tokio::test]
    async fn test_message_rate_tracking_no_trigger_on_minor_drop() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let timestamps =
            create_test_timestamps(base_time, &[0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 21, 24, 27]);
        let now = base_time + Duration::from_secs(30);

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            !should_trigger,
            "Should not trigger on minor rate reduction"
        );
    }

    #[tokio::test]
    async fn test_message_rate_tracking_old_messages_excluded() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let timestamps = create_test_timestamps(base_time, &[0, 1, 2, 3, 4, 5]);
        let now = base_time + Duration::from_secs(35);

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            !should_trigger,
            "Should not trigger when old messages are excluded"
        );
    }

    #[tokio::test]
    async fn test_message_rate_tracking_threshold_boundary() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let timestamps = create_test_timestamps(
            base_time,
            &[0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 19, 19, 19, 19, 28],
        );
        let now = base_time + Duration::from_secs(30);

        let current_rate = timestamps.len() as f64 / rate_window.as_secs_f64();
        let recent_cutoff = now - Duration::from_secs(10);
        let recent_messages = timestamps
            .iter()
            .filter(|&&timestamp| timestamp >= recent_cutoff)
            .count();
        let recent_rate = recent_messages as f64 / 10.0;

        println!("Boundary test - Total messages: {}", timestamps.len());
        println!("Boundary test - Current rate: {:.3} msg/s", current_rate);
        println!("Boundary test - Recent messages: {}", recent_messages);
        println!("Boundary test - Recent rate: {:.3} msg/s", recent_rate);
        println!(
            "Boundary test - Threshold: {:.3} msg/s",
            current_rate * rate_drop_threshold
        );

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            should_trigger,
            "Should trigger when rate drops below threshold"
        );
    }

    #[tokio::test]
    async fn test_message_timestamps_vector_cleanup() {
        let rate_window = Duration::from_secs(30);
        let max_size = 1000;

        let mut message_timestamps: Vec<Instant> = Vec::new();
        let base_time = Instant::now();

        for i in 0..1200 {
            message_timestamps.push(base_time + Duration::from_secs(i));
        }

        let now = base_time + Duration::from_secs(1200);

        if message_timestamps.len() > max_size {
            let cutoff = now - rate_window * 2;
            message_timestamps.retain(|&timestamp| timestamp >= cutoff);
        }

        assert!(
            message_timestamps.len() <= 60,
            "Should clean up old timestamps"
        );

        for timestamp in &message_timestamps {
            assert!(
                now.duration_since(*timestamp) <= rate_window * 2,
                "All remaining timestamps should be within safety window"
            );
        }
    }

    #[tokio::test]
    async fn test_message_rate_tracking_realistic_scenario() {
        let rate_window = Duration::from_secs(30);
        let min_messages_for_rate_detection = 10;
        let rate_drop_threshold = 0.3;

        let base_time = Instant::now();

        let mut timestamps = Vec::new();

        for i in 0..20 {
            timestamps.push(base_time + Duration::from_secs(i));
        }

        let now = base_time + Duration::from_secs(30);

        let current_rate = timestamps.len() as f64 / rate_window.as_secs_f64();
        let recent_cutoff = now - Duration::from_secs(10);
        let recent_messages = timestamps
            .iter()
            .filter(|&&timestamp| timestamp >= recent_cutoff)
            .count();
        let recent_rate = recent_messages as f64 / 10.0;

        println!("Total messages: {}", timestamps.len());
        println!("Current rate: {:.3} msg/s", current_rate);
        println!("Recent messages in last 10s: {}", recent_messages);
        println!("Recent rate: {:.3} msg/s", recent_rate);
        println!(
            "Threshold rate: {:.3} msg/s",
            current_rate * rate_drop_threshold
        );
        println!(
            "Drop percentage: {:.1}%",
            (1.0 - recent_rate / current_rate) * 100.0
        );

        let should_trigger = should_trigger_rate_check(
            &timestamps,
            now,
            rate_window,
            min_messages_for_rate_detection,
            rate_drop_threshold,
        );

        assert!(
            should_trigger,
            "Should detect network drop in realistic chat scenario"
        );
        assert!(recent_rate < (current_rate * rate_drop_threshold));
    }
}
