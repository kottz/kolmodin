pub mod auth;
pub mod error;
pub mod irc_parser;
pub mod service;
pub mod types;

// Re-export the main types that external modules need
pub use auth::TokenProvider;
pub use error::TwitchError;
pub use service::TwitchServiceHandle;
pub use types::{ParsedTwitchMessage, TwitchChannelConnectionStatus};
