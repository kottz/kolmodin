use std::collections::{HashMap, hash_map::Entry};
use std::pin::Pin;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::{Sleep, sleep};
use uuid::Uuid;

use crate::twitch::auth::TokenProvider;
use crate::twitch::error::TwitchError;
use crate::twitch::irc_parser::{
    AUTH_ERROR_IMPROPERLY_FORMATTED, AUTH_ERROR_INVALID_NICK, AUTH_ERROR_LOGIN_FAILED, CMD_CAP,
    CMD_JOIN, CMD_NICK, CMD_NOTICE, CMD_PASS, CMD_PING, CMD_PONG, CMD_PRIVMSG, CMD_RECONNECT,
    IRC_ACK, IRC_NAK, IRC_WELCOME_TEXT, IrcMessage, RPL_WELCOME, TWITCH_CAPABILITIES,
};
use crate::twitch::types::{ParsedTwitchMessage, TwitchChannelConnectionStatus};

const DEFAULT_COMMAND_BUFFER: usize = 64;
const DEFAULT_EVENT_BUFFER: usize = 512;
const RECONNECT_BASE_DELAY: Duration = Duration::from_secs(5);
const RECONNECT_MAX_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug)]
enum TwitchServiceCommand {
    Subscribe {
        channel_name: String,
        lobby_id: Uuid,
        twitch_message_tx_for_lobby: mpsc::Sender<ParsedTwitchMessage>,
        respond_to:
            oneshot::Sender<Result<watch::Receiver<TwitchChannelConnectionStatus>, TwitchError>>,
    },
    Unsubscribe {
        channel_name: String,
        lobby_id: Uuid,
        respond_to: oneshot::Sender<Result<(), TwitchError>>,
    },
}

#[derive(Clone, Debug)]
pub struct TwitchServiceHandle {
    sender: mpsc::Sender<TwitchServiceCommand>,
}

impl TwitchServiceHandle {
    pub fn spawn(
        token_provider: TokenProvider,
        command_buffer_size: usize,
        event_buffer_size: usize,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(command_buffer_size.max(DEFAULT_COMMAND_BUFFER));
        let service = TwitchService::new(
            token_provider,
            receiver,
            event_buffer_size.max(DEFAULT_EVENT_BUFFER),
        );
        tokio::spawn(service.run());
        Self { sender }
    }

    pub async fn subscribe_to_channel(
        &self,
        channel_name: String,
        lobby_id: Uuid,
        twitch_message_tx_for_lobby: mpsc::Sender<ParsedTwitchMessage>,
    ) -> Result<watch::Receiver<TwitchChannelConnectionStatus>, TwitchError> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchServiceCommand::Subscribe {
                channel_name,
                lobby_id,
                twitch_message_tx_for_lobby,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!(
                    "Failed to send Subscribe command to TwitchService: {}",
                    e
                ))
            })?;

        respond_to_rx.await.map_err(|e| {
            TwitchError::ActorComm(format!(
                "TwitchService failed to respond to Subscribe command: {}",
                e
            ))
        })?
    }

    pub async fn unsubscribe_from_channel(
        &self,
        channel_name: String,
        lobby_id: Uuid,
    ) -> Result<(), TwitchError> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchServiceCommand::Unsubscribe {
                channel_name,
                lobby_id,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!(
                    "Failed to send Unsubscribe command to TwitchService: {}",
                    e
                ))
            })?;

        respond_to_rx.await.map_err(|e| {
            TwitchError::ActorComm(format!(
                "TwitchService failed to respond to Unsubscribe command: {}",
                e
            ))
        })?
    }
}

struct ChannelState {
    subscribers: HashMap<Uuid, mpsc::Sender<ParsedTwitchMessage>>,
    status_tx: watch::Sender<TwitchChannelConnectionStatus>,
    joined: bool,
}

impl ChannelState {
    fn new() -> (Self, watch::Receiver<TwitchChannelConnectionStatus>) {
        let (status_tx, status_rx) = watch::channel(TwitchChannelConnectionStatus::Initializing);
        (
            ChannelState {
                subscribers: HashMap::new(),
                status_tx,
                joined: false,
            },
            status_rx,
        )
    }

    fn update_status(&self, new_status: TwitchChannelConnectionStatus) {
        let _ = self.status_tx.send(new_status);
    }
}

struct TwitchService {
    token_provider: TokenProvider,
    commands: mpsc::Receiver<TwitchServiceCommand>,
    channel_states: HashMap<String, ChannelState>,
    irc_connection: Option<IrcConnection>,
    connection_ready: bool,
    reconnect_backoff: Duration,
    reconnect_delay: Option<Pin<Box<Sleep>>>,
    bot_nickname: Option<String>,
    event_buffer_size: usize,
    connection_attempt: u32,
}

impl TwitchService {
    fn new(
        token_provider: TokenProvider,
        commands: mpsc::Receiver<TwitchServiceCommand>,
        event_buffer_size: usize,
    ) -> Self {
        TwitchService {
            token_provider,
            commands,
            channel_states: HashMap::new(),
            irc_connection: None,
            connection_ready: false,
            reconnect_backoff: RECONNECT_BASE_DELAY,
            reconnect_delay: None,
            bot_nickname: None,
            event_buffer_size,
            connection_attempt: 0,
        }
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                biased;
                _ = async {}, if self.should_attempt_connection() => {
                    self.try_establish_connection().await;
                }
                Some(cmd) = self.commands.recv() => {
                    self.handle_command(cmd).await;
                }
                _ = async {
                    if let Some(delay) = self.reconnect_delay.as_mut() {
                        delay.await;
                    }
                }, if self.reconnect_delay.is_some() => {
                    self.reconnect_delay = None;
                    if self.should_attempt_connection() {
                        self.try_establish_connection().await;
                    }
                }
                event = async {
                    match self.irc_connection.as_mut() {
                        Some(conn) => conn.events.recv().await,
                        None => None,
                    }
                }, if self.irc_connection.is_some() => {
                    match event {
                        Some(evt) => self.handle_irc_event(evt).await,
                        None => {
                            self.handle_disconnect("IRC event stream ended unexpectedly".to_string()).await;
                        }
                    }
                }
                else => {
                    if self.channel_states.is_empty() {
                        break;
                    }
                }
            }
        }
        tracing::info!("TwitchService loop stopped (no more subscribers)");
    }

    fn should_attempt_connection(&self) -> bool {
        self.irc_connection.is_none()
            && self.reconnect_delay.is_none()
            && !self.channel_states.is_empty()
    }

    async fn handle_command(&mut self, cmd: TwitchServiceCommand) {
        match cmd {
            TwitchServiceCommand::Subscribe {
                channel_name,
                lobby_id,
                twitch_message_tx_for_lobby,
                respond_to,
            } => {
                let normalized_channel = channel_name.to_lowercase();
                let (status_receiver, result) = self
                    .subscribe_lobby(normalized_channel, lobby_id, twitch_message_tx_for_lobby)
                    .await;
                let _ = respond_to.send(result.map(|_| status_receiver));
            }
            TwitchServiceCommand::Unsubscribe {
                channel_name,
                lobby_id,
                respond_to,
            } => {
                let normalized_channel = channel_name.to_lowercase();
                let result = self.unsubscribe_lobby(&normalized_channel, lobby_id).await;
                let _ = respond_to.send(result);
            }
        }
    }

    async fn subscribe_lobby(
        &mut self,
        channel: String,
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
    ) -> (
        watch::Receiver<TwitchChannelConnectionStatus>,
        Result<(), TwitchError>,
    ) {
        let attempt_number = if self.connection_attempt == 0 {
            1
        } else {
            self.connection_attempt
        };
        let status_rx = match self.channel_states.entry(channel.clone()) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().subscribers.insert(lobby_id, subscriber_tx);
                entry.get().status_tx.subscribe()
            }
            Entry::Vacant(entry) => {
                let (mut state, status_rx) = ChannelState::new();
                state.subscribers.insert(lobby_id, subscriber_tx);
                state.update_status(TwitchChannelConnectionStatus::Connecting {
                    attempt: attempt_number,
                });
                entry.insert(state);
                status_rx
            }
        };

        if self.irc_connection.is_some() && self.connection_ready {
            self.ensure_channel_joined(&channel).await;
        } else if self.irc_connection.is_none() {
            self.schedule_reconnect(Duration::from_secs(0));
        }

        (status_rx, Ok(()))
    }

    async fn unsubscribe_lobby(
        &mut self,
        channel: &str,
        lobby_id: Uuid,
    ) -> Result<(), TwitchError> {
        let mut should_remove = false;
        if let Some(state) = self.channel_states.get_mut(channel) {
            state.subscribers.remove(&lobby_id);
            if state.subscribers.is_empty() {
                state.update_status(TwitchChannelConnectionStatus::Terminated);
                should_remove = true;
            }
        }

        if should_remove {
            self.send_part_if_needed(channel).await;
            self.channel_states.remove(channel);

            if self.channel_states.is_empty() {
                self.teardown_connection().await;
            }
        }
        Ok(())
    }

    fn schedule_reconnect(&mut self, delay: Duration) {
        if delay.is_zero() {
            self.reconnect_delay = Some(Box::pin(sleep(Duration::from_secs(0))));
        } else {
            self.reconnect_delay = Some(Box::pin(sleep(delay)));
        }
    }

    async fn try_establish_connection(&mut self) {
        if self.irc_connection.is_some() || self.channel_states.is_empty() {
            return;
        }

        tracing::info!("TwitchService attempting to establish IRC connection");

        self.connection_attempt = self.connection_attempt.saturating_add(1);
        self.update_all_channel_statuses(TwitchChannelConnectionStatus::Connecting {
            attempt: self.connection_attempt,
        });

        match IrcConnection::connect(self.token_provider.clone(), self.event_buffer_size).await {
            Ok((connection, nickname)) => {
                self.bot_nickname = Some(nickname);
                self.irc_connection = Some(connection);
                self.connection_ready = false;
                self.reconnect_backoff = RECONNECT_BASE_DELAY;
                self.update_all_channel_statuses(TwitchChannelConnectionStatus::Authenticating {
                    attempt: self.connection_attempt,
                });
                tracing::info!("TwitchService connected to IRC server, waiting for welcome");
            }
            Err(err) => {
                tracing::error!(error = %err, "Failed to establish IRC connection");
                self.schedule_reconnect(self.reconnect_backoff);
                self.reconnect_backoff = (self.reconnect_backoff * 2).min(RECONNECT_MAX_DELAY);
            }
        }
    }

    async fn handle_irc_event(&mut self, event: IrcEvent) {
        match event {
            IrcEvent::Line(line) => {
                self.process_irc_line(line).await;
            }
            IrcEvent::Closed(reason) => {
                tracing::warn!(reason = %reason, "IRC connection closed");
                self.handle_disconnect(reason).await;
            }
        }
    }

    async fn process_irc_line(&mut self, line: String) {
        if let Ok(message) = IrcMessage::parse(&line) {
            match message.command() {
                Some(CMD_PING) => {
                    if let Some(payload) = message.params().first() {
                        let response = format!("{} {}", CMD_PONG, payload);
                        if let Some(conn) = self.irc_connection.as_ref() {
                            let _ = conn.send_raw(response).await;
                        }
                    }
                }
                Some(RPL_WELCOME) => {
                    tracing::info!("IRC welcome received. Marking connection ready.");
                    self.connection_ready = true;
                    self.join_all_channels().await;
                    self.update_all_channel_statuses(TwitchChannelConnectionStatus::Connected);
                }
                Some(CMD_NOTICE) => {
                    let notice_text = message.params().last().copied().unwrap_or_default();
                    tracing::warn!(notice = %notice_text, "Received Twitch NOTICE");
                    if notice_text.contains(AUTH_ERROR_LOGIN_FAILED)
                        || notice_text.contains(AUTH_ERROR_IMPROPERLY_FORMATTED)
                        || notice_text.contains(AUTH_ERROR_INVALID_NICK)
                    {
                        self.token_provider.signal_immediate_refresh();
                        self.handle_disconnect("Authentication failure".to_string())
                            .await;
                    }
                }
                Some(CMD_RECONNECT) => {
                    tracing::info!("Received IRC RECONNECT command. Restarting connection.");
                    self.handle_disconnect("Twitch requested reconnect".to_string())
                        .await;
                }
                Some(CMD_CAP) => {
                    if message.params().len() >= 2 {
                        if message.params()[1] == IRC_NAK {
                            tracing::warn!(
                                capabilities = ?message.params().get(2),
                                "IRC capability request was NAKed"
                            );
                        } else if message.params()[1] == IRC_ACK {
                            tracing::debug!(
                                capabilities = ?message.params().get(2),
                                "IRC capability acknowledged"
                            );
                        }
                    }
                }
                Some(CMD_PRIVMSG) => {
                    self.dispatch_privmsg(&message).await;
                }
                _ => {
                    if line.contains(IRC_WELCOME_TEXT) {
                        tracing::info!("IRC welcome text detected. Marking connection ready.");
                        self.connection_ready = true;
                        self.join_all_channels().await;
                        self.update_all_channel_statuses(TwitchChannelConnectionStatus::Connected);
                    }
                }
            }
        }
    }

    async fn dispatch_privmsg(&mut self, message: &IrcMessage<'_>) {
        let Some(target_channel) = message.params().first() else {
            return;
        };
        let normalized_channel = target_channel.trim_start_matches('#').to_lowercase();

        let mut channel_should_close = false;

        if let Some(state) = self.channel_states.get_mut(&normalized_channel)
            && let Some(parsed) = message.to_parsed_twitch_message(&normalized_channel)
        {
            let mut failed = Vec::new();
            for (lobby_id, tx) in &state.subscribers {
                if tx.send(parsed.clone()).await.is_err() {
                    failed.push(*lobby_id);
                }
            }
            for id in failed {
                state.subscribers.remove(&id);
            }
            if state.subscribers.is_empty() {
                channel_should_close = true;
            }
        }

        if channel_should_close {
            self.send_part_if_needed(&normalized_channel).await;
            if let Some(state) = self.channel_states.get(&normalized_channel) {
                state.update_status(TwitchChannelConnectionStatus::Terminated);
            }
            self.channel_states.remove(&normalized_channel);
            if self.channel_states.is_empty() {
                self.teardown_connection().await;
            }
        }
    }

    async fn handle_disconnect(&mut self, reason: String) {
        self.connection_ready = false;
        self.bot_nickname = None;
        self.irc_connection = None;

        if !self.channel_states.is_empty() {
            let retry_delay = self.reconnect_backoff;
            self.update_all_channel_statuses(TwitchChannelConnectionStatus::Disconnected {
                reason: reason.clone(),
            });
            self.update_all_channel_statuses(TwitchChannelConnectionStatus::Reconnecting {
                reason,
                failed_attempt: self.connection_attempt,
                retry_in: retry_delay,
            });
            self.schedule_reconnect(retry_delay);
            self.reconnect_backoff = (self.reconnect_backoff * 2).min(RECONNECT_MAX_DELAY);

            for state in self.channel_states.values_mut() {
                state.joined = false;
            }
        }
    }

    async fn join_all_channels(&mut self) {
        let channels: Vec<String> = self.channel_states.keys().cloned().collect();
        for channel in channels {
            self.ensure_channel_joined(&channel).await;
        }
    }

    async fn ensure_channel_joined(&mut self, channel: &str) {
        if let Some(conn) = self.irc_connection.as_ref()
            && let Some(state) = self.channel_states.get_mut(channel)
        {
            if state.joined || state.subscribers.is_empty() {
                return;
            }

            let join_cmd = format!("{} #{}", CMD_JOIN, channel);
            if conn.send_raw(join_cmd).await.is_ok() {
                state.joined = true;
                state.update_status(TwitchChannelConnectionStatus::Connected);
            }
        }
    }

    async fn send_part_if_needed(&mut self, channel: &str) {
        if let Some(conn) = self.irc_connection.as_ref() {
            let part_cmd = format!("PART #{}", channel);
            let _ = conn.send_raw(part_cmd).await;
        }
    }

    fn update_all_channel_statuses(&self, status: TwitchChannelConnectionStatus) {
        for state in self.channel_states.values() {
            state.update_status(status.clone());
        }
    }

    async fn teardown_connection(&mut self) {
        self.irc_connection = None;
        self.connection_ready = false;
        self.bot_nickname = None;
    }
}

enum IrcEvent {
    Line(String),
    Closed(String),
}

struct IrcConnection {
    write_tx: mpsc::Sender<String>,
    events: mpsc::Receiver<IrcEvent>,
}

impl IrcConnection {
    async fn connect(
        token_provider: TokenProvider,
        event_buffer_size: usize,
    ) -> Result<(Self, String), TwitchError> {
        let irc_server_url = token_provider.get_irc_server_url().to_string();
        let stream = TcpStream::connect(&irc_server_url).await?;
        let (reader, writer) = tokio::io::split(stream);
        let (write_tx, mut write_rx) = mpsc::channel::<String>(128);
        let (event_tx, event_rx) = mpsc::channel::<IrcEvent>(event_buffer_size);

        let writer_event_tx = event_tx.clone();
        tokio::spawn(async move {
            let mut writer = writer;
            while let Some(message) = write_rx.recv().await {
                if let Err(e) = writer.write_all(message.as_bytes()).await {
                    let _ = writer_event_tx
                        .send(IrcEvent::Closed(format!("Write error: {}", e)))
                        .await;
                    break;
                }
                if let Err(e) = writer.flush().await {
                    let _ = writer_event_tx
                        .send(IrcEvent::Closed(format!("Flush error: {}", e)))
                        .await;
                    break;
                }
            }
        });

        let reader_event_tx = event_tx.clone();
        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        let _ = reader_event_tx
                            .send(IrcEvent::Closed("Connection closed by server".to_string()))
                            .await;
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\r', '\n']).to_string();
                        if reader_event_tx.send(IrcEvent::Line(trimmed)).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = reader_event_tx
                            .send(IrcEvent::Closed(format!("Read error: {}", e)))
                            .await;
                        break;
                    }
                }
            }
        });

        let connection = IrcConnection {
            write_tx,
            events: event_rx,
        };

        connection
            .send_raw(format!("{}\r\n", TWITCH_CAPABILITIES))
            .await?;

        let token = token_provider.get_token().await;
        connection
            .send_raw(format!("{} oauth:{}\r\n", CMD_PASS, token))
            .await?;

        let nickname = format!("justinfan{}", rand::random::<u32>() % 80000 + 1000);
        connection
            .send_raw(format!("{} {}\r\n", CMD_NICK, nickname))
            .await?;

        Ok((connection, nickname))
    }

    async fn send_raw<S: Into<String>>(&self, line: S) -> Result<(), TwitchError> {
        let mut message = line.into();
        if !message.ends_with("\r\n") {
            message.push_str("\r\n");
        }
        self.write_tx.send(message).await.map_err(|e| {
            TwitchError::TwitchConnection(format!("Failed to send IRC command: {}", e))
        })
    }
}
