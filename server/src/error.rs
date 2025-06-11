use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Configuration loading error: {0}")]
    Load(String),
    #[error("Invalid configuration value: {0}")]
    InvalidValue(String),
    #[error("Missing required configuration: {0}")]
    Missing(String),
}

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Word source configuration error: {0}")]
    Config(String),
    #[error("Failed to read word file '{path}': {source}")]
    FileRead {
        path: String,
        source: std::io::Error,
    },
    #[error("Failed to fetch words from URL '{url}': {source}")]
    HttpFetch { url: String, source: reqwest::Error },
}

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),
    #[error("Twitch integration error: {0}")]
    Twitch(#[from] crate::twitch::TwitchError),
    #[error("Web server/handler error: {0}")]
    Web(#[from] crate::web::WebError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("HTTP client error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Configuration parsing error: {0}")]
    ConfigParsing(#[from] config::ConfigError),
    #[error("Database error: {0}")]
    Database(#[from] DbError),
}

pub type Result<T, E = AppError> = std::result::Result<T, E>;
