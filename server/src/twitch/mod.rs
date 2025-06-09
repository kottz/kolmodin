pub mod auth;
pub mod channel;
pub mod connection;
pub mod error;
pub mod irc_parser;
pub mod manager;
pub mod types;

// Re-export the main types that external modules need
pub use auth::TokenProvider;
pub use error::TwitchError;
pub use manager::TwitchChatManagerActorHandle;
pub use types::{ParsedTwitchMessage, TwitchChannelConnectionStatus};
