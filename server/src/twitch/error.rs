use thiserror::Error;

#[derive(Error, Debug)]
pub enum TwitchError {
    #[error("HTTP request failed: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Twitch IRC authentication/API error: {0}")]
    TwitchAuth(String),
    #[error("Actor communication error: {0}")]
    ActorComm(String),
    #[error("Channel actor internal error: {0}")]
    InternalActorError(String),
    #[error("Twitch IRC connection error: {0}")]
    TwitchConnection(String),
}

pub type Result<T, E = TwitchError> = std::result::Result<T, E>;
