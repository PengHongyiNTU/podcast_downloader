use std::{path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, PodcastError>;

#[derive(Debug, Error)]
pub enum PodcastError {
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("feed parse error: {0}")]
    FeedParse(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("url parse error: {0}")]
    Url(#[from] url::ParseError),
    #[error("duplicate feed url: {0}")]
    DuplicateFeed(String),
    #[error("feed has no downloadable episodes: {0}")]
    NoDownloadableEpisodes(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("mp3 conversion is required but no encoder is configured")]
    Mp3EncoderUnavailable,
    #[error("mp3 conversion failed: {0}")]
    Mp3ConversionFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppErrorDto {
    pub kind: String,
    pub message: String,
}

impl From<&PodcastError> for AppErrorDto {
    fn from(error: &PodcastError) -> Self {
        let kind = match error {
            PodcastError::Database(_) => "database",
            PodcastError::Http(_) => "http",
            PodcastError::FeedParse(_) => "feed_parse",
            PodcastError::Io(_) => "io",
            PodcastError::Url(_) => "url",
            PodcastError::DuplicateFeed(_) => "duplicate_feed",
            PodcastError::NoDownloadableEpisodes(_) => "no_downloadable_episodes",
            PodcastError::NotFound(_) => "not_found",
            PodcastError::InvalidConfig(_) => "invalid_config",
            PodcastError::Mp3EncoderUnavailable => "mp3_encoder_unavailable",
            PodcastError::Mp3ConversionFailed(_) => "mp3_conversion_failed",
        };
        Self {
            kind: kind.to_string(),
            message: error.to_string(),
        }
    }
}

impl From<PodcastError> for AppErrorDto {
    fn from(error: PodcastError) -> Self {
        Self::from(&error)
    }
}

#[derive(Debug, Clone)]
pub struct CoreConfig {
    pub database_path: PathBuf,
    pub download_dir: PathBuf,
    pub log_file_path: Option<PathBuf>,
    pub country: String,
    pub http_timeout: Duration,
    pub user_agent: String,
    pub default_retention_limit: u32,
    pub max_concurrent_feed_fetches: usize,
    pub max_concurrent_downloads: usize,
    pub apple_search_base_url: String,
    pub ensure_mp3: bool,
    pub mp3_encoder_path: Option<PathBuf>,
}

impl CoreConfig {
    pub fn new(database_path: impl Into<PathBuf>, download_dir: impl Into<PathBuf>) -> Self {
        Self {
            database_path: database_path.into(),
            download_dir: download_dir.into(),
            log_file_path: None,
            country: "US".to_string(),
            http_timeout: Duration::from_secs(30),
            user_agent: "podcast-downloader/0.1".to_string(),
            default_retention_limit: 4,
            max_concurrent_feed_fetches: 4,
            max_concurrent_downloads: 2,
            apple_search_base_url: "https://itunes.apple.com/search".to_string(),
            ensure_mp3: true,
            mp3_encoder_path: None,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.max_concurrent_feed_fetches == 0 {
            return Err(PodcastError::InvalidConfig(
                "max_concurrent_feed_fetches must be greater than zero".to_string(),
            ));
        }
        if self.max_concurrent_downloads == 0 {
            return Err(PodcastError::InvalidConfig(
                "max_concurrent_downloads must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioEncoderStatus {
    Available { path: PathBuf, version: String },
    Missing { path: PathBuf },
    Error { path: PathBuf, error: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PodcastSearchResult {
    pub title: String,
    pub author: Option<String>,
    pub feed_url: Option<String>,
    pub artwork_url: Option<String>,
    pub apple_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedPreview {
    pub feed_url: String,
    pub raw_title: String,
    pub normalized_title: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub artwork_url: Option<String>,
    pub episodes: Vec<EpisodePreview>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodePreview {
    pub episode_key: String,
    pub raw_title: String,
    pub normalized_title: String,
    pub raw_author: Option<String>,
    pub published_at: Option<String>,
    pub media_url: String,
    pub media_content_type: Option<String>,
    pub media_length_bytes: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedSubscription {
    pub id: String,
    pub feed_url: String,
    pub raw_title: String,
    pub normalized_title: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub artwork_url: Option<String>,
    pub retention_limit: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
    pub last_checked_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeRecord {
    pub id: String,
    pub feed_id: String,
    pub episode_key: String,
    pub raw_title: String,
    pub normalized_title: String,
    pub raw_author: Option<String>,
    pub published_at: Option<String>,
    pub media_url: String,
    pub media_content_type: Option<String>,
    pub media_length_bytes: Option<i64>,
    pub status: EpisodeStatus,
    pub file_path: Option<String>,
    pub first_seen_at: String,
    pub downloaded_at: Option<String>,
    pub deleted_at: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeStatus {
    Pending,
    Downloaded,
    SkippedInitial,
    Failed,
    Deleted,
}

impl EpisodeStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Downloaded => "downloaded",
            Self::SkippedInitial => "skipped_initial",
            Self::Failed => "failed",
            Self::Deleted => "deleted",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "downloaded" => Self::Downloaded,
            "skipped_initial" => Self::SkippedInitial,
            "failed" => Self::Failed,
            "deleted" => Self::Deleted,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedCheckSummary {
    pub feed_id: String,
    pub feed_title: String,
    pub discovered: usize,
    pub queued: usize,
    pub downloaded: usize,
    pub skipped_initial: usize,
    pub failed: usize,
    pub deleted_by_retention: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckSummary {
    pub feeds_checked: usize,
    pub discovered: usize,
    pub queued: usize,
    pub downloaded: usize,
    pub skipped_initial: usize,
    pub failed: usize,
    pub deleted_by_retention: usize,
    pub feed_summaries: Vec<FeedCheckSummary>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionSummary {
    pub feeds_checked: usize,
    pub files_deleted: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryStats {
    pub feeds: usize,
    pub episodes: usize,
    pub downloaded: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadBatchSummary {
    pub requested: usize,
    pub queued: usize,
    pub downloaded: usize,
    pub failed: usize,
    pub feed_summaries: Vec<FeedCheckSummary>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadFailure {
    pub feed_id: String,
    pub episode_id: String,
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DownloadProgress {
    FeedStarted {
        feed_id: String,
        feed_title: String,
    },
    FeedFinished {
        feed_id: String,
        feed_title: String,
        queued: usize,
        downloaded: usize,
        failed: usize,
    },
    DownloadQueued {
        feed_id: String,
        episode_id: String,
        episode_title: String,
    },
    DownloadStarted {
        feed_id: String,
        episode_id: String,
        episode_title: String,
        total_bytes: Option<u64>,
    },
    DownloadAdvanced {
        feed_id: String,
        episode_id: String,
        episode_title: String,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
    },
    ConversionStarted {
        feed_id: String,
        episode_id: String,
        episode_title: String,
    },
    ConversionFinished {
        feed_id: String,
        episode_id: String,
        episode_title: String,
    },
    DownloadFinished {
        feed_id: String,
        episode_id: String,
        episode_title: String,
        file_path: String,
    },
    DownloadFailed {
        feed_id: String,
        episode_id: String,
        episode_title: String,
        error: String,
    },
}
