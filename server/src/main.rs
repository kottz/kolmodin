// src/main.rs

use axum::{
    extract::{
        ws::{self, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{any, post},
    Router,
};
use config::Config;
use futures_util::{SinkExt, StreamExt};
use http::HeaderValue;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    // HashSet is no longer directly used in LobbyActor if game logic handles clients
    net::SocketAddr,
    // Arc is not strictly needed for AppState here but doesn't hurt
};
use tokio::sync::mpsc::Sender as TokioMpscSender;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

// Declare the game_logic module
mod game_logic;
// Import the trait and specific game types
use game_logic::{GameLogic, GameTwoEcho, HelloWorldGame};

// --- LobbyActor ---
#[derive(Debug)]
enum LobbyActorMessage {
    ProcessEvent {
        client_id: Uuid,
        event_data: String,
    },
    ClientConnected {
        client_id: Uuid,
        client_tx: TokioMpscSender<ws::Message>,
    },
    ClientDisconnected {
        client_id: Uuid,
    },
    CheckShutdown,
}

struct LobbyActor<G: GameLogic + Send + 'static> {
    receiver: mpsc::Receiver<LobbyActorMessage>,
    lobby_id: Uuid,
    game_engine: G,
    manager_handle: LobbyManagerHandle,
}

impl<G: GameLogic + Send + 'static> LobbyActor<G> {
    fn new(
        receiver: mpsc::Receiver<LobbyActorMessage>,
        lobby_id: Uuid,
        manager_handle: LobbyManagerHandle,
        game_engine: G,
    ) -> Self {
        LobbyActor {
            receiver,
            lobby_id,
            game_engine,
            manager_handle,
        }
    }

    async fn handle_message(&mut self, msg: LobbyActorMessage) {
        match msg {
            LobbyActorMessage::ProcessEvent {
                client_id,
                event_data,
            } => {
                tracing::info!(
                    "Lobby {} Actor (Game: {}): Delegating event from client {}",
                    self.lobby_id,
                    self.game_engine.game_type(),
                    client_id
                );
                self.game_engine.handle_event(client_id, event_data).await;
            }
            LobbyActorMessage::ClientConnected {
                client_id,
                client_tx,
            } => {
                tracing::info!(
                    "Lobby {} Actor (Game: {}): Delegating client {} connect.",
                    self.lobby_id,
                    self.game_engine.game_type(),
                    client_id
                );
                self.game_engine
                    .client_connected(client_id, client_tx)
                    .await;
            }
            LobbyActorMessage::ClientDisconnected { client_id } => {
                tracing::info!(
                    "Lobby {} Actor (Game: {}): Delegating client {} disconnect.",
                    self.lobby_id,
                    self.game_engine.game_type(),
                    client_id
                );
                self.game_engine.client_disconnected(client_id).await;

                // Check for shutdown eligibility based on game engine's state
                if self.game_engine.is_empty() {
                    tracing::info!(
                        "Lobby {} Actor (Game: {}): Game reports no clients. Eligible for shutdown.",
                        self.lobby_id,
                        self.game_engine.game_type()
                    );
                }
            }
            LobbyActorMessage::CheckShutdown => {
                if self.game_engine.is_empty() {
                    tracing::info!(
                        "Lobby {} Actor (Game: {}): Confirmed game empty, notifying manager to shut down.",
                        self.lobby_id,
                        self.game_engine.game_type()
                    );
                    if let Err(e) = self
                        .manager_handle
                        .notify_lobby_shutdown(self.lobby_id)
                        .await
                    {
                        tracing::error!(
                            "Lobby {} Actor: Failed to notify manager of shutdown: {}",
                            self.lobby_id,
                            e
                        );
                    }
                }
            }
        }
    }
}

async fn run_lobby_actor<G: GameLogic + Send + 'static>(mut actor: LobbyActor<G>) {
    tracing::info!(
        "Lobby Actor {} (Game: {}) started.",
        actor.lobby_id,
        actor.game_engine.game_type()
    );

    let shutdown_duration = tokio::time::Duration::from_secs(60);
    let mut shutdown_check_interval =
        tokio::time::interval_at(Instant::now() + shutdown_duration, shutdown_duration);

    loop {
        tokio::select! {
            Some(msg) = actor.receiver.recv() => {
                actor.handle_message(msg).await;
            }
            _ = shutdown_check_interval.tick() => {
                if actor.game_engine.is_empty() {
                    tracing::info!("Lobby {} Actor (Game: {}): Inactivity detected. Notifying manager.",
                        actor.lobby_id, actor.game_engine.game_type());
                    if let Err(e) = actor.manager_handle.notify_lobby_shutdown(actor.lobby_id).await {
                        tracing::error!("Lobby {} Actor: Failed to notify manager of shutdown: {}", actor.lobby_id, e);
                    }
                    break;
                } else {
                    tracing::debug!("Lobby {} Actor (Game: {}): Activity check, game not empty.",
                        actor.lobby_id, actor.game_engine.game_type());
                }
            }
            else => {
                tracing::info!("Lobby Actor {} (Game: {}): All message channel senders dropped. Shutting down.",
                    actor.lobby_id, actor.game_engine.game_type());
                break;
            }
        }
    }
    tracing::info!(
        "Lobby Actor {} (Game: {}) stopped.",
        actor.lobby_id,
        actor.game_engine.game_type()
    );
}

#[derive(Clone, Debug)]
struct LobbyActorHandle {
    sender: mpsc::Sender<LobbyActorMessage>,
    lobby_id: Uuid,
}

impl LobbyActorHandle {
    fn new_spawned<G: GameLogic + Send + 'static>(
        lobby_id: Uuid,
        buffer_size: usize,
        manager_handle: LobbyManagerHandle,
        game_engine_instance: G,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        // Pass the game_engine to the LobbyActor constructor
        let actor = LobbyActor::<G>::new(receiver, lobby_id, manager_handle, game_engine_instance);
        tokio::spawn(run_lobby_actor::<G>(actor));
        Self { sender, lobby_id }
    }

    // process_event, client_connected, client_disconnected remain unchanged
    // as they were already forwarding appropriate messages.
    async fn process_event(&self, client_id: Uuid, event_data: String) -> Result<(), String> {
        let msg = LobbyActorMessage::ProcessEvent {
            client_id,
            event_data,
        };
        self.sender
            .send(msg)
            .await
            .map_err(|e| format!("Failed to send event to lobby actor: {}", e))
    }

    async fn client_connected(&self, client_id: Uuid, client_tx: TokioMpscSender<ws::Message>) {
        let msg = LobbyActorMessage::ClientConnected {
            client_id,
            client_tx,
        };
        if let Err(e) = self.sender.send(msg).await {
            tracing::error!(
                "Lobby {}: Failed to send ClientConnected to actor: {}",
                self.lobby_id,
                e
            );
        }
    }

    async fn client_disconnected(&self, client_id: Uuid) {
        let msg = LobbyActorMessage::ClientDisconnected { client_id };
        if let Err(e) = self.sender.send(msg).await {
            tracing::error!(
                "Lobby {}: Failed to send ClientDisconnected to actor: {}",
                self.lobby_id,
                e
            );
        }
    }
}

// --- LobbyManagerActor ---
#[derive(Debug, Serialize, Clone)]
struct LobbyDetails {
    lobby_id: Uuid,
    game_type_created: String, // To inform client which game was actually instantiated
}

#[derive(Debug)]
enum LobbyManagerMessage {
    CreateLobby {
        requested_game_type: Option<String>, // Client can request a game type
        respond_to: oneshot::Sender<LobbyDetails>,
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
}

impl LobbyManagerActor {
    fn new(receiver: mpsc::Receiver<LobbyManagerMessage>) -> Self {
        LobbyManagerActor {
            receiver,
            lobbies: HashMap::new(),
            self_handle_prototype: None,
        }
    }

    fn set_self_handle(&mut self, handle: LobbyManagerHandle) {
        self.self_handle_prototype = Some(handle);
    }

    async fn handle_message(&mut self, msg: LobbyManagerMessage) {
        match msg {
            LobbyManagerMessage::CreateLobby {
                requested_game_type,
                respond_to,
            } => {
                let lobby_id = Uuid::new_v4();
                let game_type_str_req = requested_game_type
                    .clone()
                    .unwrap_or_else(|| "default".to_string());
                tracing::info!(
                    "LobbyManager: Creating lobby {} req_game='{}'",
                    lobby_id,
                    game_type_str_req
                );

                if let Some(manager_handle_clone) = self.self_handle_prototype.clone() {
                    // The "dynamic dispatch" happens here: choosing which concrete game to init
                    // and which generic actor instantiation to spawn.
                    let lobby_actor_handle: LobbyActorHandle;
                    let actual_game_type_created: String;

                    match game_type_str_req.to_lowercase().as_str() {
                        "game2" | "gametwoecho" => {
                            let game_engine = GameTwoEcho::new();
                            actual_game_type_created = game_engine.game_type();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<GameTwoEcho>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                            );
                        }
                        "helloworld" | "default" | "" => {
                            let game_engine = HelloWorldGame::new();
                            actual_game_type_created = game_engine.game_type();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<HelloWorldGame>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                            );
                        }
                        unknown => {
                            tracing::warn!("LobbyManager: Unknown game type '{}'. Defaulting to HelloWorldGame.", unknown);
                            let game_engine = HelloWorldGame::new();
                            actual_game_type_created = game_engine.game_type();
                            lobby_actor_handle = LobbyActorHandle::new_spawned::<HelloWorldGame>(
                                lobby_id,
                                32,
                                manager_handle_clone,
                                game_engine,
                            );
                        }
                    };
                    self.lobbies.insert(lobby_id, lobby_actor_handle);
                    let _ = respond_to.send(LobbyDetails {
                        lobby_id,
                        game_type_created: actual_game_type_created,
                    });
                } else {
                    tracing::error!("LobbyManager: Self handle not set.");
                }
            }
            LobbyManagerMessage::GetLobbyHandle {
                lobby_id,
                respond_to,
            } => {
                tracing::debug!("LobbyManager Actor: Request for lobby handle {}", lobby_id);
                let handle = self.lobbies.get(&lobby_id).cloned();
                let _ = respond_to.send(handle);
            }
            LobbyManagerMessage::LobbyActorShutdown { lobby_id } => {
                tracing::info!(
                    "LobbyManager Actor: Received shutdown notification for lobby {}",
                    lobby_id
                );
                if self.lobbies.remove(&lobby_id).is_some() {
                    tracing::info!("LobbyManager Actor: Removed handle for lobby {}", lobby_id);
                } else {
                    tracing::warn!("LobbyManager Actor: Received shutdown for unknown/already removed lobby {}", lobby_id);
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

#[derive(Clone, Debug)]
struct LobbyManagerHandle {
    sender: mpsc::Sender<LobbyManagerMessage>,
}

impl LobbyManagerHandle {
    fn new(buffer_size: usize) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        let mut actor = LobbyManagerActor::new(receiver);
        let handle = Self {
            sender: sender.clone(),
        };
        actor.set_self_handle(handle.clone());
        tokio::spawn(run_lobby_manager_actor(actor));
        handle
    }

    // Update create_lobby to accept optional game type string
    async fn create_lobby(
        &self,
        requested_game_type: Option<String>,
    ) -> Result<LobbyDetails, String> {
        let (respond_to, rx) = oneshot::channel();
        self.sender
            .send(LobbyManagerMessage::CreateLobby {
                requested_game_type,
                respond_to,
            })
            .await
            .map_err(|e| format!("Failed to send CreateLobby to manager: {}", e))?;
        rx.await
            .map_err(|e| format!("LobbyManager failed to respond to CreateLobby: {}", e))
    }
    // get_lobby_handle and notify_lobby_shutdown remain the same.

    async fn get_lobby_handle(&self, lobby_id: Uuid) -> Option<LobbyActorHandle> {
        let (respond_to, rx) = oneshot::channel();
        if self
            .sender
            .send(LobbyManagerMessage::GetLobbyHandle {
                lobby_id,
                respond_to,
            })
            .await
            .is_err()
        {
            return None;
        }
        rx.await.unwrap_or(None)
    }

    async fn notify_lobby_shutdown(&self, lobby_id: Uuid) -> Result<(), String> {
        self.sender
            .send(LobbyManagerMessage::LobbyActorShutdown { lobby_id })
            .await
            .map_err(|e| format!("Failed to send LobbyActorShutdown to manager: {}", e))
    }
}

// --- AppState (remains the same) ---
#[derive(Clone)]
struct AppState {
    lobby_manager: LobbyManagerHandle,
}

// --- HTTP Handlers ---
// For create_lobby_handler, we need to accept a JSON payload for game_type
#[derive(Deserialize, Debug, Default)] // Default allows optional body or empty {}
struct CreateLobbyRequest {
    game_type: Option<String>,
}

async fn create_lobby_handler(
    State(app_state): State<AppState>,
    // Use axum::Json to deserialize the request body.
    // If the client sends an empty body or no body, and CreateLobbyRequest derives Default,
    // Axum 0.7+ can often handle this gracefully by using the default.
    // For robustness, client should send at least `{}` or `{"game_type": null}`
    Json(payload): Json<CreateLobbyRequest>,
) -> Result<Json<LobbyDetails>, StatusCode> {
    tracing::info!(
        "HTTP: Received create_lobby request with payload: {:?}",
        payload
    );
    match app_state
        .lobby_manager
        .create_lobby(payload.game_type)
        .await
    {
        Ok(details) => Ok(Json(details)),
        Err(e) => {
            tracing::error!("Failed to create lobby: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// ws_handler and handle_socket remain the same, as they deal with establishing
// the WebSocket connection and passing generic messages, not game-specific logic.
// --- WebSocket Handler ---
async fn ws_handler(
    ws_upgrade: WebSocketUpgrade,
    Path(lobby_id_str): Path<String>,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    let lobby_id = match Uuid::parse_str(&lobby_id_str) {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Invalid lobby ID format in path: {}", lobby_id_str);
            return (StatusCode::BAD_REQUEST, "Invalid lobby ID format").into_response();
        }
    };

    let client_id = Uuid::new_v4();
    tracing::info!(
        "WebSocket: Connection attempt for lobby {}, client {}",
        lobby_id,
        client_id
    );

    let lobby_handle = match app_state.lobby_manager.get_lobby_handle(lobby_id).await {
        Some(handle) => handle,
        None => {
            tracing::warn!(
                "WebSocket: Lobby {} not found for client {}",
                lobby_id,
                client_id
            );
            return (StatusCode::NOT_FOUND, "Lobby not found").into_response();
        }
    };

    ws_upgrade.on_upgrade(move |socket| handle_socket(socket, client_id, lobby_handle))
}

async fn handle_socket(socket: WebSocket, client_id: Uuid, lobby_handle: LobbyActorHandle) {
    tracing::info!(
        "WebSocket: Client {} now fully handling connection for lobby {}",
        client_id,
        lobby_handle.lobby_id
    );

    let (mut ws_sender, mut ws_receiver) = socket.split();
    let (actor_to_client_tx, mut actor_to_client_rx) = mpsc::channel::<ws::Message>(32);

    lobby_handle
        .client_connected(client_id, actor_to_client_tx)
        .await;

    let lobby_id_clone_send = lobby_handle.lobby_id;
    let client_id_clone_send = client_id;
    let mut send_task = tokio::spawn(async move {
        while let Some(message_to_send) = actor_to_client_rx.recv().await {
            if ws_sender.send(message_to_send).await.is_err() {
                tracing::info!(
                    "Client {} in lobby {}: WS send error (from actor), client likely disconnected.",
                    client_id_clone_send, lobby_id_clone_send
                );
                break;
            }
        }
        tracing::debug!(
            "Client {} in lobby {}: Send task from actor to WS client terminating.",
            client_id_clone_send,
            lobby_id_clone_send
        );
    });

    let lobby_handle_clone_recv = lobby_handle.clone();
    let client_id_clone_recv = client_id;
    let lobby_id_clone_recv = lobby_handle.lobby_id;
    let mut recv_task = tokio::spawn(async move {
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
                        }
                    }
                    ws::Message::Binary(_) => {
                        tracing::debug!(
                            "Client {} in lobby {}: Received binary message (ignored)",
                            client_id_clone_recv,
                            lobby_id_clone_recv
                        );
                    }
                    ws::Message::Ping(_) | ws::Message::Pong(_) => {}
                    ws::Message::Close(_) => {
                        tracing::info!(
                            "Client {} in lobby {}: WebSocket closed by client (recv).",
                            client_id_clone_recv,
                            lobby_id_clone_recv
                        );
                        break;
                    }
                },
                Some(Err(e)) => {
                    tracing::warn!(
                        "Client {} in lobby {}: WebSocket error (recv): {}",
                        client_id_clone_recv,
                        lobby_id_clone_recv,
                        e
                    );
                    break;
                }
                None => {
                    tracing::info!(
                        "Client {} in lobby {}: WebSocket connection closed (recv - no more messages).",
                        client_id_clone_recv, lobby_id_clone_recv
                    );
                    break;
                }
            }
        }
        tracing::debug!(
            "Client {} in lobby {}: Receive task from WS client to actor terminating.",
            client_id_clone_recv,
            lobby_id_clone_recv
        );
    });

    tokio::select! {
        _ = (&mut send_task) => {
            recv_task.abort();
            tracing::debug!("Client {} in lobby {}: Send task finished or aborted, aborting recv_task.", client_id, lobby_handle.lobby_id);
        },
        _ = (&mut recv_task) => {
            send_task.abort();
            tracing::debug!("Client {} in lobby {}: Recv task finished or aborted, aborting send_task.", client_id, lobby_handle.lobby_id);
        },
    }

    lobby_handle.client_disconnected(client_id).await;
    tracing::info!(
        "WebSocket: Client {} fully disconnected from lobby {}",
        client_id,
        lobby_handle.lobby_id
    );
}

// Config structs remain the same
#[derive(Debug, Deserialize)]
struct ServerConfig {
    port: u16,
    cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AppConfig {
    server: ServerConfig,
}

// --- Main Application Setup (remains largely the same) ---
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=info,tower_http=debug", env!("CARGO_PKG_NAME")).into()
                // Ensure your package name is correct or use a generic filter
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let lobby_manager_handle = LobbyManagerHandle::new(32);

    // --- Config Loading (remains the same) ---
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

    let app_config: AppConfig = settings
        .try_deserialize()
        .map_err(|e| format!("Failed to parse config: {e}"))?;

    let app_state = AppState {
        lobby_manager: lobby_manager_handle,
    };

    let cors_origins_result: Result<Vec<HeaderValue>, _> = app_config
        .server
        .cors_origins
        .iter()
        .map(|origin| {
            origin
                .parse()
                .map_err(|e| format!("Invalid CORS origin '{origin}': {e}"))
        })
        .collect();

    let cors_origins = match cors_origins_result {
        Ok(origins) => origins,
        Err(e) => {
            // It's better to fail early if CORS origins are misconfigured.
            // Or provide a sensible default like an empty vec if no origins are critical.
            tracing::error!("CORS configuration error: {}. Defaulting to no specific origins allowed beyond simple requests if any.", e);
            // Depending on strictness, you might panic or use a very restrictive default.
            // For development, you might allow all if the vec is empty, but that's not good for prod.
            // For this example, if parsing fails, it will likely restrict CORS significantly.
            // Let's assume if this fails, an empty vec is used, meaning restrictive CORS.
            vec![]
        }
    };

    let cors = if !cors_origins.is_empty() {
        CorsLayer::new()
            .allow_methods(vec![http::Method::GET, http::Method::POST])
            .allow_origin(cors_origins) // This applies the parsed origins
            .allow_credentials(true)
            .allow_headers(vec![
                http::header::CONTENT_TYPE,
                http::header::AUTHORIZATION,
                http::header::ACCEPT,
            ])
    } else {
        // A default, possibly more restrictive CORS policy if no origins were configured or parsed correctly
        // Or, if you want to allow any origin for development when cors_origins is empty (use with caution):
        // CorsLayer::new().allow_origin(tower_http::cors::Any).allow_methods(...).allow_headers(...)
        // For now, let's make it a layer that does minimal (effectively restrictive if no origins match)
        CorsLayer::new() // This will require origins to be set to actually allow cross-origin
    };

    let app = Router::new()
        .route("/api/create-lobby", post(create_lobby_handler))
        .route("/ws/{lobby_id}", any(ws_handler)) // Ensure path parameter matches
        .with_state(app_state)
        .layer(cors);

    // Use port from config or default
    let port = app_config.server.port; // Assuming port is part of your ServerConfig
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tracing::info!("Listening on {}", addr);

    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();

    Ok(())
}
