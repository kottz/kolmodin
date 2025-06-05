use axum::extract::{
    State,
    ws::{self, WebSocket, WebSocketUpgrade},
};
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::game_logic::messages::{
    ClientToServerMessage, ServerToClientMessage, client_message_from_ws_text,
};
use crate::lobby::LobbyActorHandle;
use crate::state::AppState;

pub async fn ws_handler(
    ws_upgrade: WebSocketUpgrade,
    State(app_state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("WebSocket: Connection attempt to generic /ws endpoint");
    ws_upgrade.on_upgrade(move |socket| handle_socket(socket, app_state))
}

pub async fn handle_socket(socket: WebSocket, app_state: AppState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let lobby_handle: LobbyActorHandle;
    let client_id: Uuid;

    match ws_receiver.next().await {
        Some(Ok(ws::Message::Text(text_msg))) => {
            tracing::debug!("WS: Received initial message: {}", text_msg);
            match client_message_from_ws_text(&text_msg) {
                Ok(ClientToServerMessage::ConnectToLobby {
                    lobby_id: received_lobby_id,
                }) => {
                    client_id = Uuid::new_v4();
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
                            let _ = ws_sender.close().await;
                            return;
                        }
                    }
                }
                Ok(other_msg) => {
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
            tracing::warn!("WS: Error receiving initial message: {}. Closing.", e);
            let _ = ws_sender.close().await;
            return;
        }
        None => {
            tracing::info!("WS: Client disconnected before sending initial message. Closing.");
            return;
        }
    }

    tracing::info!(
        "WebSocket: Client {} now fully handling connection for lobby {}",
        client_id,
        lobby_handle.lobby_id
    );

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
        let _ = ws_sender.close().await;
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
                    ws::Message::Ping(ping_data) => {
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
                        client_id_clone_recv,
                        lobby_id_clone_recv
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
