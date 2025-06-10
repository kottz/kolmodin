// src/main.rs

use chrono::Utc; // Removed DateTime
use std::collections::BTreeMap; // For ordered tags
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Notify, RwLock, mpsc};
use tokio::time::{Duration, interval};
use uuid::Uuid;

// --- Server Configuration ---
const SPOOF_HOST: &str = "127.0.0.1";
const SPOOF_PORT: u16 = 6667;
const SERVER_NAME: &str = "tmi.twitch.tv"; // Twitch's actual server name
const SERVER_VERSION: &str = "spoof-0.2"; // Our spoof server version

// --- Simulated User Details (for periodic messages or testing) ---
mod simulated_users {
    pub struct UserDetails {
        pub login_name: &'static str, // Lowercase, for IRC prefix and user lookup
        pub display_name: &'static str, // Display name in tags
        pub user_id: &'static str,    // Twitch User ID
        pub color: &'static str,      // Hex color string (e.g., "#FF00FF")
        pub badges: &'static str,     // Comma-separated (e.g., "broadcaster/1,subscriber/0")
        pub badge_info: &'static str, // Comma-separated (e.g., "subscriber/6")
    }

    pub const KOTTESWE: UserDetails = UserDetails {
        login_name: "kotteswe",
        display_name: "Kotteswe",
        user_id: "23654840", // From logs
        color: "#8A2BE2",    // From logs
        badges: "broadcaster/1",
        badge_info: "", // From logs (was empty)
    };

    #[allow(dead_code)] // Keep for potential future use
    pub const ANOTHER_USER: UserDetails = UserDetails {
        login_name: "another_user",
        display_name: "AnotherUser",
        user_id: "98765432",
        color: "#00FF00",
        badges: "subscriber/3,premium/1",
        badge_info: "subscriber/3",
    };
}

// --- IRC Message Structures & Utilities ---
mod irc {
    use super::*; // To access Utc, Uuid, BTreeMap etc.

    // Common IRC Tag Keys (subset)
    pub const TAG_BADGE_INFO: &str = "badge-info";
    pub const TAG_BADGES: &str = "badges";
    pub const TAG_COLOR: &str = "color";
    pub const TAG_DISPLAY_NAME: &str = "display-name";
    pub const TAG_EMOTES: &str = "emotes";
    pub const TAG_FIRST_MSG: &str = "first-msg";
    pub const TAG_FLAGS: &str = "flags";
    pub const TAG_ID: &str = "id"; // Message ID
    pub const TAG_MOD: &str = "mod";
    pub const TAG_RETURNING_CHATTER: &str = "returning-chatter";
    pub const TAG_ROOM_ID: &str = "room-id";
    pub const TAG_SUBSCRIBER: &str = "subscriber";
    pub const TAG_TMI_SENT_TS: &str = "tmi-sent-ts";
    pub const TAG_TURBO: &str = "turbo";
    pub const TAG_USER_ID: &str = "user-id";
    pub const TAG_USER_TYPE: &str = "user-type"; // e.g., "mod", "admin", "global_mod", "" (normal)

    // Common IRC Commands
    pub const CMD_CAP: &str = "CAP";
    pub const CMD_JOIN: &str = "JOIN";
    pub const CMD_NICK: &str = "NICK";
    pub const CMD_PASS: &str = "PASS";
    pub const CMD_PING: &str = "PING";
    pub const CMD_PONG: &str = "PONG";
    pub const CMD_PRIVMSG: &str = "PRIVMSG";
    pub const CMD_QUIT: &str = "QUIT";
    pub const CMD_ROOMSTATE: &str = "ROOMSTATE";

    // Common IRC Numerics (Replies)
    pub const RPL_WELCOME: &str = "001";
    pub const RPL_YOURHOST: &str = "002";
    pub const RPL_CREATED: &str = "003";
    pub const RPL_MYINFO: &str = "004";
    pub const RPL_NAMREPLY: &str = "353";
    pub const RPL_ENDOFNAMES: &str = "366";
    pub const RPL_MOTDSTART: &str = "375";
    pub const RPL_MOTD: &str = "372";
    pub const RPL_ENDOFMOTD: &str = "376";

    #[derive(Debug, Clone)]
    pub struct IrcMessage {
        pub tags: Option<BTreeMap<String, String>>, // BTreeMap for ordered tags (good for testing/consistency)
        pub prefix: Option<String>,
        pub command: String,
        pub params: Vec<String>,
    }

    impl fmt::Display for IrcMessage {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            if let Some(tags) = &self.tags {
                if !tags.is_empty() {
                    write!(f, "@")?;
                    for (i, (k, v)) in tags.iter().enumerate() {
                        if i > 0 {
                            write!(f, ";")?;
                        }
                        // IRC tag values need escaping for ';' ' ' '\r' '\n' '\'
                        // For simplicity here, we'll assume values are pre-sanitized or simple.
                        // Proper escaping: v.replace('\\', "\\\\").replace(';', "\\:").replace(' ', "\\s").replace('\r', "\\r").replace('\n', "\\n")
                        write!(f, "{}={}", k, v)?;
                    }
                    write!(f, " ")?;
                }
            }
            if let Some(prefix) = &self.prefix {
                write!(f, ":{} ", prefix)?;
            }
            write!(f, "{}", self.command)?;
            for (i, param) in self.params.iter().enumerate() {
                if i == self.params.len() - 1 && (param.contains(' ') || param.starts_with(':')) {
                    write!(f, " :{}", param)?;
                } else {
                    write!(f, " {}", param)?;
                }
            }
            write!(f, "\r\n")
        }
    }

    impl IrcMessage {
        pub fn new(command: impl Into<String>, params: Vec<impl Into<String>>) -> Self {
            IrcMessage {
                tags: None,
                prefix: Some(SERVER_NAME.to_string()), // Default prefix for server messages
                command: command.into(),
                params: params.into_iter().map(|s| s.into()).collect(),
            }
        }

        #[allow(dead_code)] // Might be useful later
        pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
            self.prefix = Some(prefix.into());
            self
        }

        #[allow(dead_code)] // Might be useful later
        pub fn without_prefix(mut self) -> Self {
            self.prefix = None;
            self
        }

        pub fn add_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
            let tags = self.tags.get_or_insert_with(BTreeMap::new);
            tags.insert(key.into(), value.into());
            self
        }

        pub fn from_user_privmsg(
            sender: &simulated_users::UserDetails,
            channel: &str, // Just channel name, no #
            text: &str,
            room_id: &str,
        ) -> Self {
            let message_id = Uuid::new_v4().to_string();
            let timestamp = Utc::now().timestamp_millis().to_string();

            let mut msg = IrcMessage {
                tags: Some(BTreeMap::new()),
                prefix: Some(format!(
                    "{}!{}@{}.{}",
                    sender.login_name, sender.login_name, sender.login_name, SERVER_NAME
                )),
                command: CMD_PRIVMSG.to_string(),
                params: vec![format!("#{}", channel), text.to_string()],
            };

            let tags = msg.tags.as_mut().unwrap();
            if !sender.badge_info.is_empty() {
                tags.insert(TAG_BADGE_INFO.to_string(), sender.badge_info.to_string());
            }
            if !sender.badges.is_empty() {
                tags.insert(TAG_BADGES.to_string(), sender.badges.to_string());
            }
            tags.insert(TAG_COLOR.to_string(), sender.color.to_string());
            tags.insert(
                TAG_DISPLAY_NAME.to_string(),
                sender.display_name.to_string(),
            );
            tags.insert(TAG_EMOTES.to_string(), "".to_string()); // Assume no emotes for simplicity
            tags.insert(TAG_ID.to_string(), message_id);
            tags.insert(TAG_MOD.to_string(), "0".to_string()); // Assuming not mod for now
            tags.insert(TAG_ROOM_ID.to_string(), room_id.to_string());
            tags.insert(
                TAG_SUBSCRIBER.to_string(),
                if sender.badges.contains("subscriber") {
                    "1"
                } else {
                    "0"
                }
                .to_string(),
            );
            tags.insert(TAG_TMI_SENT_TS.to_string(), timestamp);
            tags.insert(TAG_TURBO.to_string(), "0".to_string());
            tags.insert(TAG_USER_ID.to_string(), sender.user_id.to_string());
            tags.insert(TAG_USER_TYPE.to_string(), "".to_string()); // normal user
            // Optional: first-msg, flags, returning-chatter
            tags.insert(TAG_FIRST_MSG.to_string(), "0".to_string());
            tags.insert(TAG_FLAGS.to_string(), "".to_string());
            tags.insert(TAG_RETURNING_CHATTER.to_string(), "0".to_string());

            msg
        }
    }
}

// --- Client Session State & Handling ---
#[derive(Debug, Clone, PartialEq)]
enum ClientHandlerState {
    Connected,
    CapabilitiesSent,
    Authenticated,
    Joined,
}

struct ClientSession {
    nick: Option<String>,    // The NICK client chose (e.g., justinfanXXXXX)
    channel: Option<String>, // The channel name (without #) client joined
    state: ClientHandlerState,
    // Future: Could store client's requested capabilities, IP, etc.
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

    let client_session_arc = Arc::new(RwLock::new(ClientSession::new()));
    let shutdown_signal = Arc::new(Notify::new());

    let (msg_to_send_tx, mut msg_to_send_rx) = mpsc::channel::<irc::IrcMessage>(32);

    // Writer Task
    let writer_task_shutdown_signal = Arc::clone(&shutdown_signal);
    let writer_task_client_addr = client_addr_str.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(irc_msg) = msg_to_send_rx.recv() => {
                    let raw_msg_str = irc_msg.to_string();
                    // Log outgoing server message before sending
                    println!("[{}] S: {}", writer_task_client_addr, raw_msg_str.trim_end());
                    if writer_half.write_all(raw_msg_str.as_bytes()).await.is_err() {
                        eprintln!("[{}] Writer task: Error writing. Shutting down client handler.", writer_task_client_addr);
                        writer_task_shutdown_signal.notify_waiters();
                        break;
                    }
                    if writer_half.flush().await.is_err() {
                        eprintln!("[{}] Writer task: Error flushing. Shutting down client handler.", writer_task_client_addr);
                        writer_task_shutdown_signal.notify_waiters();
                        break;
                    }
                }
                _ = writer_task_shutdown_signal.notified() => {
                    println!("[{}] Writer task: Shutdown signal received.", writer_task_client_addr);
                    break;
                }
                else => {
                    println!("[{}] Writer task: Message channel closed.", writer_task_client_addr);
                    writer_task_shutdown_signal.notify_waiters();
                    break;
                }
            }
        }
        println!("[{}] Writer task finished.", writer_task_client_addr);
    });

    // Periodic PRIVMSG Sender Task
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
                    let session_guard = periodic_session_arc.read().await;
                    if session_guard.state == ClientHandlerState::Joined {
                        if let Some(joined_channel) = &session_guard.channel {
                            message_counter += 1;
                            let text = format!("Periodic spoofed message #{} from {}!", message_counter, simulated_users::KOTTESWE.display_name);
                            // For room_id, we'll assume the channel name corresponds to a user (Kotteswe in this case)
                            // and use that user's ID as the room_id, which is common on Twitch.
                            let room_id_for_channel = simulated_users::KOTTESWE.user_id;

                            let privmsg = irc::IrcMessage::from_user_privmsg(
                                &simulated_users::KOTTESWE,
                                joined_channel,
                                &text,
                                room_id_for_channel,
                            );
                            if periodic_sender_tx.send(privmsg).await.is_err() {
                                println!("[{}] Periodic sender: Writer channel closed. Shutting down.", periodic_client_addr);
                                periodic_shutdown_signal.notify_waiters();
                                break;
                            }
                        }
                    }
                }
                _ = periodic_shutdown_signal.notified() => {
                    println!("[{}] Periodic sender: Shutdown signal received.", periodic_client_addr);
                    break;
                }
            }
        }
        println!("[{}] Periodic sender task finished.", periodic_client_addr);
    });

    // Main Client Handling Loop
    loop {
        line_buffer.clear();
        tokio::select! {
            result = buf_reader.read_line(&mut line_buffer) => {
                match result {
                    Ok(0) => {
                        println!("[{}] Client disconnected (EOF).", client_addr_str);
                        break;
                    }
                    Ok(_) => {
                        let incoming_msg_raw = line_buffer.trim_end_matches(['\r', '\n']).to_string();
                        if incoming_msg_raw.is_empty() { continue; }
                        println!("[{}] C: {}", client_addr_str, incoming_msg_raw);

                        let parts: Vec<&str> = incoming_msg_raw.split_whitespace().collect();
                        if parts.is_empty() { continue; }
                        let command_str = parts[0].to_uppercase();

                        let mut session = client_session_arc.write().await;

                        match command_str.as_str() {
                            irc::CMD_CAP => {
                                if parts.len() >= 3 && parts[1].to_uppercase() == "REQ" {
                                    let caps_requested = parts[2..].join(" ").trim_start_matches(':').to_string();
                                    let ack_msg = irc::IrcMessage::new(irc::CMD_CAP, vec!["*", "ACK", &format!(":{}", caps_requested)]);
                                    if msg_to_send_tx.send(ack_msg).await.is_err() { break; }
                                    session.state = ClientHandlerState::CapabilitiesSent;
                                }
                            }
                            irc::CMD_PASS => { /* Implicitly handled by NICK state transition */ }
                            irc::CMD_NICK => {
                                if parts.len() > 1 && session.state == ClientHandlerState::CapabilitiesSent {
                                    let nick = parts[1].to_string();
                                    session.nick = Some(nick.clone());

                                    let welcome_msgs = vec![
                                        irc::IrcMessage::new(irc::RPL_WELCOME, vec![&nick, ":Welcome, GLHF!"]),
                                        irc::IrcMessage::new(irc::RPL_YOURHOST, vec![&nick, &format!(":Your host is {}, running version {}", SERVER_NAME, SERVER_VERSION)]),
                                        irc::IrcMessage::new(irc::RPL_CREATED, vec![&nick, ":This server was created on a mystical date"]),
                                        irc::IrcMessage::new(irc::RPL_MYINFO, vec![&nick, SERVER_NAME, SERVER_VERSION, "aoitsnfk", ""]), // Modes
                                        irc::IrcMessage::new(irc::RPL_MOTDSTART, vec![&nick, &format!(":- {} Message of the Day -", SERVER_NAME)]),
                                        irc::IrcMessage::new(irc::RPL_MOTD, vec![&nick, ":You are in a maze of twisty passages, all alike."]),
                                        irc::IrcMessage::new(irc::RPL_ENDOFMOTD, vec![&nick, ":>"]), // Twitch uses ">"
                                    ];
                                    for msg in welcome_msgs {
                                        if msg_to_send_tx.send(msg).await.is_err() { break; }
                                    }
                                    session.state = ClientHandlerState::Authenticated;
                                }
                            }
                            irc::CMD_JOIN => {
                                if session.state == ClientHandlerState::Authenticated && parts.len() > 1 {
                                    let channel_name_raw = parts[1];
                                    let channel_name = channel_name_raw.trim_start_matches('#').to_string();
                                    session.channel = Some(channel_name.clone());

                                    // Ensure client_nick_str has a lifetime that outlives users_in_chat
                                    let client_nick_str = session.nick.as_ref().cloned().unwrap_or_else(|| "unknown_client".to_string());

                                    // 1. Client's own JOIN echo
                                    let join_echo = irc::IrcMessage {
                                        tags: None,
                                        prefix: Some(format!("{}!{}@{}.{}", client_nick_str, client_nick_str, client_nick_str, SERVER_NAME)),
                                        command: irc::CMD_JOIN.to_string(),
                                        params: vec![format!("#{}", channel_name)],
                                    };
                                    if msg_to_send_tx.send(join_echo).await.is_err() { break; }

                                    // 2. ROOMSTATE
                                    let room_id_for_channel = simulated_users::KOTTESWE.user_id;
                                    let roomstate_msg = irc::IrcMessage::new(irc::CMD_ROOMSTATE, vec![format!("#{}", channel_name)])
                                        .add_tag("emote-only", "0")
                                        .add_tag("followers-only", "-1")
                                        .add_tag("r9k", "0")
                                        .add_tag(irc::TAG_ROOM_ID, room_id_for_channel)
                                        .add_tag("slow", "0")
                                        .add_tag("subs-only", "0");
                                    if msg_to_send_tx.send(roomstate_msg).await.is_err() { break; }

                                    // 3. NAMES list
                                    let names_prefix = format!("{}.{}", client_nick_str, SERVER_NAME);

                                    let users_in_chat_names_vec: Vec<&str> = vec![simulated_users::KOTTESWE.login_name, &client_nick_str];

                                    for user_in_chat_name_ref in users_in_chat_names_vec {
                                        let twitch_namreply = irc::IrcMessage {
                                            tags: None,
                                            prefix: Some(names_prefix.clone()),
                                            command: irc::RPL_NAMREPLY.to_string(),
                                            // Params for 353: <client_nick> <symbol> <channel> :<user list>
                                            params: vec![client_nick_str.clone(), "=".to_string(), format!("#{}", channel_name), format!(":{}", user_in_chat_name_ref)],
                                        };
                                        if msg_to_send_tx.send(twitch_namreply).await.is_err() { break; }
                                    }
                                    let endofnames = irc::IrcMessage {
                                        tags: None,
                                        prefix: Some(names_prefix.clone()),
                                        command: irc::RPL_ENDOFNAMES.to_string(),
                                        // Params for 366: <client_nick> <channel> :End of /NAMES list
                                        params: vec![client_nick_str.clone(), format!("#{}", channel_name), ":End of /NAMES list".to_string()],
                                    };

                                    if msg_to_send_tx.send(endofnames).await.is_err() { break; }
                                    session.state = ClientHandlerState::Joined;
                                }
                            }
                            irc::CMD_PING => {
                                let payload = if parts.len() > 1 { parts[1] } else { "" };
                                // PONG format: :<server_name> PONG <server_name> [:<payload_from_ping>]
                                // The payload from client PING often starts with ':' if it's the second param
                                let pong_response = irc::IrcMessage::new(irc::CMD_PONG, vec![SERVER_NAME, payload]);
                                if msg_to_send_tx.send(pong_response).await.is_err() { break; }
                            }
                            irc::CMD_PRIVMSG => {
                                println!("[{}] Client sent PRIVMSG, ignoring: {}", client_addr_str, incoming_msg_raw);
                            }
                            irc::CMD_QUIT => {
                                println!("[{}] Client sent QUIT. Closing.", client_addr_str);
                                break;
                            }
                            _ => {
                                println!("[{}] Unknown command from client: {}", client_addr_str, command_str);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[{}] Error reading from client: {}. Closing.", client_addr_str, e);
                        break;
                    }
                }
            },
            _ = shutdown_signal.notified() => {
                println!("[{}] Main handler: Shutdown signal received.", client_addr_str);
                break;
            }
        }
    }

    shutdown_signal.notify_waiters();
    println!("[{}] Client handler finished.", client_addr_str);
    Ok(())
}

// --- Server Main ---
#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener_addr = format!("{}:{}", SPOOF_HOST, SPOOF_PORT);
    let listener = TcpListener::bind(&listener_addr).await?;
    println!(
        "Spoof Twitch IRC server v{} listening on {}",
        SERVER_VERSION, listener_addr
    );

    loop {
        let (stream, client_addr) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_client(stream, client_addr).await {
                if let Some(io_err) = e.downcast_ref::<std::io::Error>() {
                    if io_err.kind() == std::io::ErrorKind::BrokenPipe
                        || io_err.kind() == std::io::ErrorKind::ConnectionReset
                    {
                        // Common client disconnects
                    } else {
                        eprintln!("[{}] Error in client handler: {}", client_addr, e);
                    }
                } else {
                    eprintln!("[{}] Non-IO error in client handler: {}", client_addr, e);
                }
            }
        });
    }
}
