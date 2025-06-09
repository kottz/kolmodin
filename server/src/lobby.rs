use axum::extract::ws;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use uuid::Uuid;

use crate::config::GamesConfig;
use crate::db::WordListManager;
use crate::game_logic::{
    DealNoDealGame, GameLogic, MedAndraOrdGameState, ServerToClientMessage,
    messages as game_messages,
};
use crate::twitch::{
    ParsedTwitchMessage, TwitchChannelConnectionStatus, TwitchChatManagerActorHandle,
};

#[derive(Debug, Serialize, Clone)]
pub struct LobbyDetails {
    pub lobby_id: Uuid,
    pub admin_id: Uuid,
    pub game_type_created: String,
    pub twitch_channel_subscribed: Option<String>,
}

#[derive(Debug)]
pub enum LobbyManagerMessage {
    CreateLobby {
        requested_game_type: Option<String>,
        requested_twitch_channel: Option<String>,
        respond_to: oneshot::Sender<Result<LobbyDetails, String>>,
    },
    GetLobbyHandle {
        lobby_id: Uuid,
        respond_to: oneshot::Sender<Option<LobbyActorHandle>>,
    },
    LobbyActorShutdown {
        lobby_id: Uuid,
    },
}

pub struct LobbyManagerActor {
    receiver: mpsc::Receiver<LobbyManagerMessage>,
    lobbies: HashMap<Uuid, LobbyActorHandle>,
    self_sender: mpsc::Sender<LobbyManagerMessage>,
    twitch_chat_manager_handle: TwitchChatManagerActorHandle,
    games_config: GamesConfig,
    word_list_manager: Arc<WordListManager>,
}

impl LobbyManagerActor {
    fn new(
        receiver: mpsc::Receiver<LobbyManagerMessage>,
        self_sender: mpsc::Sender<LobbyManagerMessage>,
        twitch_chat_manager_handle: TwitchChatManagerActorHandle,
        games_config: GamesConfig,
        word_list_manager: Arc<WordListManager>,
    ) -> Self {
        LobbyManagerActor {
            receiver,
            lobbies: HashMap::new(),
            self_sender,
            twitch_chat_manager_handle,
            games_config,
            word_list_manager,
        }
    }

    #[tracing::instrument(skip(self, msg), fields(
        msg_type = %std::any::type_name_of_val(&msg)
    ))]
    async fn handle_message(&mut self, msg: LobbyManagerMessage) {
        match msg {
            LobbyManagerMessage::CreateLobby {
                requested_game_type,
                requested_twitch_channel,
                respond_to,
            } => {
                let lobby_id = Uuid::new_v4();
                let admin_id = Uuid::new_v4();
                let game_type_str_req = requested_game_type
                    .clone()
                    .unwrap_or_else(|| "medandraord".to_string());

                tracing::info!(
                    lobby.id = %lobby_id,
                    request.game_type = %game_type_str_req,
                    request.twitch_channel = ?requested_twitch_channel,
                    "Received CreateLobby request"
                );

                // Validate Twitch channel if requested
                if let Some(ref channel_name) = requested_twitch_channel {
                    if !self
                        .word_list_manager
                        .is_twitch_channel_allowed(channel_name)
                        .await
                    {
                        tracing::warn!(
                            lobby.id = %lobby_id,
                            twitch.channel = %channel_name,
                            "Twitch channel not allowed for lobby creation"
                        );
                        let _ = respond_to.send(Err(format!(
                            "Twitch channel '{}' is not in the allowed channels list.",
                            channel_name
                        )));
                        return;
                    }
                }

                let manager_handle = LobbyManagerHandle {
                    sender: self.self_sender.clone(),
                };
                let lobby_actor_handle: LobbyActorHandle;
                let actual_game_type_created: String;

                // Get current word list for MedAndraOrd
                let mao_words = self.word_list_manager.get_med_andra_ord_words().await;

                match game_type_str_req.to_lowercase().as_str() {
                    "dealnodeal" | "dealornodeal" => {
                        if !self.games_config.enabled_types.contains("dealnodeal") {
                            tracing::error!(
                                lobby.id = %lobby_id,
                                game.type = "dealnodeal",
                                "Game type not enabled"
                            );
                            let _ = respond_to
                                .send(Err("Game type 'dealnodeal' is not enabled.".to_string()));
                            return;
                        }
                        let game_engine = DealNoDealGame::new();
                        actual_game_type_created = game_engine.game_type_id();
                        lobby_actor_handle = LobbyActorHandle::new_spawned::<DealNoDealGame>(
                            lobby_id,
                            32,
                            manager_handle,
                            game_engine,
                            requested_twitch_channel.clone(),
                            self.twitch_chat_manager_handle.clone(),
                        );
                    }
                    "medandraord" | "medandra" | "ord" => {
                        if !self.games_config.enabled_types.contains("medandraord") {
                            tracing::error!(
                                lobby.id = %lobby_id,
                                game.type = "medandraord",
                                "Game type not enabled"
                            );
                            let _ = respond_to
                                .send(Err("Game type 'medandraord' is not enabled.".to_string()));
                            return;
                        }
                        let game_engine = MedAndraOrdGameState::new(mao_words);
                        actual_game_type_created = game_engine.game_type_id();
                        lobby_actor_handle = LobbyActorHandle::new_spawned::<MedAndraOrdGameState>(
                            lobby_id,
                            32,
                            manager_handle.clone(),
                            game_engine,
                            requested_twitch_channel.clone(),
                            self.twitch_chat_manager_handle.clone(),
                        );
                    }
                    unknown => {
                        tracing::warn!(
                            lobby.id = %lobby_id,
                            game.type.requested = %unknown,
                            game.type.fallback = "medandraord",
                            "Unknown game type, defaulting to MedAndraOrd"
                        );
                        if !self.games_config.enabled_types.contains("medandraord") {
                            tracing::error!(
                                lobby.id = %lobby_id,
                                game.type.requested = %unknown,
                                game.type.fallback = "medandraord",
                                "Default game type not enabled for unknown request"
                            );
                            let _ = respond_to.send(Err(format!(
                                "Default game type 'medandraord' is not enabled for unknown request '{}'.", unknown
                            )));
                            return;
                        }
                        let game_engine = MedAndraOrdGameState::new(mao_words);
                        actual_game_type_created = game_engine.game_type_id();
                        lobby_actor_handle = LobbyActorHandle::new_spawned::<MedAndraOrdGameState>(
                            lobby_id,
                            32,
                            manager_handle.clone(),
                            game_engine,
                            requested_twitch_channel.clone(),
                            self.twitch_chat_manager_handle.clone(),
                        );
                    }
                };

                self.lobbies.insert(lobby_id, lobby_actor_handle);

                tracing::info!(
                    lobby.id = %lobby_id,
                    admin.id = %admin_id,
                    game.type = %actual_game_type_created,
                    twitch.channel = ?requested_twitch_channel,
                    "Created lobby successfully"
                );

                let _ = respond_to.send(Ok(LobbyDetails {
                    lobby_id,
                    admin_id,
                    game_type_created: actual_game_type_created,
                    twitch_channel_subscribed: requested_twitch_channel,
                }));
            }
            LobbyManagerMessage::GetLobbyHandle {
                lobby_id,
                respond_to,
            } => {
                tracing::debug!(
                    lobby.id = %lobby_id,
                    "Received GetLobbyHandle request"
                );
                let handle = self.lobbies.get(&lobby_id).cloned();
                let _ = respond_to.send(handle);
            }
            LobbyManagerMessage::LobbyActorShutdown { lobby_id } => {
                if self.lobbies.remove(&lobby_id).is_some() {
                    tracing::info!(
                        lobby.id = %lobby_id,
                        "Cleaning up lobby after actor shutdown"
                    );
                } else {
                    tracing::warn!(
                        lobby.id = %lobby_id,
                        "Received shutdown for unknown lobby"
                    );
                }
            }
        }
    }
}

#[tracing::instrument(skip(actor))]
pub async fn run_lobby_manager_actor(mut actor: LobbyManagerActor) {
    tracing::info!("LobbyManager actor started");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }
    tracing::info!("LobbyManager actor stopped");
}

#[derive(Clone, Debug)]
pub struct LobbyManagerHandle {
    sender: mpsc::Sender<LobbyManagerMessage>,
}

impl LobbyManagerHandle {
    pub fn new(
        buffer_size: usize,
        twitch_chat_manager_handle: TwitchChatManagerActorHandle,
        games_config: GamesConfig,
        word_list_manager: Arc<WordListManager>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        let actor = LobbyManagerActor::new(
            receiver,
            sender.clone(),
            twitch_chat_manager_handle,
            games_config,
            word_list_manager,
        );
        let handle = Self {
            sender: sender.clone(),
        };
        tokio::spawn(run_lobby_manager_actor(actor));
        handle
    }

    pub async fn create_lobby(
        &self,
        requested_game_type: Option<String>,
        requested_twitch_channel: Option<String>,
    ) -> Result<LobbyDetails, String> {
        let (respond_to, rx) = oneshot::channel();
        self.sender
            .send(LobbyManagerMessage::CreateLobby {
                requested_game_type,
                requested_twitch_channel,
                respond_to,
            })
            .await
            .map_err(|e| format!("Failed to send CreateLobby: {}", e))?;
        rx.await
            .map_err(|e| format!("LobbyManager no response: {}", e))?
    }

    pub async fn get_lobby_handle(&self, lobby_id: Uuid) -> Option<LobbyActorHandle> {
        let (tx, rx) = oneshot::channel();
        if self
            .sender
            .send(LobbyManagerMessage::GetLobbyHandle {
                lobby_id,
                respond_to: tx,
            })
            .await
            .is_err()
        {
            return None;
        }
        rx.await.ok().flatten()
    }

    pub async fn notify_lobby_shutdown(&self, lobby_id: Uuid) -> Result<(), String> {
        self.sender
            .send(LobbyManagerMessage::LobbyActorShutdown { lobby_id })
            .await
            .map_err(|e| format!("Failed to send LobbyActorShutdown: {}", e))
    }
}

#[derive(Debug)]
pub enum LobbyActorMessage {
    ProcessEvent {
        client_id: Uuid,
        event_data: String,
    },
    ClientConnected {
        client_id: Uuid,
        client_tx: mpsc::Sender<ws::Message>,
    },
    ClientDisconnected {
        client_id: Uuid,
    },
    InternalTwitchMessage(ParsedTwitchMessage),
    InternalTwitchStatusUpdate(TwitchChannelConnectionStatus),
}

pub struct LobbyActor<G: GameLogic + Send + 'static> {
    receiver: mpsc::Receiver<LobbyActorMessage>,
    lobby_id: Uuid,
    game_engine: G,
    manager_handle: LobbyManagerHandle,
    twitch_channel_name: Option<String>,
    twitch_status_receiver: Option<tokio::sync::watch::Receiver<TwitchChannelConnectionStatus>>,
    twitch_chat_manager_handle: TwitchChatManagerActorHandle,
    _twitch_message_task_handle: Option<tokio::task::JoinHandle<()>>,
    _twitch_status_task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl<G: GameLogic + Send + 'static> LobbyActor<G> {
    fn new(
        receiver: mpsc::Receiver<LobbyActorMessage>,
        lobby_id: Uuid,
        game_engine: G,
        manager_handle: LobbyManagerHandle,
        twitch_channel_name: Option<String>,
        twitch_chat_manager_handle: TwitchChatManagerActorHandle,
    ) -> Self {
        LobbyActor {
            receiver,
            lobby_id,
            game_engine,
            manager_handle,
            twitch_channel_name,
            twitch_chat_manager_handle,
            twitch_status_receiver: None,
            _twitch_message_task_handle: None,
            _twitch_status_task_handle: None,
        }
    }

    #[tracing::instrument(skip(self, msg), fields(
        lobby.id = %self.lobby_id,
        game.type = %self.game_engine.game_type_id(),
        twitch.channel = %self.twitch_channel_name.as_deref().unwrap_or("N/A"),
        msg_type = %std::any::type_name_of_val(&msg)
    ))]
    async fn handle_message(&mut self, msg: LobbyActorMessage) {
        match msg {
            LobbyActorMessage::ProcessEvent {
                client_id,
                event_data,
            } => {
                tracing::trace!(
                    client.id = %client_id,
                    event.raw = %event_data,
                    "Raw event from client"
                );

                match game_messages::client_message_from_ws_text(&event_data) {
                    Ok(parsed_message) => {
                        tracing::debug!(
                            client.id = %client_id,
                            event.type = ?parsed_message,
                            "Processing event from client"
                        );
                        self.game_engine
                            .handle_event(client_id, parsed_message)
                            .await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            client.id = %client_id,
                            error = %e,
                            event.raw = %event_data,
                            "Failed to deserialize event from client"
                        );
                        if let Some(client_tx) = self.game_engine.get_client_tx(client_id) {
                            let error_response = ServerToClientMessage::SystemError {
                                message: format!(
                                    "Invalid message format: {}. Please send JSON like: {{\"messageType\":\"GlobalCommand\",\"payload\":{{\"command_name\":\"Echo\",\"data\":{{\"message\":\"your_text\"}}}}}}",
                                    e
                                ),
                            };
                            if let Ok(ws_msg) = error_response.to_ws_text() {
                                if client_tx.send(ws_msg).await.is_err() {
                                    tracing::warn!(
                                        client.id = %client_id,
                                        "Failed to send error response to client"
                                    );
                                }
                            }
                        }
                    }
                }
            }
            LobbyActorMessage::ClientConnected {
                client_id,
                client_tx,
            } => {
                tracing::debug!(
                    client.id = %client_id,
                    "Client connected"
                );
                self.game_engine
                    .client_connected(client_id, client_tx)
                    .await;

                self.send_current_twitch_status_to_client(client_id).await;
            }
            LobbyActorMessage::ClientDisconnected { client_id } => {
                tracing::debug!(
                    client.id = %client_id,
                    "Client disconnected"
                );
                self.game_engine.client_disconnected(client_id).await;
            }
            LobbyActorMessage::InternalTwitchMessage(twitch_msg) => {
                tracing::debug!(
                    twitch.channel = %twitch_msg.channel,
                    twitch.sender = %twitch_msg.sender_username,
                    twitch.text = %twitch_msg.text,
                    "Received Twitch message"
                );
                self.game_engine.handle_twitch_message(twitch_msg).await;
            }
            LobbyActorMessage::InternalTwitchStatusUpdate(status) => {
                tracing::info!(
                    twitch.channel = %self.twitch_channel_name.as_deref().unwrap_or("N/A"),
                    twitch.status = ?status,
                    "Twitch channel status update"
                );

                self.broadcast_twitch_status_update(status).await;
            }
        }
    }

    async fn broadcast_twitch_status_update(&self, status: TwitchChannelConnectionStatus) {
        let (status_type, details) = match &status {
            TwitchChannelConnectionStatus::Initializing => ("Initializing".to_string(), None),
            TwitchChannelConnectionStatus::Connecting { attempt } => (
                "Connecting".to_string(),
                Some(format!("Attempt {}", attempt)),
            ),
            TwitchChannelConnectionStatus::Authenticating { attempt } => (
                "Authenticating".to_string(),
                Some(format!("Attempt {}", attempt)),
            ),
            TwitchChannelConnectionStatus::Connected => ("Connected".to_string(), None),
            TwitchChannelConnectionStatus::Reconnecting {
                reason,
                failed_attempt,
                retry_in,
            } => (
                "Reconnecting".to_string(),
                Some(format!(
                    "Attempt {} failed: {}. Retry in {}s",
                    failed_attempt,
                    reason,
                    retry_in.as_secs()
                )),
            ),
            TwitchChannelConnectionStatus::Disconnected { reason } => {
                ("Disconnected".to_string(), Some(reason.clone()))
            }
            TwitchChannelConnectionStatus::Terminated => ("Terminated".to_string(), None),
        };

        let status_data = serde_json::json!({
            "channel_name": self.twitch_channel_name.clone(),
            "status_type": status_type,
            "details": details
        });

        let global_event_message = match ServerToClientMessage::new_global_event(
            "TwitchStatusUpdate".to_string(),
            &status_data,
        ) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "Failed to serialize Twitch status update"
                );
                return;
            }
        };

        if let Ok(ws_msg) = global_event_message.to_ws_text() {
            let all_client_ids: Vec<Uuid> = self.game_engine.get_all_client_ids();

            for client_id in all_client_ids {
                if let Some(client_tx) = self.game_engine.get_client_tx(client_id) {
                    if client_tx.send(ws_msg.clone()).await.is_err() {
                        tracing::warn!(
                            client.id = %client_id,
                            "Failed to send Twitch status update to client"
                        );
                    }
                }
            }
        }
    }

    async fn send_current_twitch_status_to_client(&self, client_id: Uuid) {
        let (status_type, details) = if let Some(ref status_rx) = self.twitch_status_receiver {
            let current_status = status_rx.borrow().clone();
            match current_status {
                TwitchChannelConnectionStatus::Initializing => (
                    "Initializing".to_string(),
                    Some("Checking connection...".to_string()),
                ),
                TwitchChannelConnectionStatus::Connecting { attempt } => (
                    "Connecting".to_string(),
                    Some(format!("Attempt {}", attempt)),
                ),
                TwitchChannelConnectionStatus::Authenticating { attempt } => (
                    "Authenticating".to_string(),
                    Some(format!("Attempt {}", attempt)),
                ),
                TwitchChannelConnectionStatus::Connected => ("Connected".to_string(), None),
                TwitchChannelConnectionStatus::Reconnecting {
                    reason,
                    failed_attempt,
                    retry_in,
                } => (
                    "Reconnecting".to_string(),
                    Some(format!(
                        "Attempt {} failed: {}. Retry in {}s",
                        failed_attempt,
                        reason,
                        retry_in.as_secs()
                    )),
                ),
                TwitchChannelConnectionStatus::Disconnected { reason } => {
                    ("Disconnected".to_string(), Some(reason.clone()))
                }
                TwitchChannelConnectionStatus::Terminated => ("Terminated".to_string(), None),
            }
        } else if self.twitch_channel_name.is_some() {
            (
                "Initializing".to_string(),
                Some("Checking connection...".to_string()),
            )
        } else {
            (
                "Disconnected".to_string(),
                Some("No Twitch channel configured".to_string()),
            )
        };

        let status_data = serde_json::json!({
            "channel_name": self.twitch_channel_name.clone(),
            "status_type": status_type,
            "details": details
        });

        let global_event_message = match ServerToClientMessage::new_global_event(
            "TwitchStatusUpdate".to_string(),
            &status_data,
        ) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(
                    client.id = %client_id,
                    error = %e,
                    "Failed to serialize initial Twitch status for client"
                );
                return;
            }
        };

        if let Some(client_tx) = self.game_engine.get_client_tx(client_id) {
            if let Ok(ws_msg) = global_event_message.to_ws_text() {
                if client_tx.send(ws_msg).await.is_err() {
                    tracing::warn!(
                        client.id = %client_id,
                        "Failed to send initial Twitch status to client"
                    );
                }
            }
        }
    }
}

#[tracing::instrument(skip(actor, self_sender), fields(
    lobby.id = %actor.lobby_id,
    game.type = %actor.game_engine.game_type_id(),
    twitch.channel = %actor.twitch_channel_name.as_deref().unwrap_or("N/A")
))]
pub async fn run_lobby_actor<G: GameLogic + Send + 'static>(
    mut actor: LobbyActor<G>,
    self_sender: mpsc::Sender<LobbyActorMessage>,
) {
    tracing::info!("Lobby actor started");

    let mut twitch_message_rx_option: Option<mpsc::Receiver<ParsedTwitchMessage>> = None;
    let mut twitch_status_rx_option: Option<
        tokio::sync::watch::Receiver<TwitchChannelConnectionStatus>,
    > = None;

    if let Some(ref channel_name) = actor.twitch_channel_name {
        let (tx_for_lobby_messages, rx_for_lobby_messages) = mpsc::channel(128);
        twitch_message_rx_option = Some(rx_for_lobby_messages);

        tracing::info!(
            twitch.channel = %channel_name,
            "Subscribing to Twitch channel"
        );

        match actor
            .twitch_chat_manager_handle
            .subscribe_to_channel(channel_name.clone(), actor.lobby_id, tx_for_lobby_messages)
            .await
        {
            Ok(status_receiver) => {
                tracing::info!(
                    twitch.channel = %channel_name,
                    "Successfully subscribed to Twitch channel"
                );
                actor.twitch_status_receiver = Some(status_receiver.clone());
                twitch_status_rx_option = Some(status_receiver);
            }
            Err(e) => {
                tracing::error!(
                    twitch.channel = %channel_name,
                    error = ?e,
                    "Failed to subscribe to Twitch channel"
                );
                twitch_message_rx_option = None;
            }
        }
    }

    if let Some(mut receiver) = twitch_message_rx_option {
        let actor_sender_clone = self_sender.clone();
        let _lobby_id_clone = actor.lobby_id;
        actor._twitch_message_task_handle = Some(tokio::spawn(async move {
            tracing::debug!("Twitch message listener task started");
            while let Some(twitch_msg) = receiver.recv().await {
                if actor_sender_clone
                    .send(LobbyActorMessage::InternalTwitchMessage(twitch_msg))
                    .await
                    .is_err()
                {
                    tracing::warn!("Failed to send internal Twitch message to self");
                    break;
                }
            }
            tracing::debug!("Twitch message listener task stopped");
        }));
    }

    if let Some(mut status_receiver) = twitch_status_rx_option {
        let actor_sender_clone = self_sender.clone();
        let _lobby_id_clone = actor.lobby_id;
        actor._twitch_status_task_handle = Some(tokio::spawn(async move {
            tracing::debug!("Twitch status listener task started");
            loop {
                tokio::select! {
                    changed_result = status_receiver.changed() => {
                        if changed_result.is_err() {
                            tracing::info!("Twitch status channel closed");
                            break;
                        }
                        let status = status_receiver.borrow_and_update().clone();
                         if actor_sender_clone.send(LobbyActorMessage::InternalTwitchStatusUpdate(status)).await.is_err() {
                            tracing::warn!("Failed to send internal Twitch status to self");
                            break;
                        }
                    }
                    else => break,
                }
            }
            tracing::debug!("Twitch status listener task stopped");
        }));
    }

    let client_ws_inactivity_timeout_duration = StdDuration::from_secs(60 * 60);
    let mut last_client_ws_activity = Instant::now();

    loop {
        tokio::select! {
            maybe_msg = actor.receiver.recv() => {
                match maybe_msg {
                    Some(msg) => {
                        if matches!(msg, LobbyActorMessage::ProcessEvent { .. }) {
                            last_client_ws_activity = Instant::now();
                            tracing::trace!("Client WS activity detected. Resetting inactivity timer");
                        }
                        actor.handle_message(msg).await;
                    }
                    None => {
                        tracing::info!("Lobby actor channel closed. Shutting down");
                        break;
                    }
                }
            }
            _ = tokio::time::sleep_until(last_client_ws_activity + client_ws_inactivity_timeout_duration), if !actor.game_engine.is_empty() => {
                 // Only run inactivity timeout if there are clients.
                tracing::info!("Lobby inactivity timeout. Notifying manager for shutdown");
                if let Err(e) = actor.manager_handle.notify_lobby_shutdown(actor.lobby_id).await {
                    tracing::error!(
                        error = %e,
                        "Failed to notify LobbyManager of shutdown"
                    );
                }
                break;
            }
        }
    }

    tracing::info!("Lobby actor stopping");

    if let Some(ref channel_name) = actor.twitch_channel_name {
        tracing::info!(
            twitch.channel = %channel_name,
            "Unsubscribing from Twitch channel"
        );
        if let Err(e) = actor
            .twitch_chat_manager_handle
            .unsubscribe_from_channel(channel_name.clone(), actor.lobby_id)
            .await
        {
            tracing::error!(
                twitch.channel = %channel_name,
                error = ?e,
                "Failed to unsubscribe from Twitch channel"
            );
        }
    }

    if let Some(handle) = actor._twitch_message_task_handle.take() {
        handle.abort();
    }
    if let Some(handle) = actor._twitch_status_task_handle.take() {
        handle.abort();
    }
}

#[derive(Clone, Debug)]
pub struct LobbyActorHandle {
    pub sender: mpsc::Sender<LobbyActorMessage>,
    pub lobby_id: Uuid,
}

impl LobbyActorHandle {
    pub fn new_spawned<G: GameLogic + Send + 'static>(
        lobby_id: Uuid,
        buffer_size: usize,
        lobby_manager_handle: LobbyManagerHandle,
        game_engine_instance: G,
        twitch_channel_name: Option<String>,
        twitch_chat_manager_handle: TwitchChatManagerActorHandle,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        let actor = LobbyActor::<G>::new(
            receiver,
            lobby_id,
            game_engine_instance,
            lobby_manager_handle,
            twitch_channel_name,
            twitch_chat_manager_handle,
        );
        tokio::spawn(run_lobby_actor::<G>(actor, sender.clone()));
        Self { sender, lobby_id }
    }

    pub async fn process_event(&self, client_id: Uuid, event_data: String) -> Result<(), String> {
        self.sender
            .send(LobbyActorMessage::ProcessEvent {
                client_id,
                event_data,
            })
            .await
            .map_err(|e| format!("Failed to send event: {}", e))
    }

    pub async fn client_connected(&self, client_id: Uuid, client_tx: mpsc::Sender<ws::Message>) {
        if self
            .sender
            .send(LobbyActorMessage::ClientConnected {
                client_id,
                client_tx,
            })
            .await
            .is_err()
        {
            tracing::error!("Failed to send ClientConnected");
        }
    }

    pub async fn client_disconnected(&self, client_id: Uuid) {
        if self
            .sender
            .send(LobbyActorMessage::ClientDisconnected { client_id })
            .await
            .is_err()
        {
            tracing::error!("Failed to send ClientDisconnected");
        }
    }
}
