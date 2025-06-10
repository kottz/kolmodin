// src/main.rs

use chrono::Utc;
use std::error::Error;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock; // Use Tokio's Arc and RwLock
use tokio::sync::{Notify, mpsc};
use tokio::time::{Duration, interval};
use uuid::Uuid;

const SPOOF_HOST: &str = "127.0.0.1";
const SPOOF_PORT: u16 = 6667;
const SERVER_NAME: &str = "tmi.twitch.tv"; // Twitch's actual server name used in messages

// --- Simulated sender's (e.g., Kotteswe) details for PRIVMSGs ---
const SIMULATED_SENDER_LOGIN_NAME: &str = "kotteswe"; // Lowercase, for IRC prefix
const SIMULATED_SENDER_DISPLAY_NAME: &str = "Kotteswe"; // Display name in tags
const SIMULATED_SENDER_USER_ID: &str = "23654840"; // User ID from logs
const SIMULATED_SENDER_COLOR: &str = "#8A2BE2";
const SIMULATED_SENDER_BADGES: &str = "broadcaster/1"; // As per log
const SIMULATED_SENDER_BADGE_INFO: &str = ""; // As per log, was empty.

// Room ID for ROOMSTATE and PRIVMSG tags. Logs show this matches the broadcaster's user ID for their channel.
const CHANNEL_ROOM_ID: &str = SIMULATED_SENDER_USER_ID;

#[derive(Debug, Clone, PartialEq)]
enum ClientHandlerState {
    Connected,        // Initial state after TCP connection
    CapabilitiesSent, // After server sends CAP ACK
    Authenticated,    // After client sends NICK and PASS (implicitly)
    Joined,           // After client JOINs a channel
}

// Holds information about the connected client
struct ClientSession {
    nick: Option<String>,
    channel: Option<String>,
    state: ClientHandlerState,
}

impl ClientSession {
    fn new() -> Self {
        ClientSession {
            nick: None,
            channel: None,
            state: ClientHandlerState::Connected,
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    client_addr: std::net::SocketAddr,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let client_addr_str = client_addr.to_string();
    println!("[{}] Client connected", client_addr_str);

    let (reader, mut writer_half) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut line_buffer = String::new();

    // Use tokio::sync::RwLock
    let client_session_arc = Arc::new(RwLock::new(ClientSession::new()));
    let shutdown_signal = Arc::new(Notify::new()); // Used to signal all tasks for this client to stop

    // MPSC channel to send formatted IRC messages to the writer task
    let (msg_to_send_tx, mut msg_to_send_rx) = mpsc::channel::<String>(32);

    // --- Writer Task ---
    // This task is solely responsible for writing to the TCP stream.
    let writer_task_shutdown_signal = Arc::clone(&shutdown_signal);
    let writer_task_client_addr = client_addr_str.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(message) = msg_to_send_rx.recv() => {
                    if writer_half.write_all(message.as_bytes()).await.is_err() {
                        eprintln!("[{}] Writer task: Error writing to client. Closing connection.", writer_task_client_addr);
                        writer_task_shutdown_signal.notify_waiters(); // Signal other tasks
                        break;
                    }
                    if writer_half.flush().await.is_err() {
                        eprintln!("[{}] Writer task: Error flushing to client. Closing connection.", writer_task_client_addr);
                        writer_task_shutdown_signal.notify_waiters(); // Signal other tasks
                        break;
                    }
                }
                _ = writer_task_shutdown_signal.notified() => {
                    println!("[{}] Writer task: Shutdown signal received. Exiting.", writer_task_client_addr);
                    break;
                }
                else => { // msg_to_send_rx channel closed
                    println!("[{}] Writer task: Message channel closed. Exiting.", writer_task_client_addr);
                    writer_task_shutdown_signal.notify_waiters(); // Ensure others know if this exits unexpectedly
                    break;
                }
            }
        }
        println!("[{}] Writer task finished.", writer_task_client_addr);
    });

    // --- Periodic PRIVMSG Sender Task ---
    let periodic_sender_tx = msg_to_send_tx.clone();
    let periodic_session_arc = Arc::clone(&client_session_arc);
    let periodic_shutdown_signal = Arc::clone(&shutdown_signal);
    let periodic_client_addr = client_addr_str.clone();
    tokio::spawn(async move {
        let mut tick_interval = interval(Duration::from_secs(5));
        let mut message_counter = 0;

        loop {
            tokio::select! {
                _ = tick_interval.tick() => {
                    let (current_state, joined_channel_opt) = {
                        // Read necessary info from shared session state
                        let session_guard = periodic_session_arc.read().await; // Use .await
                        (session_guard.state.clone(), session_guard.channel.clone())
                    };

                    if current_state == ClientHandlerState::Joined {
                        if let Some(joined_channel) = joined_channel_opt {
                            message_counter += 1;
                            let message_text = format!("Spoofed message #{} from {}!", message_counter, SIMULATED_SENDER_DISPLAY_NAME);
                            let msg_uuid = Uuid::new_v4().to_string();
                            let tmi_sent_ts = Utc::now().timestamp_millis().to_string();

                            let tags = format!(
                                "badge-info={};badges={};color={};display-name={};emotes=;first-msg=0;flags=;id={};mod=0;returning-chatter=0;room-id={};subscriber=0;tmi-sent-ts={};turbo=0;user-id={};user-type=",
                                SIMULATED_SENDER_BADGE_INFO,
                                SIMULATED_SENDER_BADGES,
                                SIMULATED_SENDER_COLOR,
                                SIMULATED_SENDER_DISPLAY_NAME,
                                msg_uuid,
                                CHANNEL_ROOM_ID, // This is SIMULATED_SENDER_USER_ID
                                tmi_sent_ts,
                                SIMULATED_SENDER_USER_ID
                            );

                            let privmsg_line = format!(
                                "@{tags} :{login_name}!{login_name}@{login_name}.{server_name} PRIVMSG #{channel} :{text}\r\n",
                                tags = tags,
                                login_name = SIMULATED_SENDER_LOGIN_NAME,
                                server_name = SERVER_NAME,
                                channel = joined_channel,
                                text = message_text
                            );
                            // Log locally before sending to writer task
                            println!("[{}] S (periodic): {}", periodic_client_addr, privmsg_line.trim_end());
                            if periodic_sender_tx.send(privmsg_line).await.is_err() {
                                println!("[{}] Periodic sender: Failed to send message to writer task (channel closed). Exiting.", periodic_client_addr);
                                periodic_shutdown_signal.notify_waiters(); // Signal main handler
                                break;
                            }
                        }
                    }
                }
                _ = periodic_shutdown_signal.notified() => {
                    println!("[{}] Periodic sender: Shutdown signal received. Exiting.", periodic_client_addr);
                    return; // Exit task
                }
            }
        }
        println!("[{}] Periodic sender task finished.", periodic_client_addr);
    });

    // --- Main Client Handling Loop (Reading incoming messages) ---
    loop {
        line_buffer.clear();
        tokio::select! {
            result = buf_reader.read_line(&mut line_buffer) => {
                match result {
                    Ok(0) => { // EOF
                        println!("[{}] Client disconnected (EOF)", client_addr_str);
                        break; // Exit main loop for this client
                    }
                    Ok(_) => { // Bytes read
                        let incoming_msg_orig = line_buffer.trim_end_matches(['\r', '\n']).to_string();
                        if incoming_msg_orig.is_empty() {
                            continue;
                        }
                        println!("[{}] C: {}", client_addr_str, incoming_msg_orig);

                        let parts: Vec<&str> = incoming_msg_orig.split_whitespace().collect();
                        if parts.is_empty() {
                            continue;
                        }
                        let command = parts[0].to_uppercase();

                        // Acquire write lock using .await
                        let mut session_guard = client_session_arc.write().await;

                        match command.as_str() {
                            "CAP" => {
                                if parts.len() >= 2 && parts[1].to_uppercase() == "REQ" {
                                    let caps_requested = if parts.len() > 2 && parts[2].starts_with(':') {
                                        parts[2..].join(" ").trim_start_matches(':').to_string()
                                    } else if parts.len() > 2 {
                                        parts[2..].join(" ")
                                    } else {
                                        // Default to what Kolmodin client requests
                                        "twitch.tv/membership twitch.tv/tags twitch.tv/commands".to_string()
                                    };
                                    let response = format!(":{SERVER_NAME} CAP * ACK :{caps_requested}\r\n");
                                    println!("[{}] S: {}", client_addr_str, response.trim_end());
                                    if msg_to_send_tx.send(response).await.is_err() { break; } // Guard held across await is fine with tokio::sync::RwLock
                                    session_guard.state = ClientHandlerState::CapabilitiesSent;
                                }
                            }
                            "PASS" => {
                                // Implicitly handled, usually comes before NICK.
                                // State progression to Authenticated happens upon receiving NICK.
                            }
                            "NICK" => {
                                if parts.len() > 1 {
                                    let nick = parts[1].to_string();
                                    session_guard.nick = Some(nick.clone());

                                    let welcome_msgs = vec![
                                        format!(":{SERVER_NAME} 001 {nick} :Welcome, GLHF!\r\n"),
                                        format!(":{SERVER_NAME} 002 {nick} :Your host is {SERVER_NAME}, running version spoof-0.1\r\n"),
                                        format!(":{SERVER_NAME} 003 {nick} :This server was created on a mystical date\r\n"),
                                        format!(":{SERVER_NAME} 004 {nick} {SERVER_NAME} spoof-0.1 aoit\r\n"),
                                        format!(":{SERVER_NAME} 375 {nick} :- {SERVER_NAME} Message of the Day -\r\n"),
                                        format!(":{SERVER_NAME} 372 {nick} :You are in a maze of twisty passages, all alike.\r\n"),
                                        format!(":{SERVER_NAME} 376 {nick} :>\r\n"),
                                    ];
                                    for msg in welcome_msgs {
                                        println!("[{}] S: {}", client_addr_str, msg.trim_end());
                                        if msg_to_send_tx.send(msg).await.is_err() { break; }
                                    }
                                    if session_guard.state == ClientHandlerState::CapabilitiesSent { // Ensure CAP ACK was sent
                                        session_guard.state = ClientHandlerState::Authenticated;
                                    }
                                }
                            }
                            "JOIN" => {
                                if session_guard.state == ClientHandlerState::Authenticated && parts.len() > 1 {
                                    let channel_to_join_raw = parts[1];
                                    let channel_name = channel_to_join_raw.trim_start_matches('#').to_string();
                                    session_guard.channel = Some(channel_name.clone());

                                    let client_nick = session_guard.nick.as_ref().cloned().unwrap_or_else(|| "unknown_client".to_string());

                                    // 1. Echo client's own JOIN (from client's perspective)
                                    let join_echo = format!(
                                        ":{nick}!{nick}@{nick}.{server_name} JOIN #{channel}\r\n",
                                        nick = client_nick, // This should be the client's NICK from session_guard.nick
                                        server_name = SERVER_NAME, // e.g. justinfan123.tmi.twitch.tv
                                        channel = channel_name
                                    );
                                    println!("[{}] S: {}", client_addr_str, join_echo.trim_end());
                                    if msg_to_send_tx.send(join_echo).await.is_err() { break; }

                                    // 2. Send ROOMSTATE
                                    let roomstate_tags = format!(
                                        "emote-only=0;followers-only=-1;r9k=0;room-id={};slow=0;subs-only=0",
                                        CHANNEL_ROOM_ID
                                    );
                                    let roomstate_msg = format!(
                                        "@{tags} :{SERVER_NAME} ROOMSTATE #{channel}\r\n",
                                        tags = roomstate_tags,
                                        channel = channel_name
                                    );
                                    println!("[{}] S: {}", client_addr_str, roomstate_msg.trim_end());
                                    if msg_to_send_tx.send(roomstate_msg).await.is_err() { break; }

                                    // 3. Send NAMES list (RPL_NAMREPLY) - one per user as per logs
                                    // Prefix for 353/366 is :<client_nick>.<server_name> from logs
                                    // The logs show :justinfan70698.tmi.twitch.tv 353 justinfan70698 = #kotteswe :kotteswe
                                    // So the prefix is indeed the client's full NICK.tmi.twitch.tv, and the next param is client's NICK again.

                                    let names_prefix = format!("{}.{}", client_nick, SERVER_NAME);

                                    // Simulated sender (e.g., Kotteswe)
                                    let rpl_namreply_sender = format!(
                                        ":{prefix} 353 {client_nick_param} = #{channel} :{user_in_chat}\r\n",
                                        prefix = names_prefix, // e.g. justinfan123.tmi.twitch.tv
                                        client_nick_param = client_nick, // e.g. justinfan123
                                        channel = channel_name,
                                        user_in_chat = SIMULATED_SENDER_LOGIN_NAME
                                    );
                                    println!("[{}] S: {}", client_addr_str, rpl_namreply_sender.trim_end());
                                    if msg_to_send_tx.send(rpl_namreply_sender).await.is_err() { break; }

                                    // The client itself
                                    let rpl_namreply_self = format!(
                                        ":{prefix} 353 {client_nick_param} = #{channel} :{user_in_chat}\r\n",
                                         prefix = names_prefix,
                                        client_nick_param = client_nick,
                                        channel = channel_name,
                                        user_in_chat = client_nick // Client's NICK
                                    );
                                    println!("[{}] S: {}", client_addr_str, rpl_namreply_self.trim_end());
                                    if msg_to_send_tx.send(rpl_namreply_self).await.is_err() { break; }

                                    // End of NAMES list (RPL_ENDOFNAMES)
                                    let rpl_endofnames = format!(
                                        ":{prefix} 366 {client_nick_param} #{channel} :End of /NAMES list\r\n",
                                        prefix = names_prefix,
                                        client_nick_param = client_nick,
                                        channel = channel_name
                                    );
                                    println!("[{}] S: {}", client_addr_str, rpl_endofnames.trim_end());
                                    if msg_to_send_tx.send(rpl_endofnames).await.is_err() { break; }

                                    session_guard.state = ClientHandlerState::Joined;
                                }
                            }
                            "PING" => {
                                let payload = if parts.len() > 1 { parts[1] } else { "" }; // Payload often starts with ':'
                                let pong_response = format!(":{SERVER_NAME} PONG {SERVER_NAME} {payload}\r\n", payload=payload);
                                println!("[{}] S: {}", client_addr_str, pong_response.trim_end());
                                if msg_to_send_tx.send(pong_response).await.is_err() { break; }
                            }
                            "PRIVMSG" => {
                                // Client sent a message (e.g., if it's also a bot)
                                println!("[{}] Client sent PRIVMSG, ignoring: {}", client_addr_str, incoming_msg_orig);
                            }
                            "QUIT" => {
                                println!("[{}] Client sent QUIT. Closing connection.", client_addr_str);
                                break; // Exit main loop
                            }
                            _ => {
                                println!("[{}] Unknown command from client: {}", client_addr_str, command);
                            }
                        }
                        // session_guard (RwLockWriteGuard) is dropped here when it goes out of scope
                    }
                    Err(e) => { // Error reading from client
                        eprintln!("[{}] Error reading from client: {}. Closing connection.", client_addr_str, e);
                        break; // Exit main loop
                    }
                }
            },
            _ = shutdown_signal.notified() => {
                println!("[{}] Main handler loop: Shutdown signal received (likely from writer/periodic task failure). Exiting.", client_addr_str);
                break; // Exit main loop
            }
        }
    }

    // Signal all associated tasks for this client to shut down
    shutdown_signal.notify_waiters();
    println!(
        "[{}] Client disconnected / handler finished.",
        client_addr_str
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener_addr = format!("{}:{}", SPOOF_HOST, SPOOF_PORT);
    let listener = TcpListener::bind(&listener_addr).await?;
    println!("Spoof Twitch IRC server listening on {}", listener_addr);

    loop {
        let (stream, client_addr) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, client_addr).await {
                // Don't print error if it's due to a broken pipe, which is common if client disconnects abruptly
                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == std::io::ErrorKind::BrokenPipe {
                        // Suppress broken pipe, client disconnected.
                    } else {
                        eprintln!("[{}] Error handling client: {}", client_addr, e);
                    }
                } else {
                    eprintln!("[{}] Error handling client: {}", client_addr, e);
                }
            }
        });
    }
}
