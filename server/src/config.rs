use crate::error::{ConfigError, Result as AppResult};
use crate::game_logic::GameType;
use config::{Config, Environment, Value, ValueKind};
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;

#[derive(Deserialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub cors_origins: Vec<String>,
    pub admin_api_key: String,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("port", &self.port)
            .field("cors_origins", &self.cors_origins)
            .field("admin_api_key", &"***REDACTED***")
            .finish()
    }
}

#[derive(Clone, Deserialize)]
pub struct TwitchConfig {
    pub client_id: String,
    pub client_secret: String,
    #[serde(default = "default_irc_server_url")]
    pub irc_server_url: String,
}

fn default_irc_server_url() -> String {
    "irc.chat.twitch.tv:6667".to_string()
}

impl std::fmt::Debug for TwitchConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwitchConfig")
            .field("client_id", &self.client_id)
            .field("client_secret", &"***REDACTED***")
            .field("irc_server_url", &self.irc_server_url)
            .finish()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct GamesConfig {
    #[serde(deserialize_with = "deserialize_string_or_list_to_set_lowercase")]
    pub enabled_types: HashSet<String>,
}

impl Default for GamesConfig {
    fn default() -> Self {
        let enabled_types = GameType::all()
            .iter()
            .map(|game_type| game_type.primary_id().to_string())
            .collect();
        Self { enabled_types }
    }
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DataSourceType {
    File,
    Http,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DatabaseConfig {
    pub source_type: DataSourceType,
    pub file_path: Option<String>,
    pub http_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub server: ServerConfig,
    pub twitch: TwitchConfig,
    pub games: GamesConfig,
    pub database: DatabaseConfig,
}

#[tracing::instrument]
pub fn load_settings() -> AppResult<AppSettings> {
    let default_games_enabled_types: Vec<Value> = GameType::all()
        .iter()
        .map(|game_type| Value::new(None, ValueKind::String(game_type.primary_id().to_string())))
        .collect();

    let settings_builder = Config::builder()
        .add_source(
            Environment::with_prefix("KOLMODIN")
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("server.cors_origins")
                .with_list_parse_key("games.enabled_types")
                .try_parsing(true),
        )
        .set_default("server.port", 8080)?
        .set_default("server.cors_origins", Vec::<String>::new())?
        .set_default("twitch.client_id", "")?
        .set_default("twitch.client_secret", "")?
        .set_default(
            "games.enabled_types",
            Value::new(None, ValueKind::Array(default_games_enabled_types)),
        )?;

    let settings = settings_builder
        .build()
        .map_err(|e| ConfigError::Load(e.to_string()))?;

    let app_settings: AppSettings = settings.try_deserialize().map_err(|e| {
        if e.to_string().contains("admin_api_key") {
            ConfigError::Missing(
                "server.admin_api_key (must be set via KOLMODIN_SERVER__ADMIN_API_KEY env var)"
                    .to_string(),
            )
        } else {
            ConfigError::Load(e.to_string())
        }
    })?;

    if app_settings.server.admin_api_key.is_empty() {
        return Err(ConfigError::InvalidValue(
            "server.admin_api_key must not be empty".to_string(),
        )
        .into());
    }
    if app_settings.twitch.client_id.is_empty() {
        return Err(ConfigError::Missing("twitch.client_id".to_string()).into());
    }
    if app_settings.twitch.client_secret.is_empty() {
        return Err(ConfigError::Missing("twitch.client_secret".to_string()).into());
    }

    match app_settings.database.source_type {
        DataSourceType::File => {
            if app_settings.database.file_path.is_none() {
                return Err(ConfigError::Missing(
                    "database.file_path (for source_type 'file')".to_string(),
                )
                .into());
            }
        }
        DataSourceType::Http => {
            if app_settings.database.http_url.is_none() {
                return Err(ConfigError::Missing(
                    "database.http_url (for source_type 'http')".to_string(),
                )
                .into());
            }
        }
    }

    Ok(app_settings)
}

fn deserialize_string_or_list_to_set_lowercase<'de, D>(
    deserializer: D,
) -> Result<HashSet<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    let mut set = HashSet::new();

    match value {
        Value::String(s) => {
            let trimmed = s.trim().to_lowercase();
            if trimmed == "all" {
                for game_type in GameType::all() {
                    set.insert(game_type.primary_id().to_string());
                }
            } else {
                for item in s.split(',') {
                    set.insert(item.trim().to_lowercase());
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                if let Value::String(s) = item {
                    set.insert(s.to_lowercase());
                } else {
                    return Err(D::Error::custom("Array must contain only strings"));
                }
            }
        }
        _ => return Err(D::Error::custom("Expected string or array of strings")),
    }

    Ok(set)
}
