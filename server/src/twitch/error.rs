use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum TwitchError {
    #[error("Environment variable not set: {0}")]
    EnvVarError(String),
    #[error("HTTP request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("JSON deserialization failed: {0}")]
    SerdeJson(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Twitch IRC authentication/API error: {0}")]
    TwitchAuth(String),
    #[error("IRC message parsing error: {0}")]
    ParseError(String),
    #[error("Capability negotiation failed: NAK received for {0:?}")]
    CapabilityNak(Vec<String>),
    #[error("Missing access token in API response")]
    MissingToken,
    #[error("Actor communication error: {0}")]
    ActorComm(String),
    #[error("Twitch Channel Actor for {0} shut down or failed to start")]
    ChannelActorTerminated(String),
    #[error("Failed to send to subscriber for lobby {0}: {1}")]
    SubscriberSendError(Uuid, String),
    #[error("Channel actor internal error: {0}")]
    InternalActorError(String),
    #[error("IRC Task failed to send to actor: {0}")]
    IrcTaskSendError(String),
}

pub type Result<T, E = TwitchError> = std::result::Result<T, E>;
