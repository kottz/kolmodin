use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::auth::TokenProvider;
use super::channel::TwitchChannelActorMessage;
use super::error::{Result as TwitchResult, TwitchError};
use super::irc_parser::{IrcMessage, *};
use super::types::TwitchChannelConnectionStatus;

pub async fn run_irc_connection_loop(
    channel_name: String,
    actor_id_for_logging: Uuid,
    actor_tx: mpsc::Sender<TwitchChannelActorMessage>,
    token_provider: TokenProvider,
    mut shutdown_rx: oneshot::Receiver<()>,
) {
    tracing::info!(
        channel.name = %channel_name,
        actor.id = %actor_id_for_logging,
        "IRC connection management task started"
    );
    let mut reconnect_attempts = 0u32;
    let mut consecutive_auth_failures = 0u32;
    const MAX_CONSECUTIVE_AUTH_FAILURES: u32 = 3;

    // Exponential backoff configuration
    const BASE_BACKOFF_SECONDS: u32 = 2;
    const MAX_BACKOFF_SECONDS: u32 = 300; // 5 minutes max

    loop {
        reconnect_attempts += 1;
        if actor_tx
            .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                new_status: TwitchChannelConnectionStatus::Connecting {
                    attempt: reconnect_attempts,
                },
            })
            .await
            .is_err()
        {
            tracing::error!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                "Actor channel closed (Connecting). IRC loop shutting down"
            );
            return;
        }

        tokio::select! {
            _ = &mut shutdown_rx => {
                tracing::info!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    "Shutdown signal received. Terminating connection attempt"
                );
                return;
            }
            connection_result = connect_and_listen_irc_single_attempt_adapted(
                channel_name.clone(),
                actor_id_for_logging,
                &token_provider,
                &actor_tx,
                reconnect_attempts,
            ) => {
                let (reason_for_disconnect, delay_seconds, should_terminate_loop) =
                    process_connection_result(
                        connection_result,
                        &mut consecutive_auth_failures,
                        MAX_CONSECUTIVE_AUTH_FAILURES,
                        &token_provider,
                        &channel_name,
                        actor_id_for_logging,
                        reconnect_attempts,
                        BASE_BACKOFF_SECONDS,
                        MAX_BACKOFF_SECONDS,
                    ).await;

                if let Err(e) = actor_tx.send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                    new_status: TwitchChannelConnectionStatus::Disconnected {
                        reason: reason_for_disconnect.clone(),
                    },
                }).await {
                    tracing::error!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        error = %e,
                        "Failed to send to actor. IRC loop shutting down"
                    );
                    return;
                }

                if should_terminate_loop {
                    tracing::info!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        "Loop termination condition met. Exiting"
                    );
                    return;
                }

                // Wait before reconnecting
                if delay_seconds > 0 {
                    if actor_tx.send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                        new_status: TwitchChannelConnectionStatus::Reconnecting {
                            reason: reason_for_disconnect,
                            failed_attempt: reconnect_attempts,
                            retry_in: Duration::from_secs(delay_seconds as u64),
                        },
                    }).await.is_err() {
                        tracing::error!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            "Actor channel closed (Reconnecting). IRC loop shutting down"
                        );
                        return;
                    }

                    tokio::select! {
                        _ = &mut shutdown_rx => {
                            tracing::info!(
                                channel.name = %channel_name,
                                actor.id = %actor_id_for_logging,
                                "Shutdown signal received during reconnect delay. Terminating"
                            );
                            return;
                        }
                        _ = tokio::time::sleep(Duration::from_secs(delay_seconds as u64)) => {
                            tracing::debug!(
                                channel.name = %channel_name,
                                actor.id = %actor_id_for_logging,
                                delay = delay_seconds,
                                "Reconnect delay elapsed"
                            );
                        }
                    }
                }
            }
        }
    }
}

async fn process_connection_result(
    connection_result: TwitchResult<()>,
    consecutive_auth_failures: &mut u32,
    max_consecutive_auth_failures: u32,
    token_provider: &TokenProvider,
    channel_name: &str,
    actor_id_for_logging: Uuid,
    reconnect_attempts: u32,
    base_backoff_seconds: u32,
    max_backoff_seconds: u32,
) -> (String, u32, bool) {
    // Calculate exponential backoff delay
    let calculate_backoff_delay = |attempts: u32| -> u32 {
        let delay = base_backoff_seconds * 2_u32.pow(attempts.saturating_sub(1));
        delay.min(max_backoff_seconds)
    };
    match connection_result {
        Ok(()) => {
            *consecutive_auth_failures = 0;
            let backoff_delay = calculate_backoff_delay(reconnect_attempts);
            (
                "Connection closed/ended gracefully. Will attempt to reconnect.".to_string(),
                backoff_delay,
                false,
            )
        }
        Err(TwitchError::TwitchConnection(conn_msg)) => {
            tracing::warn!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                attempt = *consecutive_auth_failures + 1,
                error = %conn_msg,
                "Connection attempt failed"
            );

            // Check for authentication failures
            if conn_msg.contains(AUTH_ERROR_LOGIN_FAILED)
                || conn_msg.contains(AUTH_ERROR_IMPROPERLY_FORMATTED)
                || conn_msg.contains(AUTH_ERROR_INVALID_NICK)
            {
                *consecutive_auth_failures += 1;

                if *consecutive_auth_failures < max_consecutive_auth_failures {
                    tracing::warn!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        consecutive_failures = *consecutive_auth_failures,
                        "Authentication failure detected. Signaling TokenProvider for immediate refresh"
                    );
                    token_provider.signal_immediate_refresh();

                    tracing::debug!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        "Pausing briefly for potential token refresh before retrying connection"
                    );
                    tokio::time::sleep(Duration::from_secs(3)).await;

                    let backoff_delay = calculate_backoff_delay(reconnect_attempts);
                    (conn_msg, backoff_delay, false)
                } else {
                    tracing::error!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        max_failures = max_consecutive_auth_failures,
                        "Reached max consecutive authentication failures. Terminating IRC loop for this channel"
                    );
                    (conn_msg, 0, true)
                }
            } else {
                *consecutive_auth_failures = 0;
                let backoff_delay = calculate_backoff_delay(reconnect_attempts);
                (conn_msg, backoff_delay, false)
            }
        }
        Err(TwitchError::TwitchAuth(other_auth_msg)) => {
            tracing::error!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                error = %other_auth_msg,
                "Critical authentication problem. Terminating IRC loop"
            );
            (
                format!("Critical authentication problem: {}", other_auth_msg),
                0,
                true,
            )
        }
        Err(TwitchError::Io(io_error)) => {
            tracing::error!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                error = %io_error,
                "I/O error in IRC connection"
            );
            let backoff_delay = calculate_backoff_delay(reconnect_attempts);
            (format!("I/O error: {}", io_error), backoff_delay, false)
        }
        Err(other_error) => {
            tracing::error!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                error = ?other_error,
                "Unexpected error in IRC connection"
            );
            let backoff_delay = calculate_backoff_delay(reconnect_attempts);
            (
                format!("Unexpected error: {:?}", other_error),
                backoff_delay,
                false,
            )
        }
    }
}

async fn connect_and_listen_irc_single_attempt_adapted(
    channel_name: String,
    actor_id_for_logging: Uuid,
    token_provider: &TokenProvider,
    actor_tx: &mpsc::Sender<TwitchChannelActorMessage>,
    attempt_number: u32,
) -> TwitchResult<()> {
    let oauth_token_str = token_provider.get_token().await;

    tracing::info!(
        channel.name = %channel_name,
        actor.id = %actor_id_for_logging,
        attempt = attempt_number,
        "Connecting to Twitch IRC as kolmodin_bot..."
    );

    let connect_timeout = Duration::from_secs(15);
    let stream_result = tokio::time::timeout(
        connect_timeout,
        TcpStream::connect("irc.chat.twitch.tv:6667"),
    )
    .await;

    let stream = match stream_result {
        Ok(Ok(stream)) => stream,
        Ok(Err(tcp_error)) => {
            tracing::error!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                error = %tcp_error,
                "TCP connection failed"
            );
            return Err(TwitchError::Io(tcp_error));
        }
        Err(_) => {
            tracing::error!(
                channel.name = %channel_name,
                actor.id = %actor_id_for_logging,
                timeout = ?connect_timeout,
                "TCP connection timed out"
            );
            return Err(TwitchError::TwitchConnection(format!(
                "TCP connection timed out after {:?}.",
                connect_timeout
            )));
        }
    };

    tracing::info!(
        channel.name = %channel_name,
        actor.id = %actor_id_for_logging,
        "TCP connected. Requesting capabilities and authenticating..."
    );

    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);

    // Standard IRC connection sequence (like the original)
    writer
        .write_all(format!("{}\r\n", TWITCH_CAPABILITIES).as_bytes())
        .await
        .map_err(TwitchError::Io)?;
    writer
        .write_all(format!("{} oauth:{}\r\n", CMD_PASS, oauth_token_str).as_bytes())
        .await
        .map_err(TwitchError::Io)?;

    // Use anonymous justinfan connection like the original
    let bot_nickname = format!("justinfan{}", rand::random::<u32>() % 80000 + 1000);
    writer
        .write_all(format!("{} {}\r\n", CMD_NICK, bot_nickname).as_bytes())
        .await
        .map_err(TwitchError::Io)?;
    writer.flush().await.map_err(TwitchError::Io)?;

    // Connection state tracking like the original
    let mut line_buffer = String::new();
    let mut last_server_activity = tokio::time::Instant::now();
    let mut last_health_check_ping_sent = tokio::time::Instant::now();
    let mut pending_health_check_pong = false;
    let mut authenticated_and_joined = false;

    // Health check configuration from the original
    let health_check_interval = Duration::from_secs(60);
    let health_check_pong_timeout = Duration::from_secs(15);
    let server_activity_timeout = Duration::from_secs(4 * 60);
    let read_timeout = Duration::from_secs(5);

    // Rate detection (for health checks) from the original
    let mut message_timestamps: Vec<tokio::time::Instant> = Vec::with_capacity(200);
    let rate_window = Duration::from_secs(30);
    let min_messages_for_rate_detection = 10;
    let rate_drop_threshold = 0.3;
    let mut last_rate_check_time = tokio::time::Instant::now();
    let rate_check_interval = Duration::from_secs(10);
    let min_time_between_rate_pings = Duration::from_secs(15);
    let mut last_ping_triggered_by_rate_drop =
        tokio::time::Instant::now() - min_time_between_rate_pings;

    if actor_tx
        .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
            new_status: TwitchChannelConnectionStatus::Authenticating {
                attempt: attempt_number,
            },
        })
        .await
        .is_err()
    {
        return Err(TwitchError::TwitchConnection(
            "Actor channel closed during authentication".to_string(),
        ));
    }

    loop {
        line_buffer.clear();

        // Health checks and rate monitoring from the original
        if authenticated_and_joined {
            let now = tokio::time::Instant::now();

            // 1. Server Activity Timeout
            if now.duration_since(last_server_activity) >= server_activity_timeout {
                tracing::warn!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    timeout = ?server_activity_timeout,
                    "No server activity - connection appears dead"
                );
                return Err(TwitchError::TwitchConnection(
                    "No server activity - connection dead".to_string(),
                ));
            }

            // 2. PING/PONG Health Check
            if pending_health_check_pong
                && now.duration_since(last_health_check_ping_sent) >= health_check_pong_timeout
            {
                tracing::warn!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    timeout = ?health_check_pong_timeout,
                    "Health check PING timeout - no PONG received"
                );
                return Err(TwitchError::TwitchConnection(
                    "Health check PONG timeout".to_string(),
                ));
            }

            // Decide if we need to send a PING
            let should_send_ping_interval = !pending_health_check_pong
                && now.duration_since(last_health_check_ping_sent) >= health_check_interval;
            let mut should_send_ping_rate_drop = false;

            // 3. Message Rate Drop Detection (only if not already pending a PONG)
            if !pending_health_check_pong
                && now.duration_since(last_rate_check_time) >= rate_check_interval
            {
                last_rate_check_time = now;
                message_timestamps
                    .retain(|&timestamp| now.duration_since(timestamp) <= rate_window);
                let current_msg_count_in_window = message_timestamps.len();

                if current_msg_count_in_window >= min_messages_for_rate_detection {
                    let current_rate =
                        current_msg_count_in_window as f64 / rate_window.as_secs_f64();
                    let recent_lookback_duration = Duration::from_secs(10);
                    let recent_cutoff = now - recent_lookback_duration;
                    let recent_msg_count = message_timestamps
                        .iter()
                        .filter(|&&ts| ts >= recent_cutoff)
                        .count();
                    let recent_rate =
                        recent_msg_count as f64 / recent_lookback_duration.as_secs_f64();

                    if recent_rate < (current_rate * (1.0 - rate_drop_threshold))
                        && now.duration_since(last_ping_triggered_by_rate_drop)
                            >= min_time_between_rate_pings
                    {
                        tracing::info!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            current_rate = %current_rate,
                            recent_rate = %recent_rate,
                            "Message rate drop detected - triggering health PING"
                        );
                        should_send_ping_rate_drop = true;
                        last_ping_triggered_by_rate_drop = now;
                    }
                }
            }

            if should_send_ping_interval || should_send_ping_rate_drop {
                let reason = if should_send_ping_rate_drop {
                    "rate_drop"
                } else {
                    "interval"
                };
                tracing::debug!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    reason = %reason,
                    "Sending health check PING"
                );
                match writer
                    .write_all(format!("{}\r\n", HEALTH_CHECK_PING).as_bytes())
                    .await
                {
                    Ok(_) => {
                        if let Err(e) = writer.flush().await {
                            tracing::error!(
                                channel.name = %channel_name,
                                actor.id = %actor_id_for_logging,
                                error = %e,
                                "Failed to flush health check PING"
                            );
                            return Err(TwitchError::Io(e));
                        }
                        pending_health_check_pong = true;
                        last_health_check_ping_sent = now;
                    }
                    Err(e) => {
                        tracing::error!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            error = %e,
                            "Failed to send health check PING"
                        );
                        return Err(TwitchError::Io(e));
                    }
                }
            }
        }

        match tokio::time::timeout(read_timeout, buf_reader.read_line(&mut line_buffer)).await {
            Ok(Ok(0)) => {
                tracing::info!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    "Connection closed by Twitch (EOF)"
                );
                return Ok(());
            }
            Ok(Ok(_bytes_read)) => {
                // Line successfully read
            }
            Ok(Err(e)) => {
                tracing::error!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    error = %e,
                    "Error reading from chat"
                );
                return Err(TwitchError::Io(e));
            }
            Err(_timeout_error) => {
                // Read timed out - normal if chat is idle
                if !authenticated_and_joined
                    && tokio::time::Instant::now().duration_since(last_server_activity)
                        > Duration::from_secs(30)
                {
                    tracing::warn!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        "No server activity for 30s during initial connection phase. Assuming failure"
                    );
                    return Err(TwitchError::TwitchConnection(
                        "Initial connection phase timeout".to_string(),
                    ));
                }
                continue;
            }
        }

        let message_line_owned = line_buffer.trim_end_matches(['\r', '\n']).to_string();

        if !message_line_owned.is_empty() {
            last_server_activity = tokio::time::Instant::now();

            // Send raw line to actor for parsing and distribution
            if actor_tx
                .send(TwitchChannelActorMessage::InternalIrcLineReceived {
                    line: message_line_owned.clone(),
                })
                .await
                .is_err()
            {
                return Err(TwitchError::TwitchConnection(
                    "Actor channel closed".to_string(),
                ));
            }
        } else {
            continue;
        }

        // Handle IRC protocol messages using proper IRC parsing
        match IrcMessage::parse(&message_line_owned) {
            Ok(parsed_irc_msg) => match parsed_irc_msg.command() {
                Some(CMD_PING) => {
                    let server = parsed_irc_msg
                        .params()
                        .first()
                        .copied()
                        .unwrap_or(":tmi.twitch.tv");
                    tracing::trace!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        server = %server,
                        "Received server PING, responding with PONG"
                    );
                    writer
                        .write_all(format!("{} {}\r\n", CMD_PONG, server).as_bytes())
                        .await
                        .map_err(TwitchError::Io)?;
                    writer.flush().await.map_err(TwitchError::Io)?;
                }
                Some(CMD_PONG) => {
                    // The PONG parameters are usually <server> [:<text sent in PING>]
                    // We are interested in the text part if it exists.
                    let pong_payload = parsed_irc_msg
                        .params()
                        .get(1)
                        .map(|s| s.trim_start_matches(':'));

                    if pending_health_check_pong && pong_payload == Some("health-check") {
                        let response_time =
                            tokio::time::Instant::now().duration_since(last_health_check_ping_sent);
                        tracing::debug!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            response_time = ?response_time,
                            "Health check PONG received correctly"
                        );
                        pending_health_check_pong = false;
                    } else {
                        tracing::trace!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            payload = ?pong_payload,
                            "Received PONG (not for health check or unexpected payload)"
                        );
                    }
                }
                Some(RPL_WELCOME) => {
                    tracing::info!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        "Authenticated successfully (RPL_WELCOME). Joining channel..."
                    );
                    if actor_tx
                        .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                            new_status: TwitchChannelConnectionStatus::Connected,
                        })
                        .await
                        .is_err()
                    {
                        return Err(TwitchError::TwitchConnection(
                            "Actor channel closed".to_string(),
                        ));
                    }
                    writer
                        .write_all(
                            format!("{} #{}\r\n", CMD_JOIN, channel_name.to_lowercase()).as_bytes(),
                        )
                        .await
                        .map_err(TwitchError::Io)?;
                    writer.flush().await.map_err(TwitchError::Io)?;
                }
                Some(CMD_JOIN) => {
                    let joining_user = parsed_irc_msg
                        .prefix()
                        .and_then(|p| p.split('!').next())
                        .unwrap_or_default();
                    let joined_chan = parsed_irc_msg
                        .params()
                        .first()
                        .map(|s| s.trim_start_matches('#'))
                        .unwrap_or_default();
                    if joined_chan.eq_ignore_ascii_case(&channel_name)
                        && joining_user.eq_ignore_ascii_case(&bot_nickname)
                    {
                        tracing::info!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            nickname = %bot_nickname,
                            "Successfully JOINED #{} as {}",
                            joined_chan,
                            bot_nickname
                        );
                        authenticated_and_joined = true;
                        last_health_check_ping_sent = tokio::time::Instant::now();
                        last_rate_check_time = tokio::time::Instant::now();
                    }
                }
                Some(CMD_NOTICE) => {
                    let notice_text = parsed_irc_msg.params().last().map_or("", |v| v);

                    if notice_text.contains(AUTH_ERROR_LOGIN_FAILED)
                        || notice_text.contains(AUTH_ERROR_IMPROPERLY_FORMATTED)
                        || notice_text.contains(AUTH_ERROR_INVALID_NICK)
                    {
                        tracing::error!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            notice = %notice_text,
                            "Authentication failed via NOTICE"
                        );
                        return Err(TwitchError::TwitchAuth(format!(
                            "Authentication failed via NOTICE: {}",
                            notice_text
                        )));
                    }
                }
                Some(CMD_RECONNECT) => {
                    tracing::info!(
                        channel.name = %channel_name,
                        actor.id = %actor_id_for_logging,
                        "Received RECONNECT command from Twitch. Closing current connection to allow re-loop"
                    );
                    return Ok(());
                }
                Some(CMD_CAP) => {
                    // Check for CAP ACK or NAK in the parameters
                    if parsed_irc_msg.params().len() >= 2 {
                        let cap_command = parsed_irc_msg.params().get(1).map_or("", |v| v);
                        if cap_command == IRC_NAK {
                            let nak_caps = parsed_irc_msg.params().get(2).map_or("unknown", |v| v);
                            tracing::error!(
                                channel.name = %channel_name,
                                actor.id = %actor_id_for_logging,
                                capabilities = %nak_caps,
                                "Capability NAK. This could affect functionality"
                            );
                        } else if cap_command == IRC_ACK {
                            let ack_caps = parsed_irc_msg.params().get(2).map_or("unknown", |v| v);
                            tracing::info!(
                                channel.name = %channel_name,
                                actor.id = %actor_id_for_logging,
                                capabilities = %ack_caps,
                                "Capability ACK"
                            );
                        }
                    }
                }
                Some(CMD_PRIVMSG) => {
                    if authenticated_and_joined {
                        message_timestamps.push(tokio::time::Instant::now());
                        if message_timestamps.len() > 1000 {
                            let cleanup_cutoff = tokio::time::Instant::now()
                                - (rate_window + Duration::from_secs(10));
                            message_timestamps.retain(|&timestamp| timestamp >= cleanup_cutoff);
                        }
                    }
                }
                _ => {
                    // Handle :Welcome text detection for compatibility
                    if message_line_owned.contains(IRC_WELCOME_TEXT) {
                        tracing::info!(
                            channel.name = %channel_name,
                            actor.id = %actor_id_for_logging,
                            "Authenticated successfully (:Welcome). Joining channel..."
                        );
                        if actor_tx
                            .send(TwitchChannelActorMessage::InternalConnectionStatusChanged {
                                new_status: TwitchChannelConnectionStatus::Connected,
                            })
                            .await
                            .is_err()
                        {
                            return Err(TwitchError::TwitchConnection(
                                "Actor channel closed".to_string(),
                            ));
                        }
                        writer
                            .write_all(
                                format!("{} #{}\r\n", CMD_JOIN, channel_name.to_lowercase())
                                    .as_bytes(),
                            )
                            .await
                            .map_err(TwitchError::Io)?;
                        writer.flush().await.map_err(TwitchError::Io)?;
                    }
                }
            },
            Err(e) => {
                tracing::warn!(
                    channel.name = %channel_name,
                    actor.id = %actor_id_for_logging,
                    raw_line = %message_line_owned,
                    error = %e,
                    "Failed to parse IRC message. Skipping line."
                );
                // Log and skip malformed lines rather than tearing down the connection
            }
        }
    }
}
