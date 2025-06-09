use std::collections::HashMap;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::auth::TokenProvider;
use super::connection::run_irc_connection_loop;
use super::error::{Result as TwitchResult, TwitchError};
use super::irc_parser::IrcMessage;
use super::types::{ChannelTerminationInfo, ParsedTwitchMessage, TwitchChannelConnectionStatus};

#[derive(Debug)]
pub enum TwitchChannelActorMessage {
    AddSubscriber {
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
        respond_to: oneshot::Sender<TwitchResult<()>>,
    },
    RemoveSubscriber {
        lobby_id: Uuid,
        respond_to: oneshot::Sender<bool>,
    },
    InternalIrcLineReceived {
        line: String,
    },
    InternalConnectionStatusChanged {
        new_status: TwitchChannelConnectionStatus,
    },
    Shutdown,
}

pub struct ChannelActorState {
    pub handle: TwitchChannelActorHandle,
}

#[derive(Clone, Debug)]
pub struct TwitchChannelActorHandle {
    pub sender: mpsc::Sender<TwitchChannelActorMessage>,
    pub channel_name: String, // Keep for identification
    status_rx: watch::Receiver<TwitchChannelConnectionStatus>,
}

impl TwitchChannelActorHandle {
    pub fn new(
        channel_name: String,
        token_provider: TokenProvider,
        actor_buffer_size: usize,
    ) -> (Self, JoinHandle<ChannelTerminationInfo>) {
        let (actor_tx, actor_rx) = mpsc::channel(actor_buffer_size);
        let (status_tx, status_rx) = watch::channel(TwitchChannelConnectionStatus::Initializing);

        let actor = TwitchChannelActor::new(
            actor_rx,
            actor_tx.clone(),
            channel_name.clone(),
            token_provider,
            status_tx,
        );

        let join_handle = tokio::spawn(run_twitch_channel_actor(actor));

        (
            Self {
                sender: actor_tx,
                channel_name,
                status_rx,
            },
            join_handle,
        )
    }

    pub async fn add_subscriber(
        &self,
        lobby_id: Uuid,
        subscriber_tx: mpsc::Sender<ParsedTwitchMessage>,
    ) -> TwitchResult<()> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchChannelActorMessage::AddSubscriber {
                lobby_id,
                subscriber_tx,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| TwitchError::ActorComm(format!("Failed to send AddSubscriber: {}", e)))?;

        respond_to_rx
            .await
            .map_err(|e| TwitchError::ActorComm(format!("Actor failed to respond: {}", e)))?
    }

    pub async fn remove_subscriber(&self, lobby_id: Uuid) -> TwitchResult<bool> {
        let (respond_to_tx, respond_to_rx) = oneshot::channel();
        self.sender
            .send(TwitchChannelActorMessage::RemoveSubscriber {
                lobby_id,
                respond_to: respond_to_tx,
            })
            .await
            .map_err(|e| {
                TwitchError::ActorComm(format!("Failed to send RemoveSubscriber: {}", e))
            })?;

        respond_to_rx
            .await
            .map_err(|e| TwitchError::ActorComm(format!("Actor failed to respond: {}", e)))
    }

    pub fn get_status_receiver(&self) -> watch::Receiver<TwitchChannelConnectionStatus> {
        self.status_rx.clone()
    }
}

pub struct TwitchChannelActor {
    receiver: mpsc::Receiver<TwitchChannelActorMessage>,
    self_sender: mpsc::Sender<TwitchChannelActorMessage>,
    channel_name: String,
    actor_id: Uuid,
    token_provider: TokenProvider,
    subscribers: HashMap<Uuid, mpsc::Sender<ParsedTwitchMessage>>,
    current_status: TwitchChannelConnectionStatus,
    status_sender: watch::Sender<TwitchChannelConnectionStatus>,
    irc_connection_task_handle: Option<JoinHandle<()>>,
    shutdown_signal_tx: Option<oneshot::Sender<()>>,
}

impl TwitchChannelActor {
    fn new(
        receiver: mpsc::Receiver<TwitchChannelActorMessage>,
        self_sender: mpsc::Sender<TwitchChannelActorMessage>,
        channel_name: String,
        token_provider: TokenProvider,
        status_sender: watch::Sender<TwitchChannelConnectionStatus>,
    ) -> Self {
        Self {
            receiver,
            self_sender,
            channel_name,
            actor_id: Uuid::new_v4(),
            token_provider,
            subscribers: HashMap::new(),
            current_status: TwitchChannelConnectionStatus::Initializing,
            status_sender,
            irc_connection_task_handle: None,
            shutdown_signal_tx: None,
        }
    }

    async fn handle_message(&mut self, msg: TwitchChannelActorMessage) {
        match msg {
            TwitchChannelActorMessage::AddSubscriber {
                lobby_id,
                subscriber_tx,
                respond_to,
            } => {
                tracing::debug!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    lobby.id = %lobby_id,
                    "Adding subscriber for lobby"
                );

                if matches!(
                    self.current_status,
                    TwitchChannelConnectionStatus::Terminated
                ) {
                    tracing::warn!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        "AddSubscriber received, but actor status is Terminated. Responding with error"
                    );
                    let _ = respond_to.send(Err(TwitchError::ActorComm(
                        "Channel actor is terminated".to_string(),
                    )));
                    return;
                }

                self.subscribers.insert(lobby_id, subscriber_tx);

                let task_is_truly_stopped_or_never_started = self
                    .irc_connection_task_handle
                    .as_ref()
                    .is_none_or(|h| h.is_finished());

                if task_is_truly_stopped_or_never_started {
                    tracing::debug!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        "IRC task is finished or was never started. Calling start_irc_connection_task"
                    );
                    self.start_irc_connection_task().await;
                } else {
                    tracing::trace!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        "IRC task is active (status: {:?}, handle exists and not finished). Not starting new task",
                        self.current_status
                    );
                }

                let _ = respond_to.send(Ok(()));
            }
            TwitchChannelActorMessage::RemoveSubscriber {
                lobby_id,
                respond_to,
            } => {
                tracing::debug!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    lobby.id = %lobby_id,
                    "Removing subscriber for lobby"
                );

                self.subscribers.remove(&lobby_id);
                let was_last = self.subscribers.is_empty();

                if was_last {
                    tracing::info!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        "No more subscribers. Signaling IRC task to shutdown"
                    );
                    self.shutdown_irc_connection_task().await;
                }

                let _ = respond_to.send(was_last);
            }
            TwitchChannelActorMessage::InternalIrcLineReceived { line } => {
                self.handle_irc_line(line).await;
            }
            TwitchChannelActorMessage::InternalConnectionStatusChanged { new_status } => {
                self.handle_connection_status_change(new_status).await;
            }
            TwitchChannelActorMessage::Shutdown => {
                tracing::info!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    "Received shutdown signal"
                );
                self.shutdown_irc_connection_task().await;
            }
        }
    }

    async fn handle_irc_line(&mut self, line: String) {
        let irc_msg = IrcMessage::parse(&line);
        if let Some(parsed_twitch_msg) = irc_msg.to_parsed_twitch_message(&self.channel_name) {
            let mut failed_sends = Vec::new();

            for (&lobby_id, subscriber_tx) in &self.subscribers {
                if subscriber_tx.send(parsed_twitch_msg.clone()).await.is_err() {
                    tracing::warn!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        lobby.id = %lobby_id,
                        "Failed to send message to subscriber lobby (channel full or closed). Marking for removal"
                    );
                    failed_sends.push(lobby_id);
                }
            }

            for lobby_id in failed_sends {
                self.subscribers.remove(&lobby_id);
            }

            if self.subscribers.is_empty() {
                tracing::info!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    "All subscribers disconnected after send failures. Signaling IRC task to shutdown"
                );
                self.shutdown_irc_connection_task().await;
            }
        } else {
            tracing::trace!(
                channel.name = %self.channel_name,
                actor.id = %self.actor_id,
                line = %line,
                "Received unhandled/non-chat IRC line"
            );
        }
    }

    async fn handle_connection_status_change(&mut self, new_status: TwitchChannelConnectionStatus) {
        self.current_status = new_status.clone();
        update_channel_status(&self.status_sender, new_status.clone());

        match new_status {
            TwitchChannelConnectionStatus::Disconnected { reason } => {
                tracing::info!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    reason = %reason,
                    "Received Disconnected status"
                );

                let irc_loop_task_has_exited = self
                    .irc_connection_task_handle
                    .as_ref()
                    .is_none_or(|h| h.is_finished());

                if irc_loop_task_has_exited {
                    tracing::info!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        "The IRC connection loop task has exited"
                    );

                    if reason.contains("Critical") || reason.contains("Authentication") {
                        tracing::error!(
                            channel.name = %self.channel_name,
                            actor.id = %self.actor_id,
                            reason = %reason,
                            "Critical IRC error after loop exit. Actor shutting down"
                        );
                        self.initiate_actor_shutdown(true).await;
                        return;
                    } else if self.subscribers.is_empty() {
                        tracing::info!(
                            channel.name = %self.channel_name,
                            actor.id = %self.actor_id,
                            reason = %reason,
                            "IRC loop exited and no subscribers. Actor shutting down"
                        );
                        self.initiate_actor_shutdown(true).await;
                        return;
                    } else {
                        tracing::debug!(
                            channel.name = %self.channel_name,
                            actor.id = %self.actor_id,
                            reason = %reason,
                            "IRC loop exited but actor has subscribers. Attempting to restart IRC task"
                        );
                        self.start_irc_connection_task().await;
                    }
                } else {
                    tracing::trace!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        reason = %reason,
                        "IRC connection attempt failed, but IRC loop is still active and will retry. Actor remains active"
                    );
                }
            }
            _ => {}
        }
    }

    async fn initiate_actor_shutdown(&mut self, from_internal_disconnect: bool) {
        tracing::info!(
            channel.name = %self.channel_name,
            actor.id = %self.actor_id,
            from_internal_disconnect = from_internal_disconnect,
            "Initiating actor shutdown sequence"
        );

        self.current_status = TwitchChannelConnectionStatus::Terminated;
        update_channel_status(&self.status_sender, self.current_status.clone());

        self.shutdown_irc_connection_task().await;
    }

    async fn start_irc_connection_task(&mut self) {
        if let Some(ref handle) = self.irc_connection_task_handle {
            if !handle.is_finished() {
                tracing::warn!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    "Attempted to start IRC task, but an active (non-finished) one is already running"
                );
                return;
            } else {
                tracing::debug!(
                    channel.name = %self.channel_name,
                    actor.id = %self.actor_id,
                    "Existing IRC task handle is for a finished task. Clearing it"
                );
                self.irc_connection_task_handle = None;
            }
        }

        tracing::info!(
            channel.name = %self.channel_name,
            actor.id = %self.actor_id,
            "Starting new IRC connection task"
        );

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_signal_tx = Some(shutdown_tx);

        let actor_sender = self.self_sender.clone();
        let channel_name = self.channel_name.clone();
        let actor_id_for_logging = self.actor_id;
        let token_provider_clone = self.token_provider.clone();

        let irc_task = tokio::spawn(run_irc_connection_loop(
            channel_name,
            actor_id_for_logging,
            actor_sender,
            token_provider_clone,
            shutdown_rx,
        ));

        self.irc_connection_task_handle = Some(irc_task);
    }

    async fn shutdown_irc_connection_task(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_signal_tx.take() {
            tracing::debug!(
                channel.name = %self.channel_name,
                actor.id = %self.actor_id,
                "Sending shutdown signal to IRC task"
            );
            let _ = shutdown_tx.send(());
        }

        if let Some(irc_task_handle) = self.irc_connection_task_handle.take() {
            tracing::debug!(
                channel.name = %self.channel_name,
                actor.id = %self.actor_id,
                "Waiting for IRC task to complete..."
            );
            match irc_task_handle.await {
                Err(join_error) => {
                    tracing::warn!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        error = ?join_error,
                        "IRC task panicked or was cancelled"
                    );
                }
                Ok(()) => {
                    tracing::debug!(
                        channel.name = %self.channel_name,
                        actor.id = %self.actor_id,
                        "IRC task completed"
                    );
                }
            }
        }
    }
}

pub async fn run_twitch_channel_actor(mut actor: TwitchChannelActor) -> ChannelTerminationInfo {
    let channel_name = actor.channel_name.clone();
    let actor_id = actor.actor_id;

    tracing::info!(
        channel.name = %channel_name,
        actor.id = %actor_id,
        "Actor started"
    );

    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg).await;

        if matches!(
            actor.current_status,
            TwitchChannelConnectionStatus::Terminated
        ) {
            break;
        }
    }

    actor.shutdown_irc_connection_task().await;

    tracing::info!(
        channel.name = %channel_name,
        actor.id = %actor_id,
        final_status = ?actor.current_status,
        "Actor stopped cleanly with final status"
    );

    ChannelTerminationInfo {
        channel_name,
        actor_id,
        final_status: actor.current_status,
    }
}

fn update_channel_status(
    status_sender: &watch::Sender<TwitchChannelConnectionStatus>,
    new_status: TwitchChannelConnectionStatus,
) {
    if let Err(_) = status_sender.send(new_status.clone()) {
        tracing::warn!(
            status = ?new_status,
            "Failed to update channel status, receiver dropped. This channel actor may be orphaned"
        );
    }
}
