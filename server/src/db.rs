// src/db.rs

use crate::config::{DatabaseConfig, MedAndraOrdWordsConfig, WordListSourceType};
use crate::error::{AppError, DbError, Result as AppResult};
use std::fs;
use std::sync::Arc;
use tokio::sync::RwLock; // Use tokio's RwLock for async contexts if methods become async

pub struct WordListManager {
    med_andra_ord_words: RwLock<Arc<Vec<String>>>,
    // Store config for refresh
    config: DatabaseConfig,
}

impl WordListManager {
    pub async fn new(config: DatabaseConfig) -> AppResult<Self> {
        let initial_words = Self::fetch_words_from_source(&config.med_andra_ord_words)
            .await
            .unwrap_or_else(|err| {
                tracing::warn!(
                    "Initial MedAndraOrd word load failed: {:?}. Using empty list.",
                    err
                );
                Vec::new()
            });

        Ok(Self {
            med_andra_ord_words: RwLock::new(Arc::new(initial_words)),
            config,
        })
    }

    async fn fetch_words_from_source(
        mao_config: &MedAndraOrdWordsConfig,
    ) -> Result<Vec<String>, DbError> {
        tracing::info!(
            "Fetching MedAndraOrd words from source: {:?}",
            mao_config.source_type
        );
        let content = match mao_config.source_type {
            WordListSourceType::File => {
                let path = mao_config.file_path.as_ref().ok_or_else(|| {
                    DbError::Config("File path missing for file source type".to_string())
                })?;
                tracing::info!("Reading MedAndraOrd words from file: {}", path);
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
                return Ok(Vec::new()); // Or return Err(DbError::SourceIsNone) if empty is not desired
            }
        };

        let words: Vec<String> = content
            .lines()
            .map(|line| line.trim().to_lowercase())
            .filter(|line| !line.is_empty())
            .collect();

        if words.is_empty() && mao_config.source_type != WordListSourceType::None {
            tracing::warn!("Fetched MedAndraOrd word list is empty.");
            // Depending on strictness, you might return Err(DbError::EmptyOrInvalidData)
        }
        tracing::info!("Successfully loaded {} MedAndraOrd words.", words.len());
        Ok(words)
    }

    pub async fn refresh_med_andra_ord_words(&self) -> AppResult<()> {
        // Corrected: self.config is DatabaseConfig, so access med_andra_ord_words directly
        match Self::fetch_words_from_source(&self.config.med_andra_ord_words).await {
            Ok(new_words) => {
                let mut guard = self.med_andra_ord_words.write().await;
                *guard = Arc::new(new_words);
                tracing::info!(
                    "Successfully refreshed MedAndraOrd words. New count: {}",
                    guard.len()
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!("Failed to refresh MedAndraOrd words: {:?}", e);
                Err(e.into())
            }
        }
    }

    pub async fn get_med_andra_ord_words(&self) -> Arc<Vec<String>> {
        self.med_andra_ord_words.read().await.clone()
    }
}
