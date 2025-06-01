// src/main.rs

use axum::{
    Router,
    extract::{
        Path, State,
        ws::{self, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{any, post},
};
use config::Config;
use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration as StdDuration};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

// --- Module Declarations ---
mod game_logic;
mod twitch_chat_manager;
mod twitch_integration;

// --- Imports from our modules ---
use game_logic::{
    ClientToServerMessage,
    DealNoDealGame,
    GameLogic,
    GameTwoEcho,
    HelloWorldGame,
    MedAndraOrdGameState,
    ServerToClientMessage, // For the deserialization helper
    messages as game_messages,
};
use twitch_chat_manager::TwitchChatManagerActorHandle;
use twitch_integration::{
    ParsedTwitchMessage,
    TwitchChannelConnectionStatus,
    TwitchError, // Assuming TwitchError is pub
};

// --- AppState ---
#[derive(Clone)]
struct AppState {
    lobby_manager: LobbyManagerHandle,
    twitch_chat_manager: TwitchChatManagerActorHandle,
}

// --- LobbyActor ---
#[derive(Debug)]
enum LobbyActorMessage {
    ProcessEvent {
        client_id: Uuid,
        event_data: String,
    },
    ClientConnected {
        client_id: Uuid,
        client_tx: mpsc::Sender<ws::Message>, // Corrected type
    },
    ClientDisconnected {
        client_id: Uuid,
    },
    InternalTwitchMessage(ParsedTwitchMessage),
    InternalTwitchStatusUpdate(TwitchChannelConnectionStatus),
}

struct LobbyActor<G: GameLogic + Send + 'static> {
    receiver: mpsc::Receiver<LobbyActorMessage>,
    lobby_id: Uuid,
    game_engine: G,
    manager_handle: LobbyManagerHandle, // LobbyManager, not TwitchChatManager

    twitch_channel_name: Option<String>,
    twitch_status_receiver: Option<watch::Receiver<TwitchChannelConnectionStatus>>,
    twitch_chat_manager_handle: TwitchChatManagerActorHandle,
    _twitch_message_task_handle: Option<tokio::task::JoinHandle<()>>,
    _twitch_status_task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl<G: GameLogic + Send + 'static> LobbyActor<G> {
    #[allow(clippy::too_many_arguments)]
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

    async fn handle_message(&mut self, msg: LobbyActorMessage) {
        match msg {
            LobbyActorMessage::ProcessEvent {
                client_id,
                event_data, // This is still the raw String from WebSocket
            } => {
                tracing::trace!(
                    // Changed to trace for less noise on raw data
                    "Lobby {} (Game: {}): Raw event from client {}: {}",
                    self.lobby_id,
                    self.game_engine.game_type_id(),
                    client_id,
                    event_data
                );

                // Attempt to deserialize the raw string into a structured ClientToServerMessage
                match game_messages::client_message_from_ws_text(&event_data) {
                    Ok(parsed_message) => {
                        tracing::debug!(
                            // Log the parsed message
                            "Lobby {} (Game: {}): Delegating parsed event {:?} from client {}",
                            self.lobby_id,
                            self.game_engine.game_type_id(),
                            parsed_message,
                            client_id
                        );
                        self.game_engine
                            .handle_event(client_id, parsed_message)
                            .await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Lobby {} (Game: {}): Failed to deserialize event from client {}: '{}'. Raw data: '{}'",
                            self.lobby_id,
                            self.game_engine.game_type_id(),
                            client_id,
                            e,
                            event_data
                        );
                        // Send an error message back to the specific client
                        if let Some(client_tx) = self.game_engine.get_client_tx(client_id) {
                            let error_response = ServerToClientMessage::SystemError {
                                message: format!(
                                    "Invalid message format: {}. Please send JSON like: {{\"command\":\"Echo\",\"payload\":{{\"message\":\"your_text\"}}}}",
                                    e
                                ),
                            };
                            if let Ok(ws_msg) = error_response.to_ws_text() {
                                if client_tx.send(ws_msg).await.is_err() {
                                    tracing::warn!(
                                        "Lobby {} (Game: {}): Failed to send error response to client {}",
                                        self.lobby_id,
                                        self.game_engine.game_type_id(),
                                        client_id
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
                    "Lobby {} (Game: {}): Delegating client {} connect.",
                    self.lobby_id,
                    self.game_engine.game_type_id(),
                    client_id
                );
                self.game_engine
                    .client_connected(client_id, client_tx)
                    .await;

                self.send_current_twitch_status_to_client(client_id).await;
            }
            LobbyActorMessage::ClientDisconnected { client_id } => {
                tracing::debug!(
                    "Lobby {} (Game: {}): Delegating client {} disconnect.",
                    self.lobby_id,
                    self.game_engine.game_type_id(),
                    client_id
                );
                self.game_engine.client_disconnected(client_id).await;
            }
            LobbyActorMessage::InternalTwitchMessage(twitch_msg) => {
                tracing::trace!(
                    "Lobby {} (Game: {}): Received internal Twitch message for channel #{}: <{}> {}",
                    self.lobby_id,
                    self.game_engine.game_type_id(),
                    twitch_msg.channel,
                    twitch_msg.sender_username,
                    twitch_msg.text
                );
                self.game_engine.handle_twitch_message(twitch_msg).await;
            }
            LobbyActorMessage::InternalTwitchStatusUpdate(status) => {
                tracing::info!(
                    "Lobby {} (Game: {}): Twitch channel '{}' status update: {:?}",
                    self.lobby_id,
                    self.game_engine.game_type_id(),
                    self.twitch_channel_name.as_deref().unwrap_or("N/A"),
                    status
                );

                // Broadcast the status update to all connected clients
                self.broadcast_twitch_status_update(status).await;
            }
        }
    }

    async fn broadcast_twitch_status_update(&self, status: TwitchChannelConnectionStatus) {
        // Convert the status to the format expected by the client
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

        // Create the data structure expected by the client
        let status_data = serde_json::json!({
            "channel_name": self.twitch_channel_name.clone(),
            "status_type": status_type,
            "details": details
        });

        // Send as GlobalEvent
        let global_event_message = match ServerToClientMessage::new_global_event(
            "TwitchStatusUpdate".to_string(),
            &status_data,
        ) {
            Ok(msg) => msg,
            Err(e) => {
                tracing::error!(
                    "Lobby {}: Failed to serialize Twitch status update: {}",
                    self.lobby_id,
                    e
                );
                return;
            }
        };

        if let Ok(ws_msg) = global_event_message.to_ws_text() {
            // Get all client IDs and broadcast
            let all_client_ids: Vec<Uuid> = self.game_engine.get_all_client_ids();

            for client_id in all_client_ids {
                if let Some(client_tx) = self.game_engine.get_client_tx(client_id) {
                    if client_tx.send(ws_msg.clone()).await.is_err() {
                        tracing::warn!(
                            "Lobby {}: Failed to send Twitch status update to client {}",
                            self.lobby_id,
                            client_id
                        );
                    }
                }
            }
        }
    }

    // Send initial status when client connects
    async fn send_current_twitch_status_to_client(&self, client_id: Uuid) {
        let (status_type, details) = if let Some(ref status_rx) = self.twitch_status_receiver {
            // Get the actual current status from the receiver
            let current_status = status_rx.borrow().clone();
            match &current_status {
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
                    "Lobby {}: Failed to serialize initial Twitch status for client {}: {}",
                    self.lobby_id,
                    client_id,
                    e
                );
                return;
            }
        };

        if let Some(client_tx) = self.game_engine.get_client_tx(client_id) {
            if let Ok(ws_msg) = global_event_message.to_ws_text() {
                if client_tx.send(ws_msg).await.is_err() {
                    tracing::warn!(
                        "Lobby {}: Failed to send initial Twitch status to client {}",
                        self.lobby_id,
                        client_id
                    );
                }
            }
        }
    }
}

async fn run_lobby_actor<G: GameLogic + Send + 'static>(
    mut actor: LobbyActor<G>,
    self_sender: mpsc::Sender<LobbyActorMessage>,
) {
    tracing::info!(
        "Lobby Actor {} (Game: {}) started. Twitch Channel: {:?}",
        actor.lobby_id,
        actor.game_engine.game_type_id(),
        actor.twitch_channel_name
    );

    let mut twitch_message_rx_option: Option<mpsc::Receiver<ParsedTwitchMessage>> = None;
    let mut twitch_status_rx_option: Option<watch::Receiver<TwitchChannelConnectionStatus>> = None;

    if let Some(ref channel_name) = actor.twitch_channel_name {
        let (tx_for_lobby_messages, rx_for_lobby_messages) = mpsc::channel(128);
        twitch_message_rx_option = Some(rx_for_lobby_messages);

        match actor
            .twitch_chat_manager_handle
            .subscribe_to_channel(channel_name.clone(), actor.lobby_id, tx_for_lobby_messages)
            .await
        {
            Ok(status_receiver) => {
                tracing::info!(
                    "Lobby {}: Successfully subscribed to Twitch channel '{}'",
                    actor.lobby_id,
                    channel_name
                );
                actor.twitch_status_receiver = Some(status_receiver.clone());
                twitch_status_rx_option = Some(status_receiver);
            }
            Err(e) => {
                tracing::error!(
                    "Lobby {}: Failed to subscribe to Twitch channel '{}': {:?}",
                    actor.lobby_id,
                    channel_name,
                    e
                );
                twitch_message_rx_option = None;
            }
        }
    }

    if let Some(mut receiver) = twitch_message_rx_option {
        let actor_sender_clone = self_sender.clone();
        let lobby_id_clone = actor.lobby_id;
        actor._twitch_message_task_handle = Some(tokio::spawn(async move {
            tracing::debug!(
                "Lobby {}: Twitch message listener task started.",
                lobby_id_clone
            );
            while let Some(twitch_msg) = receiver.recv().await {
                if actor_sender_clone
                    .send(LobbyActorMessage::InternalTwitchMessage(twitch_msg))
                    .await
                    .is_err()
                {
                    tracing::warn!(
                        "Lobby {}: Failed to send internal Twitch message to self.",
                        lobby_id_clone
                    );
                    break;
                }
            }
            tracing::debug!(
                "Lobby {}: Twitch message listener task stopped.",
                lobby_id_clone
            );
        }));
    }

    if let Some(mut status_receiver) = twitch_status_rx_option {
        let actor_sender_clone = self_sender.clone();
        let lobby_id_clone = actor.lobby_id;
        actor._twitch_status_task_handle = Some(tokio::spawn(async move {
            tracing::debug!(
                "Lobby {}: Twitch status listener task started.",
                lobby_id_clone
            );
            loop {
                tokio::select! {
                    changed_result = status_receiver.changed() => {
                        if changed_result.is_err() {
                            tracing::info!("Lobby {}: Twitch status channel closed.", lobby_id_clone);
                            break;
                        }
                        let status = status_receiver.borrow_and_update().clone();
                         if actor_sender_clone.send(LobbyActorMessage::InternalTwitchStatusUpdate(status)).await.is_err() {
                            tracing::warn!("Lobby {}: Failed to send internal Twitch status to self.", lobby_id_clone);
                            break;
                        }
                    }
                    else => break,
                }
            }
            tracing::debug!(
                "Lobby {}: Twitch status listener task stopped.",
                lobby_id_clone
            );
        }));
    }

    // Timeout for inactivity based on client WebSocket messages
    let client_ws_inactivity_timeout_duration = StdDuration::from_secs(60 * 60); // 60 minutes
    let mut last_client_ws_activity = Instant::now(); // Initialize to now

    loop {
        tokio::select! {
            maybe_msg = actor.receiver.recv() => {
                match maybe_msg {
                    Some(msg) => {
                        // Check if the message is a client WebSocket event, and only then update the timer.
                        if matches!(msg, LobbyActorMessage::ProcessEvent { .. }) {
                            last_client_ws_activity = Instant::now();
                            tracing::debug!(
                                "Lobby {} Actor (Game: {}): Client WS activity detected. Resetting 60-min inactivity timer.",
                                actor.lobby_id, actor.game_engine.game_type_id()
                            );
                        }
                        // Process all messages regardless
                        actor.handle_message(msg).await;
                    }
                    None => {
                        tracing::info!("Lobby Actor {} (Game: {}): Channel closed. Shutting down.",
                            actor.lobby_id, actor.game_engine.game_type_id());
                        break; // Exit actor loop
                    }
                }
            }
            // This branch fires if no client WebSocket messages have been received for the timeout duration
            _ = tokio::time::sleep_until(last_client_ws_activity + client_ws_inactivity_timeout_duration) => {
                tracing::info!(
                    "Lobby {} Actor (Game: {}): 60-minute inactivity (no client WebSocket messages received). Notifying manager for shutdown.",
                    actor.lobby_id, actor.game_engine.game_type_id()
                );
                if let Err(e) = actor.manager_handle.notify_lobby_shutdown(actor.lobby_id).await {
                    tracing::error!("Lobby {} Actor: Failed to notify LobbyManager of shutdown: {}", actor.lobby_id, e);
                }
                break; // Exit the actor loop
            }
        }
    }

    tracing::info!(
        "Lobby Actor {} (Game: {}) stopping.",
        actor.lobby_id,
        actor.game_engine.game_type_id()
    );

    if let Some(ref channel_name) = actor.twitch_channel_name {
        tracing::info!(
            "Lobby {}: Unsubscribing from Twitch channel '{}'",
            actor.lobby_id,
            channel_name
        );
        if let Err(e) = actor
            .twitch_chat_manager_handle
            .unsubscribe_from_channel(channel_name.clone(), actor.lobby_id)
            .await
        {
            tracing::error!(
                "Lobby {}: Failed to unsubscribe from Twitch channel '{}': {:?}",
                actor.lobby_id,
                channel_name,
                e
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

// --- LobbyActorHandle ---
#[derive(Clone, Debug)]
struct LobbyActorHandle {
    sender: mpsc::Sender<LobbyActorMessage>,
    lobby_id: Uuid,
}

impl LobbyActorHandle {
    #[allow(clippy::too_many_arguments)]
    fn new_spawned<G: GameLogic + Send + 'static>(
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

    async fn process_event(&self, client_id: Uuid, event_data: String) -> Result<(), String> {
        self.sender
            .send(LobbyActorMessage::ProcessEvent {
                client_id,
                event_data,
            })
            .await
            .map_err(|e| format!("Failed to send event: {}", e))
    }
    async fn client_connected(&self, client_id: Uuid, client_tx: mpsc::Sender<ws::Message>) {
        if self
            .sender
            .send(LobbyActorMessage::ClientConnected {
                client_id,
                client_tx,
            })
            .await
            .is_err()
        {
            tracing::error!("Lobby {}: Failed to send ClientConnected", self.lobby_id);
        }
    }
    async fn client_disconnected(&self, client_id: Uuid) {
        if self
            .sender
            .send(LobbyActorMessage::ClientDisconnected { client_id })
            .await
            .is_err()
        {
            tracing::error!("Lobby {}: Failed to send ClientDisconnected", self.lobby_id);
        }
    }
}

// --- LobbyManagerActor ---
#[derive(Debug, Serialize, Clone)]
struct LobbyDetails {
    lobby_id: Uuid,
    admin_id: Uuid,
    game_type_created: String,
    twitch_channel_subscribed: Option<String>,
}

#[derive(Debug)]
enum LobbyManagerMessage {
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

struct LobbyManagerActor {
    receiver: mpsc::Receiver<LobbyManagerMessage>,
    lobbies: HashMap<Uuid, LobbyActorHandle>,
    self_handle_prototype: Option<LobbyManagerHandle>,
    twitch_chat_manager_handle: TwitchChatManagerActorHandle,
}

impl LobbyManagerActor {
    fn new(
        receiver: mpsc::Receiver<LobbyManagerMessage>,
        twitch_chat_manager_handle: TwitchChatManagerActorHandle,
    ) -> Self {
        LobbyManagerActor {
            receiver,
            lobbies: HashMap::new(),
            self_handle_prototype: None,
            twitch_chat_manager_handle,
        }
    }

    fn set_self_handle(&mut self, handle: LobbyManagerHandle) {
        self.self_handle_prototype = Some(handle);
    }

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
                    .unwrap_or_else(|| "default".to_string());
                tracing::info!(
                    "LobbyManager: Creating lobby {} req_game='{}' req_twitch='{:?}'",
                    lobby_id,
                    game_type_str_req,
                    requested_twitch_channel
                );

                if let Some(manager_handle_clone) = self.self_handle_prototype.clone() {
                    let lobby_actor_handle: LobbyActorHandle;
                    let actual_game_type_created: String;

                    match game_type_str_req.to_lowercase().as_str() {
                        "game2" | "gametwoecho" => {
                            let game_engine = GameTwoEcho::new();
                            actual_game_type_created = game_engine.game_type_id();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<GameTwoEcho>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                                requested_twitch_channel.clone(),
                                self.twitch_chat_manager_handle.clone(),
                            );
                        }
                        "helloworld" | "default" | "" => {
                            let game_engine = HelloWorldGame::new();
                            actual_game_type_created = game_engine.game_type_id();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<HelloWorldGame>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                                requested_twitch_channel.clone(),
                                self.twitch_chat_manager_handle.clone(),
                            );
                        }
                        "dealnodeal" | "dealornodeal" => {
                            let game_engine = DealNoDealGame::new();
                            actual_game_type_created = game_engine.game_type_id();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<DealNoDealGame>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                                requested_twitch_channel.clone(),
                                self.twitch_chat_manager_handle.clone(),
                            );
                        }
                        "medandraord" | "medandra" | "ord" => {
                            let game_engine = MedAndraOrdGameState::new();
                            actual_game_type_created = game_engine.game_type_id();
                            lobby_actor_handle =
                                LobbyActorHandle::new_spawned::<MedAndraOrdGameState>(
                                    lobby_id,
                                    32,
                                    manager_handle_clone,
                                    game_engine,
                                    requested_twitch_channel.clone(),
                                    self.twitch_chat_manager_handle.clone(),
                                );
                        }
                        unknown => {
                            tracing::warn!(
                                "LobbyManager: Unknown game type '{}'. Defaulting to HelloWorldGame.",
                                unknown
                            );
                            let game_engine = HelloWorldGame::new();
                            actual_game_type_created = game_engine.game_type_id();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<HelloWorldGame>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                                requested_twitch_channel.clone(),
                                self.twitch_chat_manager_handle.clone(),
                            );
                        }
                    };

                    self.lobbies.insert(lobby_id, lobby_actor_handle);
                    let _ = respond_to.send(Ok(LobbyDetails {
                        lobby_id,
                        admin_id,
                        game_type_created: actual_game_type_created,
                        twitch_channel_subscribed: requested_twitch_channel,
                    }));
                } else {
                    tracing::error!("LobbyManager: Self handle not set for CreateLobby.");
                    let _ = respond_to.send(Err(
                        "LobbyManager internal error: self handle not set.".to_string(),
                    ));
                }
            }
            LobbyManagerMessage::GetLobbyHandle {
                lobby_id,
                respond_to,
            } => {
                let handle = self.lobbies.get(&lobby_id).cloned();
                let _ = respond_to.send(handle);
            }
            LobbyManagerMessage::LobbyActorShutdown { lobby_id } => {
                if self.lobbies.remove(&lobby_id).is_some() {
                    tracing::info!("LobbyManager: Removed handle for lobby {}", lobby_id);
                } else {
                    tracing::warn!(
                        "LobbyManager: Received shutdown for unknown lobby {}",
                        lobby_id
                    );
                }
            }
        }
    }
}

async fn run_lobby_manager_actor(mut actor: LobbyManagerActor) {
    tracing::info!("LobbyManager Actor started.");
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;
    }
    tracing::info!("LobbyManager Actor stopped.");
}

// --- LobbyManagerHandle ---
#[derive(Clone, Debug)]
struct LobbyManagerHandle {
    sender: mpsc::Sender<LobbyManagerMessage>,
}

impl LobbyManagerHandle {
    fn new(buffer_size: usize, twitch_chat_manager_handle: TwitchChatManagerActorHandle) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        let mut actor = LobbyManagerActor::new(receiver, twitch_chat_manager_handle);
        let handle = Self {
            sender: sender.clone(),
        };
        actor.set_self_handle(handle.clone());
        tokio::spawn(run_lobby_manager_actor(actor));
        handle
    }

    async fn create_lobby(
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
    async fn get_lobby_handle(&self, lobby_id: Uuid) -> Option<LobbyActorHandle> {
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
    async fn notify_lobby_shutdown(&self, lobby_id: Uuid) -> Result<(), String> {
        self.sender
            .send(LobbyManagerMessage::LobbyActorShutdown { lobby_id })
            .await
            .map_err(|e| format!("Failed to send LobbyActorShutdown: {}", e))
    }
}

// --- HTTP Handlers ---
#[derive(Deserialize, Debug, Default)]
struct CreateLobbyRequest {
    game_type: Option<String>,
    twitch_channel: Option<String>,
}

async fn create_lobby_handler(
    State(app_state): State<AppState>,
    Json(payload): Json<CreateLobbyRequest>,
) -> Result<Json<LobbyDetails>, (StatusCode, String)> {
    tracing::info!("HTTP: Received create_lobby request: {:?}", payload);
    match app_state
        .lobby_manager
        .create_lobby(payload.game_type, payload.twitch_channel)
        .await
    {
        Ok(details) => Ok(Json(details)),
        Err(e) => {
            tracing::error!("Failed to create lobby: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e))
        }
    }
}

async fn ws_handler(
    ws_upgrade: WebSocketUpgrade,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("WebSocket: Connection attempt to generic /ws endpoint");
    // The lobby_id is no longer known at this stage.
    // It will be determined by the first message from the client.
    ws_upgrade.on_upgrade(move |socket| handle_socket(socket, app_state))
}

async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // --- Initial Handshake Phase ---
    let lobby_handle: LobbyActorHandle;
    let client_id: Uuid;

    match ws_receiver.next().await {
        Some(Ok(ws::Message::Text(text_msg))) => {
            tracing::debug!("WS: Received initial message: {}", text_msg);
            match game_messages::client_message_from_ws_text(&text_msg) {
                Ok(ClientToServerMessage::ConnectToLobby {
                    lobby_id: received_lobby_id,
                }) => {
                    client_id = Uuid::new_v4(); // Generate client_id after getting lobby_id
                    tracing::info!(
                        "WebSocket: Client {} attempting to connect to lobby {} via initial message",
                        client_id,
                        received_lobby_id
                    );
                    match app_state
                        .lobby_manager
                        .get_lobby_handle(received_lobby_id)
                        .await
                    {
                        Some(handle) => {
                            lobby_handle = handle;
                            // Optionally, send a confirmation to the client that connection to lobby was successful.
                            // For example:
                            // let welcome_msg = ServerToClientMessage::GlobalEvent {
                            //     event_name: "ConnectedToLobby".to_string(),
                            //     data: serde_json::json!({ "status": "success", "lobbyId": received_lobby_id }),
                            // };
                            // if let Ok(ws_msg) = welcome_msg.to_ws_text() {
                            //     if ws_sender.send(ws_msg).await.is_err() {
                            //          tracing::warn!("WS: Failed to send ConnectedToLobby confirmation to client {}. Closing.", client_id);
                            //          let _ = ws_sender.close().await; // Close before returning if send fails
                            //          return;
                            //     }
                            // }
                        }
                        None => {
                            tracing::warn!(
                                "WebSocket: Lobby {} not found for client {} (requested via initial message). Closing.",
                                received_lobby_id,
                                client_id
                            );
                            let error_response = ServerToClientMessage::SystemError {
                                message: format!("Lobby {} not found.", received_lobby_id),
                            };
                            if let Ok(ws_msg) = error_response.to_ws_text() {
                                let _ = ws_sender.send(ws_msg).await;
                            }
                            let _ = ws_sender.close().await; // Close the WebSocket
                            return;
                        }
                    }
                }
                Ok(other_msg) => {
                    // First message was valid JSON but not ConnectToLobby
                    tracing::warn!(
                        "WebSocket: Initial message was not ConnectToLobby. Received: {:?}. Closing.",
                        other_msg
                    );
                    let error_response = ServerToClientMessage::SystemError {
                        message: "Invalid initial message type. Expected ConnectToLobby."
                            .to_string(),
                    };
                    if let Ok(ws_msg) = error_response.to_ws_text() {
                        let _ = ws_sender.send(ws_msg).await;
                    }
                    let _ = ws_sender.close().await;
                    return;
                }
                Err(e) => {
                    // First message was not valid JSON for ClientToServerMessage
                    tracing::warn!(
                        "WebSocket: Failed to deserialize initial message: {}. Raw: '{}'. Closing.",
                        e,
                        text_msg
                    );
                    let error_response = ServerToClientMessage::SystemError {
                        message: format!("Invalid initial connection message format: {}", e),
                    };
                    if let Ok(ws_msg) = error_response.to_ws_text() {
                        let _ = ws_sender.send(ws_msg).await;
                    }
                    let _ = ws_sender.close().await;
                    return;
                }
            }
        }
        Some(Ok(other_type_msg)) => {
            // First message was not Text (e.g. Binary, Ping, etc.)
            tracing::warn!(
                "WS: Client sent non-text initial message: {:?}. Closing.",
                other_type_msg
            );
            let error_response = ServerToClientMessage::SystemError {
                message: "Initial message must be a text JSON message (ConnectToLobby)."
                    .to_string(),
            };
            if let Ok(ws_msg) = error_response.to_ws_text() {
                let _ = ws_sender.send(ws_msg).await;
            }
            let _ = ws_sender.close().await;
            return;
        }
        Some(Err(e)) => {
            // WebSocket read error on first message
            tracing::warn!("WS: Error receiving initial message: {}. Closing.", e);
            // Cannot reliably send a message back if the receive itself failed badly.
            let _ = ws_sender.close().await;
            return;
        }
        None => {
            // Client disconnected before sending any message
            tracing::info!("WS: Client disconnected before sending initial message. Closing.");
            // Cannot send a message back if the stream is already closed.
            return;
        }
    }
    // --- End of Initial Handshake Phase ---

    tracing::info!(
        "WebSocket: Client {} now fully handling connection for lobby {}",
        client_id,
        lobby_handle.lobby_id
    );

    let (actor_to_client_tx, mut actor_to_client_rx) = mpsc::channel::<ws::Message>(32);

    lobby_handle
        .client_connected(client_id, actor_to_client_tx)
        .await;

    let lobby_id_clone_send = lobby_handle.lobby_id; // For logging in send_task
    let client_id_clone_send = client_id; // For logging in send_task
    let mut send_task = tokio::spawn(async move {
        while let Some(message_to_send) = actor_to_client_rx.recv().await {
            if ws_sender.send(message_to_send).await.is_err() {
                tracing::info!(
                    "Client {} in lobby {}: WS send error (from actor), client likely disconnected.",
                    client_id_clone_send,
                    lobby_id_clone_send
                );
                break;
            }
        }
        tracing::debug!(
            "Client {} in lobby {}: Send task from actor to WS client terminating.",
            client_id_clone_send,
            lobby_id_clone_send
        );
        // Attempt to close the sender part of the WebSocket gracefully when the loop ends
        let _ = ws_sender.close().await;
    });

    let lobby_handle_clone_recv = lobby_handle.clone(); // For recv_task
    let client_id_clone_recv = client_id; // For logging in recv_task
    let lobby_id_clone_recv = lobby_handle.lobby_id; // For logging in recv_task
    let mut recv_task = tokio::spawn(async move {
        // Note: ws_receiver has already consumed the first message for the handshake.
        loop {
            match ws_receiver.next().await {
                Some(Ok(msg)) => match msg {
                    ws::Message::Text(text_msg) => {
                        tracing::debug!(
                            "Client {} in lobby {}: Received text from WS: {:?}",
                            client_id_clone_recv,
                            lobby_id_clone_recv,
                            text_msg
                        );
                        if let Err(e) = lobby_handle_clone_recv
                            .process_event(client_id_clone_recv, text_msg.to_string())
                            .await
                        {
                            tracing::error!(
                                "Client {} in lobby {}: Error sending event to actor: {}",
                                client_id_clone_recv,
                                lobby_id_clone_recv,
                                e
                            );
                            // This error means the actor might be shut down or its channel is full.
                            // Consider if the client should be notified or connection closed.
                            // For now, we just log and continue, but breaking might be appropriate
                            // if the actor is critical for this connection.
                        }
                    }
                    ws::Message::Binary(_) => {
                        tracing::debug!(
                            "Client {} in lobby {}: Received binary message (ignored)",
                            client_id_clone_recv,
                            lobby_id_clone_recv
                        );
                    }
                    ws::Message::Ping(ping_data) => {
                        // Axum's WebSocket implementation typically handles responding to Pings with Pongs automatically.
                        // If you needed to customize this, you would send a ws::Message::Pong back.
                        tracing::trace!(
                            "Client {} in lobby {}: Received Ping from client (data: {:?}). Axum will auto-respond with Pong.",
                            client_id_clone_recv,
                            lobby_id_clone_recv,
                            ping_data
                        );
                    }
                    ws::Message::Pong(_) => {
                        tracing::trace!(
                            "Client {} in lobby {}: Received Pong from client.",
                            client_id_clone_recv,
                            lobby_id_clone_recv
                        );
                    }
                    ws::Message::Close(_) => {
                        tracing::info!(
                            "Client {} in lobby {}: WebSocket closed by client (recv).",
                            client_id_clone_recv,
                            lobby_id_clone_recv
                        );
                        break; // Exit the recv_task loop
                    }
                },
                Some(Err(e)) => {
                    tracing::warn!(
                        "Client {} in lobby {}: WebSocket error (recv): {}",
                        client_id_clone_recv,
                        lobby_id_clone_recv,
                        e
                    );
                    break; // Exit the recv_task loop on error
                }
                None => {
                    tracing::info!(
                        "Client {} in lobby {}: WebSocket connection closed (recv - no more messages).",
                        client_id_clone_recv,
                        lobby_id_clone_recv
                    );
                    break; // Exit the recv_task loop as stream ended
                }
            }
        }
        tracing::debug!(
            "Client {} in lobby {}: Receive task from WS client to actor terminating.",
            client_id_clone_recv,
            lobby_id_clone_recv
        );
    });

    // Wait for either task to complete, then abort the other.
    tokio::select! {
        _ = (&mut send_task) => {
            tracing::debug!("Client {} in lobby {}: Send task finished or aborted, aborting recv_task.", client_id, lobby_handle.lobby_id);
            recv_task.abort();
        },
        _ = (&mut recv_task) => {
            tracing::debug!("Client {} in lobby {}: Recv task finished or aborted, aborting send_task.", client_id, lobby_handle.lobby_id);
            send_task.abort();
        },
    }

    // Notify the lobby actor that this client has disconnected.
    lobby_handle.client_disconnected(client_id).await;
    tracing::info!(
        "WebSocket: Client {} fully disconnected from lobby {}",
        client_id,
        lobby_handle.lobby_id
    );
}

// --- Config Structs ---
#[derive(Debug, Deserialize)]
struct ServerConfig {
    port: u16,
    cors_origins: Vec<String>,
}
#[derive(Debug, Deserialize)]
struct TwitchConfig {
    client_id: String,
    client_secret: String,
}
#[derive(Debug, Deserialize)]
struct AppSettings {
    // Renamed from AppConfig to avoid conflict with config crate
    server: ServerConfig,
    twitch: TwitchConfig,
}

// --- Main Application Setup ---
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!(
                    "{}=info,tower_http=debug,{}=trace",
                    env!("CARGO_PKG_NAME"),
                    env!("CARGO_PKG_NAME")
                )
                .into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Load .env file if present
    //dotenvy::dotenv().ok();

    // Load Configuration
    let settings = Config::builder()
        .add_source(
            config::Environment::with_prefix("KOLMODIN") // Ensure this prefix is correct for your env vars
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("admin_password")
                .with_list_parse_key("server.cors_origins")
                .try_parsing(true),
        )
        .add_source(config::File::with_name("config").required(false)) // e.g., config.toml
        .build()
        .map_err(|e| format!("Failed to build config: {e}"))?;

    let app_settings: AppSettings = settings.try_deserialize()?;

    let app_oauth_token = Arc::new(
        fetch_twitch_app_access_token(
            &app_settings.twitch.client_id,
            &app_settings.twitch.client_secret,
        )
        .await
        .unwrap(),
    );
    tracing::info!("Successfully fetched Twitch App Access Token.");

    let twitch_chat_manager_handle = TwitchChatManagerActorHandle::new(app_oauth_token, 32, 32);
    let lobby_manager_handle = LobbyManagerHandle::new(32, twitch_chat_manager_handle.clone());

    let app_state = AppState {
        lobby_manager: lobby_manager_handle,
        twitch_chat_manager: twitch_chat_manager_handle,
    };

    let cors_origins_result: Result<Vec<HeaderValue>, _> = app_settings
        .server
        .cors_origins
        .iter()
        .map(|origin| {
            origin
                .parse()
                .map_err(|e| format!("Invalid CORS origin '{origin}': {e}"))
        })
        .collect();
    let cors_origins = cors_origins_result.unwrap_or_else(|e| {
        tracing::error!("CORS config error: {}. Defaulting to restrictive.", e);
        vec![]
    });
    let cors = if !cors_origins.is_empty() {
        CorsLayer::new()
            .allow_methods(vec![http::Method::GET, http::Method::POST])
            .allow_origin(cors_origins)
            .allow_credentials(true)
            .allow_headers(vec![
                http::header::CONTENT_TYPE,
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
            ])
    } else {
        CorsLayer::new()
    };

    let app = Router::new()
        .route("/api/create-lobby", post(create_lobby_handler))
        .route("/ws", any(ws_handler))
        .with_state(app_state)
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], app_settings.server.port)); // Listen on all interfaces
    tracing::info!("Listening on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

// --- Helper for Twitch Token Fetching ---
#[derive(Deserialize, Debug)]
struct TokenResponse {
    access_token: String,
}
async fn fetch_twitch_app_access_token(
    client_id: &str,
    client_secret: &str,
) -> Result<String, TwitchError> {
    tracing::info!("[TWITCH_API] Fetching App Access Token...");
    let url = "https://id.twitch.tv/oauth2/token";
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("grant_type", "client_credentials"),
    ];
    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .form(&params)
        .send()
        .await
        .map_err(TwitchError::Reqwest)?; // Convert reqwest::Error

    if response.status().is_success() {
        let token_data = response
            .json::<TokenResponse>()
            .await
            .map_err(TwitchError::Reqwest)?;
        Ok(token_data.access_token)
    } else {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error body".to_string());
        tracing::error!(
            "[TWITCH_API] Failed to get App Access Token (HTTP {}): {}",
            status,
            error_text
        );
        Err(TwitchError::TwitchAuth(format!(
            "Token fetch failed (HTTP {}): {}",
            status, error_text
        )))
    }
}
