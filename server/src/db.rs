use crate::config::{DatabaseConfig, MedAndraOrdWordsConfig, WordListSourceType};
use crate::error::{DbError, Result as AppResult};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct GameDataFile {
    pub twitch_whitelist: Vec<String>,
    pub medandraord_words: Vec<String>,
}

pub struct DataFileParser;

impl DataFileParser {
    pub fn parse_data_file(content: &str) -> Result<GameDataFile, DbError> {
        let mut sections = HashMap::new();
        let mut current_section: Option<String> = None;
        let mut current_items = Vec::new();

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() {
                continue;
            }

            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                if let Some(section_name) = current_section.take() {
                    sections.insert(section_name, current_items.clone());
                    current_items.clear();
                }

                let section_name = trimmed[1..trimmed.len() - 1].to_string();
                current_section = Some(section_name);
            } else if current_section.is_some() {
                current_items.push(trimmed.to_string());
            }
        }

        if let Some(section_name) = current_section {
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

        Ok(GameDataFile {
            twitch_whitelist,
            medandraord_words,
        })
    }
}

pub struct WordListManager {
    med_andra_ord_words: RwLock<Arc<Vec<String>>>,
    twitch_whitelist: RwLock<Arc<Vec<String>>>,
    config: DatabaseConfig,
}

impl WordListManager {
    pub async fn new(config: DatabaseConfig) -> AppResult<Self> {
        let initial_data = Self::fetch_data_from_sources(&config)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!("Initial data load failed: {:?}. Using empty lists.", err);
                GameDataFile::default()
            });

        Ok(Self {
            med_andra_ord_words: RwLock::new(Arc::new(initial_data.medandraord_words)),
            twitch_whitelist: RwLock::new(Arc::new(initial_data.twitch_whitelist)),
            config,
        })
    }

    async fn fetch_data_from_sources(config: &DatabaseConfig) -> Result<GameDataFile, DbError> {
        let data_file_path = &config.data_file.file_path;
        tracing::info!("Attempting to read data from file: {}", data_file_path);

        if let Ok(content) = fs::read_to_string(data_file_path) {
            tracing::info!("Successfully read data file, parsing sections");
            let parsed_data = DataFileParser::parse_data_file(&content)?;

            tracing::info!(
                "Parsed data file: {} twitch channels, {} words",
                parsed_data.twitch_whitelist.len(),
                parsed_data.medandraord_words.len()
            );

            return Ok(parsed_data);
        }

        tracing::warn!(
            "Could not read data file {}, falling back to legacy word source",
            data_file_path
        );

        let words = Self::fetch_words_from_legacy_source(&config.med_andra_ord_words)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(
                    "Legacy word source also failed: {:?}. Using empty list.",
                    err
                );
                Vec::new()
            });

        Ok(GameDataFile {
            twitch_whitelist: Vec::new(),
            medandraord_words: words,
        })
    }

    async fn fetch_words_from_legacy_source(
        mao_config: &MedAndraOrdWordsConfig,
    ) -> Result<Vec<String>, DbError> {
        tracing::info!(
            "Fetching MedAndraOrd words from legacy source: {:?}",
            mao_config.source_type
        );
        let content = match mao_config.source_type {
            WordListSourceType::File => {
                let path = mao_config.file_path.as_ref().ok_or_else(|| {
                    DbError::Config("File path missing for file source type".to_string())
                })?;
                tracing::info!("Reading MedAndraOrd words from legacy file: {}", path);
                fs::read_to_string(path).map_err(|e| DbError::FileRead {
                    path: path.clone(),
                    source: e,
                })?
            }
            WordListSourceType::Http => {
                let url = mao_config.http_url.as_ref().ok_or_else(|| {
                    DbError::Config("HTTP URL missing for http source type".to_string())
                })?;
                tracing::info!("Fetching MedAndraOrd words from URL: {}", url);
                reqwest::get(url)
                    .await
                    .map_err(|e| DbError::HttpFetch {
                        url: url.clone(),
                        source: e,
                    })?
                    .text()
                    .await
                    .map_err(|e| DbError::HttpFetch {
                        url: url.clone(),
                        source: e,
                    })?
            }
            WordListSourceType::None => {
                tracing::info!("MedAndraOrd word source type is None. Returning empty list.");
                return Ok(Vec::new());
            }
        };

        let words: Vec<String> = content
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect();

        if words.is_empty() && mao_config.source_type != WordListSourceType::None {
            tracing::warn!("Fetched MedAndraOrd word list is empty.");
        }
        tracing::info!(
            "Successfully loaded {} MedAndraOrd words from legacy source.",
            words.len()
        );
        Ok(words)
    }

    pub async fn refresh_data(&self) -> AppResult<()> {
        match Self::fetch_data_from_sources(&self.config).await {
            Ok(new_data) => {
                {
                    let mut words_guard = self.med_andra_ord_words.write().await;
                    *words_guard = Arc::new(new_data.medandraord_words);
                    tracing::info!(
                        "Successfully refreshed MedAndraOrd words. New count: {}",
                        words_guard.len()
                    );
                }
                {
                    let mut whitelist_guard = self.twitch_whitelist.write().await;
                    *whitelist_guard = Arc::new(new_data.twitch_whitelist);
                    tracing::info!(
                        "Successfully refreshed Twitch whitelist. New count: {}",
                        whitelist_guard.len()
                    );
                }
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to refresh data: {:?}", e);
                Err(e.into())
            }
        }
    }

    pub async fn refresh_med_andra_ord_words(&self) -> AppResult<()> {
        self.refresh_data().await
    }

    pub async fn get_med_andra_ord_words(&self) -> Arc<Vec<String>> {
        self.med_andra_ord_words.read().await.clone()
    }

    pub async fn get_twitch_whitelist(&self) -> Arc<Vec<String>> {
        self.twitch_whitelist.read().await.clone()
    }

    pub async fn is_twitch_channel_allowed(&self, channel_name: &str) -> bool {
        let whitelist = self.twitch_whitelist.read().await;
        if whitelist.is_empty() {
            return true;
        }

        let normalized_channel = channel_name.to_lowercase();
        whitelist.contains(&normalized_channel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_data_file_parser() {
        let test_content = r#"[twitch_whitelist]
testchannel
example_user


[medandraord_words]
word1
word2
word3
"#;

        let result = DataFileParser::parse_data_file(test_content).unwrap();

        assert_eq!(result.twitch_whitelist, vec!["testchannel", "example_user"]);
        assert_eq!(result.medandraord_words, vec!["word1", "word2", "word3"]);
    }

    #[test]
    fn test_data_file_parser_empty_sections() {
        let test_content = r#"[twitch_whitelist]

[medandraord_words]
"#;

        let result = DataFileParser::parse_data_file(test_content).unwrap();

        assert!(result.twitch_whitelist.is_empty());
        assert!(result.medandraord_words.is_empty());
    }

    #[test]
    fn test_data_file_parser_missing_sections() {
        let test_content = r#"[other_section]
ignored_item
"#;

        let result = DataFileParser::parse_data_file(test_content).unwrap();

        assert!(result.twitch_whitelist.is_empty());
        assert!(result.medandraord_words.is_empty());
    }
}
