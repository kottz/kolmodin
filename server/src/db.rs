use crate::config::{DataSourceType, DatabaseConfig};
use crate::error::{DbError, Result as AppResult};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// Trivial Pursuit structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrivialPursuitQuestion {
    pub id: u32,
    pub question: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_info: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrivialPursuitCard {
    pub id: u32,
    pub questions: Vec<TrivialPursuitQuestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrivialPursuitData {
    pub cards: Vec<TrivialPursuitCard>,
}

// Vem Vet Mest structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VemVetMestQuestion {
    pub question: String,
    pub answer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra_info: Option<String>,
}

// Kolmodin legacy data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KolmodinData {
    pub twitch_whitelist: Vec<String>,
    pub medandraord_words: Vec<String>,
}

// Root data structure matching the JSON schema
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonGameData {
    pub kolmodin: KolmodinData,
    pub trivial_pursuit: TrivialPursuitData,
    pub vem_vet_mest: Vec<VemVetMestQuestion>,
}

// Main game data structure
#[derive(Debug, Clone)]
pub struct GameData {
    pub twitch_whitelist: Vec<String>,
    pub medandraord_words: Vec<String>,
    pub trivial_pursuit: TrivialPursuitData,
    pub vem_vet_mest: Vec<VemVetMestQuestion>,
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
    /// Parse JSON structured data
    #[tracing::instrument(skip(content), fields(content.length = content.len()))]
    pub fn parse_structured_data(content: &str) -> Result<GameData, DbError> {
        tracing::debug!("Parsing JSON structured data");

        let json_data: JsonGameData = serde_json::from_str(content)
            .map_err(|e| DbError::Parse(format!("Failed to parse JSON: {}", e)))?;

        Ok(GameData {
            twitch_whitelist: json_data
                .kolmodin
                .twitch_whitelist
                .into_iter()
                .map(|s| s.to_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
            medandraord_words: json_data
                .kolmodin
                .medandraord_words
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            trivial_pursuit: json_data.trivial_pursuit,
            vem_vet_mest: json_data.vem_vet_mest,
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
            trivial_pursuit.cards.count = game_data.trivial_pursuit.cards.len(),
            vem_vet_mest.questions.count = game_data.vem_vet_mest.len(),
            "Loaded structured data"
        );

        Ok(game_data)
    }
}

pub struct WordListManager {
    medandraord_words: RwLock<Arc<Vec<String>>>,
    twitch_whitelist: RwLock<Arc<Vec<String>>>,
    trivial_pursuit_data: RwLock<Option<Arc<TrivialPursuitData>>>,
    vem_vet_mest_questions: RwLock<Arc<Vec<VemVetMestQuestion>>>,
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
            trivial_pursuit.cards.count = initial_data.trivial_pursuit.cards.len(),
            vem_vet_mest.questions.count = initial_data.vem_vet_mest.len(),
            "WordListManager initialized successfully"
        );

        Ok(Self {
            medandraord_words: RwLock::new(Arc::new(initial_data.medandraord_words)),
            twitch_whitelist: RwLock::new(Arc::new(initial_data.twitch_whitelist)),
            trivial_pursuit_data: RwLock::new(Some(Arc::new(initial_data.trivial_pursuit))),
            vem_vet_mest_questions: RwLock::new(Arc::new(initial_data.vem_vet_mest)),
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

        {
            let mut trivial_pursuit_guard = self.trivial_pursuit_data.write().await;
            *trivial_pursuit_guard = Some(Arc::new(new_data.trivial_pursuit));
            tracing::info!(
                trivial_pursuit.cards.count = trivial_pursuit_guard
                    .as_ref()
                    .map(|tp| tp.cards.len())
                    .unwrap_or(0),
                "Refreshed trivial pursuit data"
            );
        }

        {
            let mut vem_vet_mest_guard = self.vem_vet_mest_questions.write().await;
            *vem_vet_mest_guard = Arc::new(new_data.vem_vet_mest);
            tracing::info!(
                vem_vet_mest.questions.count = vem_vet_mest_guard.len(),
                "Refreshed vem vet mest questions"
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

    /// Get Trivial Pursuit data for quiz games
    pub async fn get_trivial_pursuit_data(&self) -> Option<Arc<TrivialPursuitData>> {
        self.trivial_pursuit_data.read().await.clone()
    }

    /// Get Vem Vet Mest questions for quiz games
    pub async fn get_vem_vet_mest_questions(&self) -> Arc<Vec<VemVetMestQuestion>> {
        self.vem_vet_mest_questions.read().await.clone()
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
    fn test_parse_json_data() {
        let content = r#"{
  "kolmodin": {
    "twitch_whitelist": ["testchannel", "example_user"],
    "medandraord_words": ["word1", "word2", "word3"]
  },
  "trivial_pursuit": {
    "cards": [
      {
        "id": 1,
        "questions": [
          {
            "id": 1,
            "question": "What is 2+2?",
            "answer": "4",
            "extra_info": "Basic math"
          }
        ]
      }
    ]
  },
  "vem_vet_mest": [
    {
      "question": "What is the capital of Sweden?",
      "answer": "Stockholm",
      "category": "Geography"
    }
  ]
}"#;

        let result = DataFileParser::parse_structured_data(content).unwrap();
        assert_eq!(result.twitch_whitelist, vec!["testchannel", "example_user"]);
        assert_eq!(result.medandraord_words, vec!["word1", "word2", "word3"]);

        assert_eq!(result.trivial_pursuit.cards.len(), 1);
        assert_eq!(result.trivial_pursuit.cards[0].questions.len(), 1);
        assert_eq!(
            result.trivial_pursuit.cards[0].questions[0].question,
            "What is 2+2?"
        );

        assert_eq!(result.vem_vet_mest.len(), 1);
        assert_eq!(
            result.vem_vet_mest[0].question,
            "What is the capital of Sweden?"
        );
    }
}
