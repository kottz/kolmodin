// src/api.rs

use axum::{Router, extract::State, http::StatusCode, response::Json, routing::post};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::irc_server::CustomMessage;

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub channel: String,
    pub username: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub success: bool,
    pub message: String,
}

pub type CustomMessageSender = mpsc::Sender<CustomMessage>;

async fn send_message_handler(
    State(custom_msg_tx): State<CustomMessageSender>,
    Json(payload): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, StatusCode> {
    // Create a custom message with default values for missing fields
    let custom_msg = CustomMessage {
        channel: payload.channel.clone(),
        username: payload.username.clone(),
        display_name: payload.username.clone(), // Use username as display name by default
        message: payload.message,
        color: "#FFFFFF".to_string(), // Default to white color
    };

    // Send the message to the IRC server
    match custom_msg_tx.send(custom_msg).await {
        Ok(_) => Ok(Json(SendMessageResponse {
            success: true,
            message: format!(
                "Message sent to #{} from {}",
                payload.channel, payload.username
            ),
        })),
        Err(_) => {
            eprintln!("Failed to send message to IRC server - channel closed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

pub fn create_router(custom_msg_tx: CustomMessageSender) -> Router {
    Router::new()
        .route("/send_message", post(send_message_handler))
        .with_state(custom_msg_tx)
}

pub async fn run_api_server(
    custom_msg_tx: CustomMessageSender,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = create_router(custom_msg_tx);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8080").await?;
    println!("API server listening on http://127.0.0.1:8080");
    println!("Send POST requests to http://127.0.0.1:8080/send_message");

    axum::serve(listener, app).await?;
    Ok(())
}
