// src/config.rs

use crate::error::{ConfigError, Result as AppResult};
use crate::game_logic::GameType;
use config::{Config, Environment, Value, ValueKind};
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

#[derive(Debug, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum WordListSourceType {
    File,
    Http,
    None, // Explicitly no source, game will use empty/default list
}

#[derive(Debug, Clone, Deserialize)]
pub struct MedAndraOrdWordsConfig {
    pub source_type: WordListSourceType,
    pub file_path: Option<String>,
    pub http_url: Option<String>,
}

impl Default for MedAndraOrdWordsConfig {
    fn default() -> Self {
        Self {
            source_type: WordListSourceType::File,    // Default to file
            file_path: Some("words.txt".to_string()), // Default file name
            http_url: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub med_andra_ord_words: MedAndraOrdWordsConfig,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            med_andra_ord_words: MedAndraOrdWordsConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AppSettings {
    pub server: ServerConfig,
    pub twitch: TwitchConfig,
    pub games: GamesConfig,
    #[serde(default)]
    pub database: DatabaseConfig,
}

pub fn load_settings() -> AppResult<AppSettings> {
    // Set default for games.enabled_types
    let default_games_enabled_types: Vec<Value> = GameType::all()
        .iter()
        .map(|game_type| Value::new(None, ValueKind::String(game_type.primary_id().to_string())))
        .collect();

    // Set default for database.med_andra_ord_words
    let default_db_mao_source_type = Value::new(None, ValueKind::String("file".to_string()));
    let default_db_mao_file_path = Value::new(None, ValueKind::String("words.txt".to_string()));

    let settings = Config::builder()
        .add_source(
            Environment::with_prefix("KOLMODIN")
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("server.cors_origins")
                .with_list_parse_key("games.enabled_types")
                .try_parsing(true),
        )
        // Defaults for top-level required fields if not in Env
        .set_default("server.port", 8080)?
        .set_default("server.cors_origins", Vec::<String>::new())?
        .set_default("twitch.client_id", "")? // Placeholder, must be set in env
        .set_default("twitch.client_secret", "")? // Placeholder, must be set in env
        // Defaults for games
        .set_default(
            "games.enabled_types",
            Value::new(None, ValueKind::Array(default_games_enabled_types)),
        )?
        // Defaults for database
        .set_default(
            "database.med_andra_ord_words.source_type",
            default_db_mao_source_type,
        )?
        .set_default(
            "database.med_andra_ord_words.file_path",
            default_db_mao_file_path,
        )?
        .set_default(
            "database.med_andra_ord_words.http_url",
            Value::new(None, ValueKind::Nil),
        )? // None
        .build()
        .map_err(|e| ConfigError::Load(e.to_string()))?;

    let app_settings: AppSettings = settings
        .try_deserialize()
        .map_err(|e| ConfigError::Load(e.to_string()))?;

    // Validate specific settings
    if app_settings.twitch.client_id.is_empty() {
        return Err(ConfigError::Missing("twitch.client_id".to_string()).into());
    }
    if app_settings.twitch.client_secret.is_empty() {
        return Err(ConfigError::Missing("twitch.client_secret".to_string()).into());
    }

    match app_settings.database.med_andra_ord_words.source_type {
        WordListSourceType::File => {
            if app_settings
                .database
                .med_andra_ord_words
                .file_path
                .is_none()
            {
                return Err(ConfigError::Missing(
                    "database.med_andra_ord_words.file_path (for source_type 'file')".to_string(),
                )
                .into());
            }
        }
        WordListSourceType::Http => {
            if app_settings.database.med_andra_ord_words.http_url.is_none() {
                return Err(ConfigError::Missing(
                    "database.med_andra_ord_words.http_url (for source_type 'http')".to_string(),
                )
                .into());
            }
        }
        WordListSourceType::None => { /* No path/url needed */ }
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
