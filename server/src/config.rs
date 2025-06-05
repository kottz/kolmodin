use crate::error::{ConfigError, Result as AppResult};
use crate::game_logic::GameType;
use config::{Config, Environment, File, Value, ValueKind};
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;

#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    pub port: u16,
    pub cors_origins: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct TwitchConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub server: ServerConfig,
    pub twitch: TwitchConfig,
    pub games: GamesConfig,
}

pub fn load_settings() -> AppResult<AppSettings> {
    let mut builder = Config::builder()
        .add_source(
            Environment::with_prefix("KOLMODIN")
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("admin_password")
                .with_list_parse_key("server.cors_origins")
                .try_parsing(true),
        )
        .add_source(File::with_name("config").required(false));

    // Set default for games.enabled_types
    let default_games: Vec<Value> = GameType::all()
        .iter()
        .map(|game_type| Value::new(None, ValueKind::String(game_type.primary_id().to_string())))
        .collect();

    builder = builder
        .set_default(
            "games.enabled_types",
            Value::new(None, ValueKind::Array(default_games)),
        )
        .map_err(|e| ConfigError::Load(e.to_string()))?;

    let settings = builder
        .build()
        .map_err(|e| ConfigError::Load(e.to_string()))?;

    settings
        .try_deserialize()
        .map_err(|e| ConfigError::Load(e.to_string()).into())
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
                // Enable all available games
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
