use chrono::Utc;
use std::error::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};
use uuid::Uuid;

const SPOOF_HOST: &str = "127.0.0.1";
const SPOOF_PORT: u16 = 6667; // Standard IRC port, ensure it's free
const SERVER_NAME: &str = "spoof.tmi.twitch.tv"; // Name for our mock server

#[derive(Debug, Clone)]
enum ClientState {
    Connected,
    CapabilitiesRequested,
    Authenticated(String),  // Stores client's NICK
    Joined(String, String), // Stores client's NICK, Channel name
}

async fn handle_client(
    stream: TcpStream,
    client_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    println!("[{}] Client connected", client_addr);
    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut line_buffer = String::new();

    let mut client_state = ClientState::Connected;

    // Channel for periodic messages to be sent to this client
    let (periodic_msg_tx, mut periodic_msg_rx) = mpsc::channel::<String>(10);

    // Task to generate periodic PRIVMSGs
    // This task will start sending templates; they get finalized when client is Joined.
    let periodic_sender_task = tokio::spawn(async move {
        let mut tick_interval = interval(Duration::from_secs(5));
        loop {
            tick_interval.tick().await;
            match create_spoof_privmsg_template() {
                Ok(msg_template) => {
                    if periodic_msg_tx.send(msg_template).await.is_err() {
                        println!(
                            "[{}] Periodic sender: Receiver for client has dropped. Stopping periodic messages for this client.",
                            client_addr
                        );
                        break;
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[{}] Error creating spoof message template: {}",
                        client_addr, e
                    );
                }
            }
        }
    });

    loop {
        tokio::select! {
            // Read incoming data from the client
            result = buf_reader.read_line(&mut line_buffer) => {
                match result {
                    Ok(0) => {
                        println!("[{}] Client disconnected (EOF)", client_addr);
                        break; // Connection closed
                    }
                    Ok(_) => {
                        let incoming_msg = line_buffer.trim().to_string();
                        if incoming_msg.is_empty() {
                            line_buffer.clear();
                            continue;
                        }
                        println!("[{}] C: {}", client_addr, incoming_msg);

                        let (new_state, response_messages) = process_client_message(&incoming_msg, client_state.clone());
                        client_state = new_state;

                        for response in response_messages {
                            println!("[{}] S: {}", client_addr, response.trim());
                            writer.write_all(response.as_bytes()).await?;
                        }
                        writer.flush().await?; // Ensure all responses are sent immediately
                        line_buffer.clear();
                    }
                    Err(e) => {
                        eprintln!("[{}] Error reading from client: {}", client_addr, e);
                        break; // Error, close connection
                    }
                }
            },
            // Receive a message template from the periodic sender task
            Some(msg_template) = periodic_msg_rx.recv() => {
                if let ClientState::Joined(_nick, channel_name) = &client_state {
                    let final_msg = msg_template.replace("#{{channel}}", &format!("#{}", channel_name));
                    println!("[{}] S (periodic): {}", client_addr, final_msg.trim());
                    writer.write_all(final_msg.as_bytes()).await?;
                    writer.flush().await?;
                }
                // If not joined, the message is effectively skipped for now.
            }
        }
    }

    periodic_sender_task.abort(); // Stop the periodic sender when client disconnects
    println!("[{}] Client disconnected / handler finished", client_addr);
    Ok(())
}

fn process_client_message(msg: &str, current_state: ClientState) -> (ClientState, Vec<String>) {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    if parts.is_empty() {
        return (current_state, vec![]);
    }

    let command = parts[0].to_uppercase();
    let mut responses = Vec::new();
    let mut next_state = current_state.clone(); // Clone current state to modify

    match command.as_str() {
        "CAP" => {
            if parts.len() > 1 && parts[1].to_uppercase() == "REQ" {
                let caps_to_ack = if parts.len() > 2 && parts[2].starts_with(':') {
                    parts[2..].join(" ").trim_start_matches(':').to_string()
                } else if parts.len() > 2 {
                    parts[2..].join(" ")
                } else {
                    // Default to what Kolmodin client requests
                    "twitch.tv/membership twitch.tv/tags twitch.tv/commands".to_string()
                };
                responses.push(format!(":{SERVER_NAME} CAP * ACK :{}\r\n", caps_to_ack));
                next_state = ClientState::CapabilitiesRequested;
            }
        }
        "PASS" => {
            // Kolmodin sends PASS. We don't need to do much other than acknowledge state progression if needed.
            // Assuming PASS comes after CAP REQ and before NICK.
            // No specific response needed, but state transition happens implicitly before NICK.
        }
        "NICK" => {
            if parts.len() > 1 {
                let nick = parts[1].to_string();
                responses.push(format!(
                    ":{SERVER_NAME} 001 {} :Welcome to the Spoof Twitch IRC Server, {}!\r\n",
                    nick, nick
                ));
                responses.push(format!(":{SERVER_NAME} 002 {} :Your host is {SERVER_NAME}, running version spoof-0.1\r\n", nick));
                responses.push(format!(
                    ":{SERVER_NAME} 003 {} :This server was created on a mystical date.\r\n",
                    nick
                ));
                responses.push(format!(
                    ":{SERVER_NAME} 004 {} {SERVER_NAME} spoof-0.1 aoit\r\n",
                    nick
                )); // available user/channel modes

                responses.push(format!(
                    ":{SERVER_NAME} 375 {} :- {SERVER_NAME} Message of the Day -\r\n",
                    nick
                ));
                responses.push(format!(
                    ":{SERVER_NAME} 372 {} :- This is a mock Twitch server.\r\n",
                    nick
                ));
                responses.push(format!(
                    ":{SERVER_NAME} 372 {} :- All your base are belong to us.\r\n",
                    nick
                ));
                responses.push(format!(
                    ":{SERVER_NAME} 376 {} :End of /MOTD command.\r\n",
                    nick
                ));
                next_state = ClientState::Authenticated(nick);
            }
        }
        "JOIN" => {
            if let ClientState::Authenticated(nick) = current_state {
                if parts.len() > 1 {
                    let channel_to_join = parts[1].trim_start_matches('#').to_string();

                    // Client sees itself join
                    responses.push(format!(
                        ":{}!{}@{}.{SERVER_NAME} JOIN #{}\r\n",
                        nick, nick, nick, channel_to_join
                    ));

                    // NAMES list (RPL_NAMREPLY and RPL_ENDOFNAMES)
                    // Include the client and our spoofer "kotteswe"
                    responses.push(format!(
                        ":{SERVER_NAME} 353 {} = #{} :{} kotteswe\r\n",
                        nick, channel_to_join, nick
                    ));
                    responses.push(format!(
                        ":{SERVER_NAME} 366 {} #{} :End of /NAMES list.\r\n",
                        nick, channel_to_join
                    ));

                    next_state = ClientState::Joined(nick.clone(), channel_to_join);
                }
            } else {
                responses.push(format!(
                    ":{SERVER_NAME} NOTICE * :You must authenticate with NICK before JOIN.\r\n"
                ));
            }
        }
        "PING" => {
            let token = if parts.len() > 1 {
                parts[1]
            } else {
                SERVER_NAME
            };
            responses.push(format!(":{SERVER_NAME} PONG {SERVER_NAME} {}\r\n", token));
        }
        "PRIVMSG" => {
            // The client might send PRIVMSG (e.g. if it were a bot itself). We just ignore for now.
        }
        "QUIT" => {
            // Client is quitting. No specific server response needed to acknowledge.
            // The read_line(0) in handle_client will detect closure.
        }
        _ => {
            // For unknown commands, you could send RPL_UNKNOWNCOMMAND (421)
            // responses.push(format!(":{SERVER_NAME} 421 {} {} :Unknown command\r\n", nick_or_star, command));
        }
    }
    (next_state, responses)
}

// Creates a template for a spoofed PRIVMSG. The channel part will be a placeholder.
fn create_spoof_privmsg_template() -> Result<String, Box<dyn Error + Send + Sync>> {
    let sender_nick = "kotteswe";
    let message_text = "test123";

    // Mimic Twitch tags
    let user_id = "12345"; // Arbitrary user ID for kotteswe
    let room_id = "67890"; // Arbitrary room ID, not strictly needed by Kolmodin parser but good for format
    let message_s_id = Uuid::new_v4().to_string();
    let timestamp_ms = Utc::now().timestamp_millis();

    // Note: #{{channel}} is a placeholder that will be replaced later
    let tags = format!(
        "badges=;color=#FF00FF;display-name={};emotes=;flags=;id={};mod=0;room-id={};subscriber=0;tmi-sent-ts={};turbo=0;user-id={};user-type=",
        sender_nick, message_s_id, room_id, timestamp_ms, user_id
    );

    // The channel here is a placeholder, it will be filled in `handle_client`
    Ok(format!(
        "@{tags} :{sender_nick}!{sender_nick}@{sender_nick}.tmi.twitch.tv PRIVMSG #{{channel}} :{message_text}\r\n"
    ))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener_addr = format!("{}:{}", SPOOF_HOST, SPOOF_PORT);
    let listener = TcpListener::bind(&listener_addr).await?;
    println!(
        "Spoof Twitch IRC server listening on http://{}",
        listener_addr
    );

    loop {
        let (stream, client_addr) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, client_addr).await {
                eprintln!("[{}] Error handling client: {}", client_addr, e);
            }
        });
    }
}
