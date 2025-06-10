// src/irc_server.rs

use chrono::Utc;
use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, broadcast, mpsc}; // Removed Notify if not used, kept broadcast
use uuid::Uuid; // Added Uuid import

// --- Server Configuration ---
const SPOOF_HOST: &str = "127.0.0.1";
const SPOOF_PORT: u16 = 6667;
pub const SERVER_NAME: &str = "tmi.twitch.tv";
pub const SERVER_VERSION: &str = "spoof-tui-0.1";

// --- Types for Communication with UI ---
#[derive(Debug, Clone)]
pub enum ServerLog {
    Incoming(SocketAddr, String),
    Outgoing(SocketAddr, String),
    Internal(String),
    ClientConnected(SocketAddr),
    ClientDisconnected(SocketAddr),
}

#[derive(Debug, Clone)]
pub struct CustomMessage {
    pub channel: String,
    pub username: String,
    pub display_name: String,
    pub message: String,
    pub color: String,
}

// --- IRC Message Structures & Utilities (module content unchanged) ---
pub mod irc {
    use super::*; // To access Utc, Uuid from parent scope

    pub const TAG_BADGE_INFO: &str = "badge-info";
    pub const TAG_BADGES: &str = "badges";
    pub const TAG_COLOR: &str = "color";
    pub const TAG_DISPLAY_NAME: &str = "display-name";
    pub const TAG_EMOTES: &str = "emotes";
    pub const TAG_FIRST_MSG: &str = "first-msg";
    pub const TAG_FLAGS: &str = "flags";
    pub const TAG_ID: &str = "id";
    pub const TAG_MOD: &str = "mod";
    pub const TAG_RETURNING_CHATTER: &str = "returning-chatter";
    pub const TAG_ROOM_ID: &str = "room-id";
    pub const TAG_SUBSCRIBER: &str = "subscriber";
    pub const TAG_TMI_SENT_TS: &str = "tmi-sent-ts";
    pub const TAG_TURBO: &str = "turbo";
    pub const TAG_USER_ID: &str = "user-id";
    pub const TAG_USER_TYPE: &str = "user-type";

    pub const CMD_CAP: &str = "CAP";
    pub const CMD_JOIN: &str = "JOIN";
    pub const CMD_NICK: &str = "NICK";
    pub const CMD_PASS: &str = "PASS";
    pub const CMD_PING: &str = "PING";
    pub const CMD_PONG: &str = "PONG";
    pub const CMD_PRIVMSG: &str = "PRIVMSG";
    pub const CMD_QUIT: &str = "QUIT";
    pub const CMD_ROOMSTATE: &str = "ROOMSTATE";

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
        pub tags: Option<BTreeMap<String, String>>,
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
                prefix: Some(SERVER_NAME.to_string()),
                command: command.into(),
                params: params.into_iter().map(|s| s.into()).collect(),
            }
        }

        pub fn add_tag(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
            let tags = self.tags.get_or_insert_with(BTreeMap::new);
            tags.insert(key.into(), value.into());
            self
        }

        pub fn from_custom_message(custom_msg: &CustomMessage, room_id_guess: &str) -> Self {
            let message_id = Uuid::new_v4().to_string();
            let timestamp = Utc::now().timestamp_millis().to_string();
            let login_name = custom_msg.username.to_lowercase(); // login name should be lowercase

            let mut msg = IrcMessage {
                tags: Some(BTreeMap::new()),
                prefix: Some(format!(
                    "{}!{}@{}.{}",
                    login_name,
                    login_name,
                    login_name,
                    SERVER_NAME // Prefix uses login_name
                )),
                command: CMD_PRIVMSG.to_string(),
                params: vec![
                    format!("#{}", custom_msg.channel),
                    custom_msg.message.clone(),
                ],
            };

            let tags = msg.tags.as_mut().unwrap();
            // ***** THIS IS THE KEY FIX *****
            tags.insert(
                TAG_DISPLAY_NAME.to_string(),
                custom_msg.display_name.clone(),
            ); // Use the display_name from CustomMessage
            // ***** END OF KEY FIX *****
            tags.insert(TAG_COLOR.to_string(), custom_msg.color.clone());
            tags.insert(TAG_ID.to_string(), message_id);
            tags.insert(TAG_TMI_SENT_TS.to_string(), timestamp);
            // Use a consistent, derived user ID for spoofed users if not provided,
            // or allow CustomMessage to carry a user_id. For now, derived.
            tags.insert(TAG_USER_ID.to_string(), format!("spoofed-{}", login_name));
            tags.insert(TAG_ROOM_ID.to_string(), room_id_guess.to_string());
            tags.insert(TAG_BADGES.to_string(), "".to_string()); // Default no badges
            tags.insert(TAG_EMOTES.to_string(), "".to_string());
            tags.insert(TAG_MOD.to_string(), "0".to_string());
            tags.insert(TAG_SUBSCRIBER.to_string(), "0".to_string()); // Default no sub
            tags.insert(TAG_TURBO.to_string(), "0".to_string());
            tags.insert(TAG_USER_TYPE.to_string(), "".to_string()); // Default normal user
            tags.insert(TAG_FIRST_MSG.to_string(), "0".to_string());
            tags.insert(TAG_FLAGS.to_string(), "".to_string());
            tags.insert(TAG_RETURNING_CHATTER.to_string(), "0".to_string());
            msg
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ClientHandlerState {
    Connected,
    CapabilitiesSent,
    Authenticated,
    Joined,
}

#[derive(Clone)]
struct ClientSession {
    nick: Option<String>,
    channel: Option<String>,
    state: ClientHandlerState,
    addr: SocketAddr,
    message_sender_to_client: mpsc::Sender<irc::IrcMessage>,
}

impl ClientSession {
    fn new(addr: SocketAddr, message_sender_to_client: mpsc::Sender<irc::IrcMessage>) -> Self {
        ClientSession {
            nick: None,
            channel: None,
            state: ClientHandlerState::Connected,
            addr,
            message_sender_to_client,
        }
    }
}

type SharedClients = Arc<RwLock<HashMap<SocketAddr, ClientSession>>>;

async fn handle_client(
    stream: TcpStream,
    addr: SocketAddr,
    log_tx: mpsc::Sender<ServerLog>, // This is already a clone specific to this handler
    clients: SharedClients,
    mut broadcast_rx_for_this_client: broadcast::Receiver<irc::IrcMessage>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let _ = log_tx.send(ServerLog::ClientConnected(addr)).await;

    let (reader, mut writer_half) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut line_buffer = String::new();

    let (to_this_client_direct_tx, mut to_this_client_direct_rx) =
        mpsc::channel::<irc::IrcMessage>(32);

    {
        let mut clients_guard = clients.write().await;
        clients_guard.insert(
            addr,
            ClientSession::new(addr, to_this_client_direct_tx.clone()),
        );
    }

    let log_tx_writer = log_tx.clone(); // Clone for the writer task
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Some(irc_msg) = to_this_client_direct_rx.recv() => {
                    let raw_msg_str = irc_msg.to_string();
                    if writer_half.write_all(raw_msg_str.as_bytes()).await.is_err() || writer_half.flush().await.is_err() {
                        let _ = log_tx_writer.send(ServerLog::Internal(format!("[{}] Writer: Error writing/flushing direct.", addr))).await;
                        break;
                    }
                    let _ = log_tx_writer.send(ServerLog::Outgoing(addr, raw_msg_str.trim_end().to_string())).await;
                }
                Ok(broadcast_msg) = broadcast_rx_for_this_client.recv() => {
                    let raw_msg_str = broadcast_msg.to_string();
                     if writer_half.write_all(raw_msg_str.as_bytes()).await.is_err() || writer_half.flush().await.is_err() {
                        let _ = log_tx_writer.send(ServerLog::Internal(format!("[{}] Writer: Error writing broadcast.", addr))).await;
                        break; // Exit the writer task when client is disconnected
                    } else {
                        let _ = log_tx_writer.send(ServerLog::Outgoing(addr, raw_msg_str.trim_end().to_string())).await;
                    }
                }
                else => { break; }
            }
        }
    });

    loop {
        line_buffer.clear();
        tokio::select! {
            result = buf_reader.read_line(&mut line_buffer) => {
                match result {
                    Ok(0) => break,
                    Ok(_) => {
                        let incoming_msg_raw = line_buffer.trim_end_matches(['\r', '\n']).to_string();
                        if incoming_msg_raw.is_empty() { continue; }
                        let _ = log_tx.send(ServerLog::Incoming(addr, incoming_msg_raw.clone())).await;

                        let parts: Vec<&str> = incoming_msg_raw.split_whitespace().collect();
                        if parts.is_empty() { continue; }
                        let command_str = parts[0].to_uppercase();

                        let mut clients_guard = clients.write().await;
                        let session = match clients_guard.get_mut(&addr) {
                            Some(s) => s,
                            None => {
                                let _ = log_tx.send(ServerLog::Internal(format!("[{}] Error: Client session not found.", addr))).await;
                                break;
                            }
                        };
                        let msg_sender = session.message_sender_to_client.clone();

                        match command_str.as_str() {
                            irc::CMD_CAP => {
                                if parts.len() >= 3 && parts[1].to_uppercase() == "REQ" {
                                    let caps = parts[2..].join(" ").trim_start_matches(':').to_string();
                                    let msg = irc::IrcMessage::new(irc::CMD_CAP, vec!["*", "ACK", &format!(":{}", caps)]);
                                    if msg_sender.send(msg).await.is_err() {break;}
                                    session.state = ClientHandlerState::CapabilitiesSent;
                                }
                            }
                            irc::CMD_PASS => {}
                            irc::CMD_NICK => {
                                if parts.len() > 1 && session.state == ClientHandlerState::CapabilitiesSent {
                                    let nick = parts[1].to_string();
                                    session.nick = Some(nick.clone());
                                    let msgs = vec![
                                        irc::IrcMessage::new(irc::RPL_WELCOME, vec![&nick, ":Welcome, GLHF!"]),
                                        irc::IrcMessage::new(irc::RPL_YOURHOST, vec![&nick, &format!(":Your host is {}, running version {}", SERVER_NAME, SERVER_VERSION)]),
                                        irc::IrcMessage::new(irc::RPL_CREATED, vec![&nick, ":This server was created on an arbitrary date."]),
                                        irc::IrcMessage::new(irc::RPL_MYINFO, vec![&nick, SERVER_NAME, SERVER_VERSION, "aoits", ""]),
                                        irc::IrcMessage::new(irc::RPL_MOTDSTART, vec![&nick, &format!(":- {} Message of the Day -", SERVER_NAME)]),
                                        irc::IrcMessage::new(irc::RPL_MOTD, vec![&nick, ":This is a spoof TUI IRC server."]),
                                        irc::IrcMessage::new(irc::RPL_ENDOFMOTD, vec![&nick, ":>"]),
                                    ];
                                    for msg in msgs { if msg_sender.send(msg).await.is_err() {break;} }
                                    if msg_sender.is_closed() { break; }
                                    session.state = ClientHandlerState::Authenticated;
                                }
                            }
                            irc::CMD_JOIN => {
                                if session.state == ClientHandlerState::Authenticated && parts.len() > 1 {
                                    let chan_name = parts[1].trim_start_matches('#').to_string();
                                    session.channel = Some(chan_name.clone());
                                    let nick = session.nick.as_ref().cloned().unwrap_or_default();

                                    let join_echo = irc::IrcMessage {
                                        tags: None, prefix: Some(format!("{}!{}@{}.{}", nick, nick, nick, SERVER_NAME)),
                                        command: irc::CMD_JOIN.to_string(), params: vec![format!("#{}", chan_name)],
                                    };
                                    if msg_sender.send(join_echo).await.is_err() {break;}

                                    let room_id = format!("room-for-{}", chan_name);
                                    let roomstate = irc::IrcMessage::new(irc::CMD_ROOMSTATE, vec![format!("#{}", chan_name)])
                                        .add_tag(irc::TAG_ROOM_ID, &room_id);
                                    if msg_sender.send(roomstate).await.is_err() {break;}

                                    let names_prefix = format!("{}.{}", nick, SERVER_NAME);
                                    let users = vec![nick.as_str(), "SpoofedUser1"];
                                    for user in users {
                                        let namreply = irc::IrcMessage {
                                            tags: None, prefix: Some(names_prefix.clone()), command: irc::RPL_NAMREPLY.to_string(),
                                            params: vec![nick.clone(), "=".to_string(), format!("#{}", chan_name), format!(":{}", user)],
                                        };
                                        if msg_sender.send(namreply).await.is_err() {break;}
                                    }
                                    if msg_sender.is_closed() { break; }

                                    let endofnames = irc::IrcMessage {
                                        tags: None, prefix: Some(names_prefix), command: irc::RPL_ENDOFNAMES.to_string(),
                                        params: vec![nick, format!("#{}", chan_name), ":End of /NAMES list".to_string()],
                                    };
                                    if msg_sender.send(endofnames).await.is_err() {break;}
                                    session.state = ClientHandlerState::Joined;
                                }
                            }
                            irc::CMD_PING => {
                                let payload = if parts.len() > 1 { parts[1] } else { "" };
                                let pong = irc::IrcMessage::new(irc::CMD_PONG, vec![SERVER_NAME, payload]);
                                if msg_sender.send(pong).await.is_err() {break;}
                            }
                            irc::CMD_QUIT => break,
                            _ => {}
                        }
                    }
                    Err(e) => {
                        let _ = log_tx.send(ServerLog::Internal(format!("[{}] Error reading from client: {}", addr, e))).await;
                        break;
                    }
                }
            },
        }
    }

    {
        clients.write().await.remove(&addr);
    }
    let _ = log_tx.send(ServerLog::ClientDisconnected(addr)).await;
    Ok(())
}

pub async fn run_server(
    log_tx_main: mpsc::Sender<ServerLog>, // Renamed for clarity, this is the original log_tx
    mut custom_msg_rx_from_ui: mpsc::Receiver<CustomMessage>,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let listener_addr = format!("{}:{}", SPOOF_HOST, SPOOF_PORT);
    let listener = TcpListener::bind(&listener_addr).await?;
    let _ = log_tx_main
        .send(ServerLog::Internal(format!(
            "IRC Server listening on {}",
            listener_addr
        )))
        .await;

    let clients: SharedClients = Arc::new(RwLock::new(HashMap::new()));
    let (broadcast_tx, _) = broadcast::channel::<irc::IrcMessage>(100);

    let log_tx_for_broadcast_listener = log_tx_main.clone(); // Clone for this task
    let broadcast_tx_clone_for_listener = broadcast_tx.clone(); // Clone sender for this task
    tokio::spawn(async move {
        while let Some(custom_msg) = custom_msg_rx_from_ui.recv().await {
            let _ = log_tx_for_broadcast_listener
                .send(ServerLog::Internal(format!(
                    "Server got custom msg for #{}: {}",
                    custom_msg.channel, custom_msg.message
                )))
                .await;
            let room_id = format!("room-for-{}", custom_msg.channel);
            let irc_msg = irc::IrcMessage::from_custom_message(&custom_msg, &room_id);

            // Send to the broadcast channel.
            if broadcast_tx_clone_for_listener
                .send(irc_msg.clone())
                .is_err()
            {
                let _ = log_tx_for_broadcast_listener
                    .send(ServerLog::Internal(
                        "Broadcast failed: No active subscribers.".to_string(),
                    ))
                    .await;
            }
        }
    });

    loop {
        let (stream, addr) = listener.accept().await?;
        let log_tx_for_client = log_tx_main.clone(); // Clone for each new client
        let clients_clone_for_client = Arc::clone(&clients);
        let broadcast_rx_for_client = broadcast_tx.subscribe();

        tokio::spawn(async move {
            // Use the log_tx_for_client for this specific handler's error reporting
            if let Err(e) = handle_client(
                stream,
                addr,
                log_tx_for_client.clone(),
                clients_clone_for_client,
                broadcast_rx_for_client,
            )
            .await
            {
                let _ = log_tx_for_client
                    .send(ServerLog::Internal(format!(
                        "[{}] Client handler error: {}",
                        addr, e
                    )))
                    .await;
            }
        });
    }
}
