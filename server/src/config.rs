use crate::error::{ConfigError, Result as AppResult};
use crate::game_logic::GameType;
use config::{Config, Environment, Value, ValueKind}; // ValueKind is used
use serde::{Deserialize, Deserializer};
use std::collections::HashSet;

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub port: u16,
    pub cors_origins: Vec<String>,
    pub admin_api_key: String, // Changed: Now required, not Option<String>
}

#[derive(Debug, Deserialize)]
pub struct TwitchConfig {
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Deserialize, Clone)] // GamesConfig was already Clone
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
    None,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DataFileConfig {
    pub file_path: String,
}

impl Default for DataFileConfig {
    fn default() -> Self {
        Self {
            file_path: "kolmodin_data.txt".to_string(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct MedAndraOrdWordsConfig {
    pub source_type: WordListSourceType,
    pub file_path: Option<String>,
    pub http_url: Option<String>,
}

impl Default for MedAndraOrdWordsConfig {
    fn default() -> Self {
        Self {
            source_type: WordListSourceType::File,
            file_path: Some("words.txt".to_string()),
            http_url: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct DatabaseConfig {
    #[serde(default)]
    pub data_file: DataFileConfig,
    #[serde(default)]
    pub med_andra_ord_words: MedAndraOrdWordsConfig,
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
    let default_games_enabled_types: Vec<Value> = GameType::all()
        .iter()
        .map(|game_type| Value::new(None, ValueKind::String(game_type.primary_id().to_string())))
        .collect();

    let default_data_file_path =
        Value::new(None, ValueKind::String("kolmodin_data.txt".to_string()));
    let default_db_mao_source_type = Value::new(None, ValueKind::String("file".to_string()));
    let default_db_mao_file_path = Value::new(None, ValueKind::String("words.txt".to_string()));

    let settings_builder = Config::builder()
        .add_source(
            Environment::with_prefix("KOLMODIN")
                .separator("__")
                .list_separator(",")
                .with_list_parse_key("server.cors_origins")
                .with_list_parse_key("games.enabled_types")
                .try_parsing(true),
        )
        // Set defaults for fields that can have them.
        // `server.admin_api_key` is now required, so no default here.
        // If not provided via ENV, deserialization will fail.
        .set_default("server.port", 8080)?
        .set_default("server.cors_origins", Vec::<String>::new())?
        // `twitch.client_id` and `secret` are also effectively required,
        // as they are checked later.
        .set_default("twitch.client_id", "")?
        .set_default("twitch.client_secret", "")?
        .set_default(
            "games.enabled_types",
            Value::new(None, ValueKind::Array(default_games_enabled_types)),
        )?
        .set_default("database.data_file.file_path", default_data_file_path)?
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
        )?;

    let settings = settings_builder
        .build()
        .map_err(|e| ConfigError::Load(e.to_string()))?;

    // Attempt to deserialize. This will fail if required fields (like admin_api_key) are missing.
    let app_settings: AppSettings = settings.try_deserialize().map_err(|e| {
        // Provide a more helpful error message if admin_api_key is likely the cause
        if e.to_string().contains("admin_api_key") {
            ConfigError::Missing(
                "server.admin_api_key (must be set via KOLMODIN_SERVER__ADMIN_API_KEY env var)"
                    .to_string(),
            )
        } else {
            ConfigError::Load(e.to_string())
        }
    })?;

    // --- Post-deserialization validation ---
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
        WordListSourceType::None => {}
    }

    Ok(app_settings)
}
// ... (deserialize_string_or_list_to_set_lowercase remains the same)

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
