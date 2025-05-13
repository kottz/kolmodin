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
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::Arc, // Required for AppState if not using explicit actor for manager initially
};
use tokio::sync::mpsc::Sender as TokioMpscSender; // Alias to avoid conflict with futures_util::SinkExt::send
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use uuid::Uuid;

// --- Actor Definitions ---

// --- LobbyActor ---
#[derive(Debug)]
enum LobbyActorMessage {
    ProcessEvent {
        client_id: Uuid,
        event_data: String, // We can parse this to determine the action
                            // respond_to is no longer needed here if we handle all responses via client_tx
    },
    ClientConnected {
        client_id: Uuid,
        // NEW: Sender to push messages directly to this client's WebSocket
        client_tx: TokioMpscSender<ws::Message>,
    },
    ClientDisconnected {
        client_id: Uuid,
    },
    // CheckShutdown remains the same
    CheckShutdown,
}

struct LobbyActor {
    receiver: mpsc::Receiver<LobbyActorMessage>,
    lobby_id: Uuid,
    // OLD: connected_clients: HashSet<Uuid>,
    // NEW: Store client ID and their WebSocket sender
    clients: HashMap<Uuid, TokioMpscSender<ws::Message>>,
    manager_handle: LobbyManagerHandle,
}

impl LobbyActor {
    fn new(
        receiver: mpsc::Receiver<LobbyActorMessage>,
        lobby_id: Uuid,
        manager_handle: LobbyManagerHandle,
    ) -> Self {
        LobbyActor {
            receiver,
            lobby_id,
            clients: HashMap::new(), // Changed from HashSet
            manager_handle,
        }
    }

    async fn handle_message(&mut self, msg: LobbyActorMessage) {
        match msg {
            LobbyActorMessage::ProcessEvent {
                client_id,
                event_data, // This now contains "COMMAND message_payload"
            } => {
                tracing::info!(
                    "Lobby {} Actor: Received event raw data '{}' from client {}",
                    self.lobby_id,
                    event_data,
                    client_id
                );

                // Parse event_data: first word is command, rest is payload
                let mut parts = event_data.splitn(2, ' ');
                let command = parts.next().unwrap_or("").to_lowercase(); // Get command, or empty if bad format
                let payload = parts.next().unwrap_or("Default message if none provided"); // Get payload

                tracing::info!(
                    "Lobby {} Actor: Parsed command: '{}', payload: '{}' from client {}",
                    self.lobby_id,
                    command,
                    payload,
                    client_id
                );

                // Use the 'payload' as the message content
                let message_to_send = payload.to_string();

                match command.as_str() {
                    "send_to_self" => {
                        if let Some(sender_tx) = self.clients.get(&client_id) {
                            // Send the custom payload
                            if sender_tx
                                .send(ws::Message::Text(
                                    format!("Private: {}", message_to_send).into(),
                                ))
                                .await
                                .is_err()
                            {
                                tracing::warn!(
                                    "Lobby {}: Failed to send direct response to client {}",
                                    self.lobby_id,
                                    client_id
                                );
                            }
                        } else {
                            tracing::warn!(
                                "Lobby {}: Client {} not found for direct response",
                                self.lobby_id,
                                client_id
                            );
                        }
                    }
                    "broadcast_all" => {
                        tracing::info!("Lobby {}: Broadcasting to all clients", self.lobby_id);
                        for (target_client_id, client_tx) in self.clients.iter() {
                            // Send the custom payload
                            if client_tx
                                .send(ws::Message::Text(
                                    format!("Broadcast: {}", message_to_send).into(),
                                ))
                                .await
                                .is_err()
                            {
                                tracing::warn!(
                                    "Lobby {}: Failed to broadcast to client {}",
                                    self.lobby_id,
                                    target_client_id
                                );
                            }
                        }
                    }
                    "broadcast_except_self" => {
                        tracing::info!(
                            "Lobby {}: Broadcasting to all clients except sender {}",
                            self.lobby_id,
                            client_id
                        );
                        for (target_client_id, client_tx) in self.clients.iter() {
                            if *target_client_id != client_id {
                                // Send the custom payload
                                if client_tx
                                    .send(ws::Message::Text(
                                        format!("Broadcast (others): {}", message_to_send).into(),
                                    ))
                                    .await
                                    .is_err()
                                {
                                    tracing::warn!(
                                        "Lobby {}: Failed to broadcast (others) to client {}",
                                        self.lobby_id,
                                        target_client_id
                                    );
                                }
                            }
                        }
                    }
                    _ => {
                        tracing::warn!(
                            "Lobby {}: Unknown command: '{}'. Sending echo of payload to self.",
                            self.lobby_id,
                            command
                        );
                        if let Some(sender_tx) = self.clients.get(&client_id) {
                            if sender_tx
                                .send(ws::Message::Text(
                                    format!("Echo of your message: {}", message_to_send).into(),
                                ))
                                .await
                                .is_err()
                            {
                                tracing::warn!(
                                    "Lobby {}: Failed to send echo to client {}",
                                    self.lobby_id,
                                    client_id
                                );
                            }
                        }
                    }
                }
            }
            // ... other LobbyActorMessage cases remain the same
            LobbyActorMessage::ClientConnected {
                client_id,
                client_tx,
            } => {
                tracing::info!(
                    "Lobby {} Actor: Client {} connected.",
                    self.lobby_id,
                    client_id
                );
                self.clients.insert(client_id, client_tx);
            }
            LobbyActorMessage::ClientDisconnected { client_id } => {
                tracing::info!(
                    "Lobby {} Actor: Client {} disconnected.",
                    self.lobby_id,
                    client_id
                );
                self.clients.remove(&client_id);
                if self.clients.is_empty() {
                    tracing::info!(
                        "Lobby {} Actor: No clients connected. Eligible for shutdown.",
                        self.lobby_id
                    );
                }
            }
            LobbyActorMessage::CheckShutdown => {
                if self.clients.is_empty() {
                    tracing::info!(
                        "Lobby {} Actor: Confirmed no clients, notifying manager to shut down.",
                        self.lobby_id
                    );
                    // Manager notification logic...
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
                    // The actor should break its loop after notifying manager.
                    // This is handled in run_lobby_actor.
                }
            }
        }
    }
}

async fn run_lobby_actor(mut actor: LobbyActor) {
    tracing::info!("Lobby Actor {} started.", actor.lobby_id);

    let shutdown_duration = tokio::time::Duration::from_secs(60);
    // The first tick will occur `shutdown_duration` from now.
    let mut shutdown_check_interval =
        tokio::time::interval_at(Instant::now() + shutdown_duration, shutdown_duration);

    loop {
        tokio::select! {
            Some(msg) = actor.receiver.recv() => {
                actor.handle_message(msg).await;
            }
            _ = shutdown_check_interval.tick() => {
                if actor.clients.is_empty() {
                    tracing::info!("Lobby {} Actor: Inactivity detected ({}s). Notifying manager and preparing to shut down.", actor.lobby_id, shutdown_duration.as_secs());
                    if let Err(e) = actor.manager_handle.notify_lobby_shutdown(actor.lobby_id).await {
                        tracing::error!("Lobby {} Actor: Failed to notify manager of shutdown: {}", actor.lobby_id, e);
                    }
                    break; // Exit the loop, actor task will terminate.
                } else {
                    tracing::debug!("Lobby {} Actor: Activity check, {} clients connected. Resetting inactivity timer.", actor.lobby_id, actor.clients.len());
                    // The interval resets automatically on the next tick.
                }
            }
            else => {
                tracing::info!("Lobby Actor {}: All message channel senders dropped. Shutting down.", actor.lobby_id);
                // Optionally, notify manager if it wasn't an explicit shutdown_check
                if !actor.clients.is_empty() { // If clients were connected, this is unexpected
                    tracing::warn!("Lobby {} Actor: Shutting down due to dropped channels WITH clients connected. This might indicate an issue.", actor.lobby_id);
                }
                // Ensure manager is notified if this actor is shutting down for reasons other than inactivity timer
                // However, the manager removing the handle is what causes this branch, usually.
                // If the manager still holds a handle and this actor's receiver closes,
                // it implies all other handles (like temporary ones in ws_handler) were dropped.
                break;
            }
        }
    }
    tracing::info!("Lobby Actor {} stopped.", actor.lobby_id);
}

#[derive(Clone, Debug)]
struct LobbyActorHandle {
    sender: mpsc::Sender<LobbyActorMessage>,
    lobby_id: Uuid,
}

impl LobbyActorHandle {
    // Renamed to indicate it's for creating an actor instance managed elsewhere (e.g., by LobbyManager)
    fn new_spawned(lobby_id: Uuid, buffer_size: usize, manager_handle: LobbyManagerHandle) -> Self {
        let (sender, receiver) = mpsc::channel(buffer_size);
        let actor = LobbyActor::new(receiver, lobby_id, manager_handle);
        tokio::spawn(run_lobby_actor(actor));
        Self { sender, lobby_id }
    }

    async fn process_event(&self, client_id: Uuid, event_data: String) -> Result<(), String> {
        // No oneshot channel needed here anymore for the primary response,
        // as the LobbyActor will use the client_tx to send messages.
        let msg = LobbyActorMessage::ProcessEvent {
            client_id,
            event_data,
        };
        self.sender
            .send(msg)
            .await
            .map_err(|e| format!("Failed to send event to lobby actor: {}", e))
    }

    // Updated client_connected to pass the client_tx
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
#[derive(Debug, Serialize, Clone)] // Added Serialize and Clone
struct LobbyDetails {
    lobby_id: Uuid,
}

#[derive(Debug)]
enum LobbyManagerMessage {
    CreateLobby {
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
    // To pass its own handle to newly created LobbyActors for callbacks
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

    // Method to set its own handle after creation
    fn set_self_handle(&mut self, handle: LobbyManagerHandle) {
        self.self_handle_prototype = Some(handle);
    }

    async fn handle_message(&mut self, msg: LobbyManagerMessage) {
        match msg {
            LobbyManagerMessage::CreateLobby { respond_to } => {
                let lobby_id = Uuid::new_v4();
                tracing::info!("LobbyManager Actor: Creating lobby {}", lobby_id);

                if let Some(manager_handle_clone) = self.self_handle_prototype.clone() {
                    let lobby_actor_handle =
                        LobbyActorHandle::new_spawned(lobby_id, 32, manager_handle_clone);
                    self.lobbies.insert(lobby_id, lobby_actor_handle);
                    let _ = respond_to.send(LobbyDetails { lobby_id });
                } else {
                    tracing::error!(
                        "LobbyManager Actor: Self handle not set, cannot create lobby actor."
                    );
                    // respond_to will be dropped, client will get an error
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
                    tracing::warn!(
                        "LobbyManager Actor: Received shutdown for unknown/already removed lobby {}",
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
        }; // Create handle first
        actor.set_self_handle(handle.clone()); // Give the actor its own handle
        tokio::spawn(run_lobby_manager_actor(actor));
        handle
    }

    async fn create_lobby(&self) -> Result<LobbyDetails, String> {
        let (respond_to, rx) = oneshot::channel();
        self.sender
            .send(LobbyManagerMessage::CreateLobby { respond_to })
            .await
            .map_err(|e| format!("Failed to send CreateLobby to manager: {}", e))?;
        rx.await
            .map_err(|e| format!("LobbyManager failed to respond to CreateLobby: {}", e))
    }

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
            return None; // Manager likely shut down
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

// --- AppState ---
#[derive(Clone)]
struct AppState {
    lobby_manager: LobbyManagerHandle,
}

// --- HTTP Handlers ---
async fn create_lobby_handler(
    State(app_state): State<AppState>,
) -> Result<Json<LobbyDetails>, StatusCode> {
    tracing::info!("HTTP: Received create_lobby request");
    match app_state.lobby_manager.create_lobby().await {
        Ok(details) => Ok(Json(details)),
        Err(e) => {
            tracing::error!("Failed to create lobby: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// --- WebSocket Handler ---
async fn ws_handler(
    ws_upgrade: WebSocketUpgrade,
    Path(lobby_id_str): Path<String>, // Extract lobby_id from path
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    let lobby_id = match Uuid::parse_str(&lobby_id_str) {
        Ok(id) => id,
        Err(_) => {
            tracing::error!("Invalid lobby ID format in path: {}", lobby_id_str);
            return (StatusCode::BAD_REQUEST, "Invalid lobby ID format").into_response();
        }
    };

    let client_id = Uuid::new_v4(); // Unique ID for this WebSocket connection
    tracing::info!(
        "WebSocket: Connection attempt for lobby {}, client {}",
        lobby_id,
        client_id
    );

    // Get the handle for the specific lobby actor
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

    ws_upgrade.on_upgrade(move |socket| {
        handle_socket(socket, client_id, lobby_handle) // Pass lobby_handle
    })
}

async fn handle_socket(
    socket: WebSocket, // Renamed from mut socket as we'll split it
    client_id: Uuid,
    lobby_handle: LobbyActorHandle,
) {
    tracing::info!(
        "WebSocket: Client {} now fully handling connection for lobby {}",
        client_id,
        lobby_handle.lobby_id
    );

    // Split the WebSocket into a sender and receiver.
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Create an mpsc channel for messages from the LobbyActor to this client's WebSocket.
    // The LobbyActor will hold the `actor_to_client_tx`.
    // This task will listen on `actor_to_client_rx` and send messages to `ws_sender`.
    let (actor_to_client_tx, mut actor_to_client_rx) = mpsc::channel::<ws::Message>(32); // Buffer size of 32 for messages to this client

    // Notify the lobby actor that a new client has connected, providing the sender half of the channel.
    lobby_handle
        .client_connected(client_id, actor_to_client_tx)
        .await;

    // Task to forward messages from LobbyActor (via actor_to_client_rx) to the WebSocket client (ws_sender)
    let lobby_id_clone_for_send_task = lobby_handle.lobby_id; // Clone for the send task
    let client_id_clone_for_send_task = client_id; // Clone for the send task
    let mut send_task = tokio::spawn(async move {
        while let Some(message_to_send) = actor_to_client_rx.recv().await {
            if ws_sender.send(message_to_send).await.is_err() {
                tracing::info!(
                    "Client {} in lobby {}: WS send error (from actor), client likely disconnected.",
                    client_id_clone_for_send_task, lobby_id_clone_for_send_task
                );
                break; // Error sending, socket is likely closed.
            }
        }
        // If the loop breaks, it means actor_to_client_tx was dropped (actor shut down or removed client)
        // or ws_sender.send failed.
        tracing::debug!(
            "Client {} in lobby {}: Send task from actor to WS client terminating.",
            client_id_clone_for_send_task,
            lobby_id_clone_for_send_task
        );
    });

    // Task to handle incoming messages from the WebSocket client (ws_receiver)
    // and forward them to the LobbyActor.
    let lobby_handle_clone_for_recv_task = lobby_handle.clone(); // Clone for the receive task
    let client_id_clone_for_recv_task = client_id; // Clone for the receive task
    let lobby_id_clone_for_recv_task = lobby_handle.lobby_id;
    let mut recv_task = tokio::spawn(async move {
        loop {
            match ws_receiver.next().await {
                Some(Ok(msg)) => {
                    match msg {
                        ws::Message::Text(text_msg) => {
                            tracing::debug!(
                                "Client {} in lobby {}: Received text from WS: {:?}",
                                client_id_clone_for_recv_task,
                                lobby_id_clone_for_recv_task,
                                text_msg
                            );

                            // Send the event to the lobby actor
                            // No direct response expected here anymore, actor sends via its channel
                            if let Err(e) = lobby_handle_clone_for_recv_task
                                .process_event(client_id_clone_for_recv_task, text_msg.to_string())
                                .await
                            {
                                tracing::error!(
                                    "Client {} in lobby {}: Error sending event to actor: {}",
                                    client_id_clone_for_recv_task,
                                    lobby_id_clone_for_recv_task,
                                    e
                                );
                                // Optionally, you could try to send an error back to the client
                                // via the actor_to_client_tx if it was still held by the actor,
                                // but it's simpler to let the actor handle all outbound comms.
                                // For now, we just log.
                            }
                        }
                        ws::Message::Binary(_) => {
                            tracing::debug!(
                                "Client {} in lobby {}: Received binary message (ignored)",
                                client_id_clone_for_recv_task,
                                lobby_id_clone_for_recv_task
                            );
                        }
                        ws::Message::Ping(_) | ws::Message::Pong(_) => {}
                        ws::Message::Close(_) => {
                            tracing::info!(
                                "Client {} in lobby {}: WebSocket closed by client (recv).",
                                client_id_clone_for_recv_task,
                                lobby_id_clone_for_recv_task
                            );
                            break; // Client initiated close
                        }
                    }
                }
                Some(Err(e)) => {
                    tracing::warn!(
                        "Client {} in lobby {}: WebSocket error (recv): {}",
                        client_id_clone_for_recv_task,
                        lobby_id_clone_for_recv_task,
                        e
                    );
                    break; // Connection error
                }
                None => {
                    tracing::info!(
                        "Client {} in lobby {}: WebSocket connection closed (recv - no more messages).",
                        client_id_clone_for_recv_task,
                        lobby_id_clone_for_recv_task
                    );
                    break; // Connection closed
                }
            }
        }
        tracing::debug!(
            "Client {} in lobby {}: Receive task from WS client to actor terminating.",
            client_id_clone_for_recv_task,
            lobby_id_clone_for_recv_task
        );
    });

    // Wait for either task to complete.
    // If one task finishes (e.g., recv_task due to client disconnect),
    // we want to abort the other to clean up resources.
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

    // Notify the lobby actor that the client has disconnected
    // This ensures the actor cleans up the client's sender channel from its map.
    lobby_handle.client_disconnected(client_id).await;
    tracing::info!(
        "WebSocket: Client {} fully disconnected from lobby {}",
        client_id,
        lobby_handle.lobby_id
    );
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    port: u16,
    cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AppConfig {
    server: ServerConfig,
    admin_password: Vec<String>,
}

// --- Main Application Setup ---
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                format!("{}=info,tower_http=debug", env!("CARGO_PKG_NAME")).into()
            }),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let lobby_manager_handle = LobbyManagerHandle::new(32); // Create the central manager actor

    let settings = Config::builder()
        .add_source(
            config::Environment::with_prefix("KOLMODIN")
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("admin_password")
                .with_list_parse_key("server.cors_origins")
                .try_parsing(true),
        )
        .add_source(config::File::with_name("config").required(false))
        .build()
        .map_err(|e| format!("Failed to build config: {e}"))?;

    let app_config: AppConfig = settings
        .try_deserialize()
        .map_err(|e| format!("Failed to parse config: {e}"))?;
    let app_state = AppState {
        lobby_manager: lobby_manager_handle,
    };
    let cors_origins: Vec<HeaderValue> = app_config
        .server
        .cors_origins
        .iter()
        .map(|origin| {
            origin
                .parse()
                .map_err(|e| format!("Invalid CORS origin '{origin}': {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let cors = CorsLayer::new()
        .allow_methods(vec![http::Method::GET, http::Method::POST])
        .allow_origin(cors_origins)
        .allow_credentials(true)
        .allow_headers(vec![
            http::header::CONTENT_TYPE,
            http::header::AUTHORIZATION,
            http::header::ACCEPT,
        ]);

    let app = Router::new()
        .route("/api/create-lobby", post(create_lobby_handler))
        // Note: Path segment for lobby_id
        .route("/ws/{lobby_id}", any(ws_handler))
        .with_state(app_state)
        .layer(cors);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::info!("Listening on {}", addr);

    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();

    Ok(())
}
