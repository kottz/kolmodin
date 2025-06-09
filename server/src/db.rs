use crate::config::{DataSourceType, DatabaseConfig};
use crate::error::{DbError, Result as AppResult};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct GameData {
    pub twitch_whitelist: Vec<String>,
    pub medandraord_words: Vec<String>,
}

#[async_trait::async_trait]
pub trait DataSource {
    async fn load(&self) -> Result<String, DbError>;
}

pub struct FileDataSource {
    file_path: String,
}

impl FileDataSource {
    pub fn new(file_path: String) -> Self {
        Self { file_path }
    }
}

#[async_trait::async_trait]
impl DataSource for FileDataSource {
    #[tracing::instrument(skip(self), fields(file.path = %self.file_path))]
    async fn load(&self) -> Result<String, DbError> {
        tracing::debug!("Loading data from file");
        tokio::fs::read_to_string(&self.file_path)
            .await
            .map_err(|e| DbError::FileRead {
                path: self.file_path.clone(),
                source: e,
            })
    }
}

pub struct HttpDataSource {
    url: String,
}

impl HttpDataSource {
    pub fn new(url: String) -> Self {
        Self { url }
    }
}

#[async_trait::async_trait]
impl DataSource for HttpDataSource {
    #[tracing::instrument(skip(self), fields(http.url = %self.url))]
    async fn load(&self) -> Result<String, DbError> {
        tracing::debug!("Fetching data from URL");
        let response = reqwest::get(&self.url)
            .await
            .map_err(|e| DbError::HttpFetch {
                url: self.url.clone(),
                source: e,
            })?;

        response.text().await.map_err(|e| DbError::HttpFetch {
            url: self.url.clone(),
            source: e,
        })
    }
}

pub struct DataFileParser;

impl DataFileParser {
    #[tracing::instrument(skip(content), fields(content.length = content.len()))]
    pub fn parse_structured_data(content: &str) -> Result<GameData, DbError> {
        tracing::debug!("Parsing structured data");
        let mut sections = HashMap::new();
        let mut current_section: Option<String> = None;
        let mut current_items = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            if let Some(section_name) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
            {
                if let Some(prev_section) = current_section.take() {
                    tracing::debug!(
                        section.name = %prev_section,
                        section.items.count = current_items.len(),
                        "Parsed section"
                    );
                    sections.insert(prev_section, current_items.clone());
                    current_items.clear();
                }
                current_section = Some(section_name.to_string());
            } else if current_section.is_some() {
                current_items.push(trimmed.to_string());
            }
        }

        if let Some(section_name) = current_section {
            tracing::debug!(
                section.name = %section_name,
                section.items.count = current_items.len(),
                "Parsed section"
            );
            sections.insert(section_name, current_items);
        }

        let twitch_whitelist = sections
            .get("twitch_whitelist")
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        let medandraord_words = sections
            .get("medandraord_words")
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        Ok(GameData {
            twitch_whitelist,
            medandraord_words,
        })
    }
}

pub struct DataManager {
    data_source: Box<dyn DataSource + Send + Sync>,
}

impl DataManager {
    pub fn new(config: &DatabaseConfig) -> Result<Self, DbError> {
        let data_source = match &config.source_type {
            DataSourceType::File => {
                let file_path = config.file_path.as_ref().ok_or_else(|| {
                    DbError::Config("File path required for file source".to_string())
                })?;
                Box::new(FileDataSource::new(file_path.clone()))
                    as Box<dyn DataSource + Send + Sync>
            }
            DataSourceType::Http => {
                let url = config.http_url.as_ref().ok_or_else(|| {
                    DbError::Config("HTTP URL required for http source".to_string())
                })?;
                Box::new(HttpDataSource::new(url.clone())) as Box<dyn DataSource + Send + Sync>
            }
        };

        Ok(Self { data_source })
    }

    #[tracing::instrument(skip(self))]
    pub async fn load_game_data(&self) -> Result<GameData, DbError> {
        let content = self.data_source.load().await?;
        let game_data = DataFileParser::parse_structured_data(&content)?;

        tracing::info!(
            twitch.channels.count = game_data.twitch_whitelist.len(),
            words.count = game_data.medandraord_words.len(),
            "Loaded structured data"
        );

        Ok(game_data)
    }
}

pub struct WordListManager {
    medandraord_words: RwLock<Arc<Vec<String>>>,
    twitch_whitelist: RwLock<Arc<Vec<String>>>,
    data_manager: DataManager,
}

impl WordListManager {
    #[tracing::instrument(skip(config), fields(
        data.source_type = ?config.source_type,
        data.file_path = ?config.file_path,
        data.http_url = ?config.http_url
    ))]
    pub async fn new(config: DatabaseConfig) -> AppResult<Self> {
        let data_manager = DataManager::new(&config)?;
        let initial_data = data_manager.load_game_data().await.map_err(|err| {
            tracing::error!(error = %err, "Failed to load required data file");
            err
        })?;

        tracing::info!(
            twitch.channels.count = initial_data.twitch_whitelist.len(),
            words.count = initial_data.medandraord_words.len(),
            "WordListManager initialized successfully"
        );

        Ok(Self {
            medandraord_words: RwLock::new(Arc::new(initial_data.medandraord_words)),
            twitch_whitelist: RwLock::new(Arc::new(initial_data.twitch_whitelist)),
            data_manager,
        })
    }

    #[tracing::instrument(skip(self))]
    pub async fn refresh_data(&self) -> AppResult<()> {
        tracing::info!("Refreshing game data");
        let new_data = self.data_manager.load_game_data().await?;

        {
            let mut words_guard = self.medandraord_words.write().await;
            *words_guard = Arc::new(new_data.medandraord_words);
            tracing::info!(
                words.count = words_guard.len(),
                "Refreshed medandraord words"
            );
        }

        {
            let mut whitelist_guard = self.twitch_whitelist.write().await;
            *whitelist_guard = Arc::new(new_data.twitch_whitelist);
            tracing::info!(
                twitch.channels.count = whitelist_guard.len(),
                "Refreshed twitch whitelist"
            );
        }

        Ok(())
    }

    pub async fn refresh_med_andra_ord_words(&self) -> AppResult<()> {
        self.refresh_data().await
    }

    pub async fn get_med_andra_ord_words(&self) -> Arc<Vec<String>> {
        self.medandraord_words.read().await.clone()
    }

    pub async fn get_twitch_whitelist(&self) -> Arc<Vec<String>> {
        self.twitch_whitelist.read().await.clone()
    }

    #[tracing::instrument(skip(self), fields(channel.name = %channel_name))]
    pub async fn is_twitch_channel_allowed(&self, channel_name: &str) -> bool {
        let whitelist = self.twitch_whitelist.read().await;
        let is_empty = whitelist.is_empty();
        let normalized_channel = channel_name.to_lowercase();
        let is_allowed = is_empty || whitelist.contains(&normalized_channel);

        tracing::trace!(
            channel.normalized = %normalized_channel,
            whitelist.empty = is_empty,
            whitelist.count = whitelist.len(),
            result = is_allowed,
            "Checking if Twitch channel is allowed"
        );

        is_allowed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_structured_data() {
        let content = r#"[twitch_whitelist]
testchannel
example_user

[medandraord_words]
word1
word2
word3
"#;

        let result = DataFileParser::parse_structured_data(content).unwrap();
        assert_eq!(result.twitch_whitelist, vec!["testchannel", "example_user"]);
        assert_eq!(result.medandraord_words, vec!["word1", "word2", "word3"]);
    }

    #[test]
    fn test_parse_structured_data_empty_sections() {
        let content = r#"[twitch_whitelist]

[medandraord_words]
"#;

        let result = DataFileParser::parse_structured_data(content).unwrap();
        assert!(result.twitch_whitelist.is_empty());
        assert!(result.medandraord_words.is_empty());
    }

    #[test]
    fn test_parse_structured_data_missing_sections() {
        let content = r#"[other_section]
ignored_item
"#;

        let result = DataFileParser::parse_structured_data(content).unwrap();
        assert!(result.twitch_whitelist.is_empty());
        assert!(result.medandraord_words.is_empty());
    }
}
