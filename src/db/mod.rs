use std::{path::Path, str::FromStr};

use chrono::Utc;
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};
use uuid::Uuid;

use crate::{
    core::{EpisodeRecord, EpisodeStatus, FeedSubscription, LibraryStats, PodcastError, Result},
    feeds::{ParsedEpisode, ParsedFeed},
};

#[derive(Debug, Clone)]
pub struct Db {
    pool: SqlitePool,
}

impl Db {
    pub async fn open(database_path: &Path) -> Result<Self> {
        if let Some(parent) = database_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let options = SqliteConnectOptions::from_str(&format!(
            "sqlite://{}",
            database_path.to_string_lossy().replace('\\', "/")
        ))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        let db = Self { pool };
        db.init().await?;
        Ok(db)
    }

    pub async fn init(&self) -> Result<()> {
        for statement in SCHEMA
            .split(";")
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            sqlx::query(statement).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn insert_config_defaults(&self, default_retention: u32) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO config (key, value) VALUES ('default_retention_limit', ?)",
        )
        .bind(default_retention.to_string())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn feed_by_url(&self, feed_url: &str) -> Result<Option<FeedSubscription>> {
        let row = sqlx::query(FEED_SELECT_BY_URL)
            .bind(feed_url)
            .fetch_optional(&self.pool)
            .await?;
        row.map(feed_from_row).transpose()
    }

    pub async fn feed_by_id(&self, feed_id: &str) -> Result<Option<FeedSubscription>> {
        let row = sqlx::query(FEED_SELECT_BY_ID)
            .bind(feed_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(feed_from_row).transpose()
    }

    pub async fn list_feeds(&self) -> Result<Vec<FeedSubscription>> {
        let rows = sqlx::query(
            "SELECT id, feed_url, raw_title, normalized_title, site_url, description, artwork_url, retention_limit, created_at, updated_at, last_checked_at FROM feeds ORDER BY normalized_title",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(feed_from_row).collect()
    }

    pub async fn library_stats(&self) -> Result<LibraryStats> {
        let row = sqlx::query(
            "SELECT
                (SELECT COUNT(*) FROM feeds) AS feeds,
                (SELECT COUNT(*) FROM episodes) AS episodes,
                (SELECT COUNT(*) FROM episodes WHERE status = 'downloaded') AS downloaded",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(LibraryStats {
            feeds: usize::try_from(row.get::<i64, _>("feeds")).unwrap_or_default(),
            episodes: usize::try_from(row.get::<i64, _>("episodes")).unwrap_or_default(),
            downloaded: usize::try_from(row.get::<i64, _>("downloaded")).unwrap_or_default(),
        })
    }

    pub async fn insert_feed(
        &self,
        feed_url: &str,
        parsed: &ParsedFeed,
    ) -> Result<FeedSubscription> {
        if self.feed_by_url(feed_url).await?.is_some() {
            return Err(PodcastError::DuplicateFeed(feed_url.to_string()));
        }

        let id = Uuid::new_v4().to_string();
        let now = now();
        sqlx::query(
            "INSERT INTO feeds (id, feed_url, raw_title, normalized_title, site_url, description, artwork_url, retention_limit, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, NULL, ?, ?)",
        )
        .bind(&id)
        .bind(feed_url)
        .bind(&parsed.raw_title)
        .bind(&parsed.normalized_title)
        .bind(&parsed.site_url)
        .bind(&parsed.description)
        .bind(&parsed.artwork_url)
        .bind(&now)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.feed_by_id(&id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(id))
    }

    pub async fn update_feed_metadata(&self, feed_id: &str, parsed: &ParsedFeed) -> Result<()> {
        sqlx::query(
            "UPDATE feeds SET raw_title = ?, normalized_title = ?, site_url = ?, description = ?, artwork_url = ?, updated_at = ? WHERE id = ?",
        )
        .bind(&parsed.raw_title)
        .bind(&parsed.normalized_title)
        .bind(&parsed.site_url)
        .bind(&parsed.description)
        .bind(&parsed.artwork_url)
        .bind(now())
        .bind(feed_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_feed_checked(&self, feed_id: &str) -> Result<()> {
        sqlx::query("UPDATE feeds SET last_checked_at = ?, updated_at = ? WHERE id = ?")
            .bind(now())
            .bind(now())
            .bind(feed_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn remove_feed(&self, feed_id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM feeds WHERE id = ?")
            .bind(feed_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(PodcastError::NotFound(feed_id.to_string()));
        }
        Ok(())
    }

    pub async fn set_feed_retention(
        &self,
        feed_id: &str,
        retention_limit: Option<u32>,
    ) -> Result<()> {
        let result =
            sqlx::query("UPDATE feeds SET retention_limit = ?, updated_at = ? WHERE id = ?")
                .bind(retention_limit.map(i64::from))
                .bind(now())
                .bind(feed_id)
                .execute(&self.pool)
                .await?;
        if result.rows_affected() == 0 {
            return Err(PodcastError::NotFound(feed_id.to_string()));
        }
        Ok(())
    }

    pub async fn count_episodes(&self, feed_id: &str) -> Result<i64> {
        let row = sqlx::query("SELECT COUNT(*) AS count FROM episodes WHERE feed_id = ?")
            .bind(feed_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get("count"))
    }

    pub async fn episode_by_key(
        &self,
        feed_id: &str,
        episode_key: &str,
    ) -> Result<Option<EpisodeRecord>> {
        let row = sqlx::query(EPISODE_SELECT_BY_KEY)
            .bind(feed_id)
            .bind(episode_key)
            .fetch_optional(&self.pool)
            .await?;
        row.map(episode_from_row).transpose()
    }

    pub async fn insert_episode(
        &self,
        feed_id: &str,
        episode: &ParsedEpisode,
        status: EpisodeStatus,
    ) -> Result<EpisodeRecord> {
        let id = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO episodes
             (id, feed_id, episode_key, raw_title, normalized_title, raw_author, published_at, media_url, media_content_type, media_length_bytes, status, first_seen_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(feed_id)
        .bind(&episode.episode_key)
        .bind(&episode.raw_title)
        .bind(&episode.normalized_title)
        .bind(&episode.raw_author)
        .bind(&episode.published_at)
        .bind(&episode.media_url)
        .bind(&episode.media_content_type)
        .bind(episode.media_length_bytes)
        .bind(status.as_str())
        .bind(now())
        .execute(&self.pool)
        .await?;
        self.episode_by_id(&id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(id))
    }

    pub async fn mark_episode_pending(&self, episode_id: &str) -> Result<()> {
        sqlx::query("UPDATE episodes SET status = 'pending', last_error = NULL WHERE id = ?")
            .bind(episode_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_episode_downloaded(&self, episode_id: &str, file_path: &Path) -> Result<()> {
        sqlx::query(
            "UPDATE episodes SET status = 'downloaded', file_path = ?, downloaded_at = ?, deleted_at = NULL, last_error = NULL WHERE id = ?",
        )
        .bind(file_path.to_string_lossy().to_string())
        .bind(now())
        .bind(episode_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_episode_failed(&self, episode_id: &str, error: &str) -> Result<()> {
        sqlx::query("UPDATE episodes SET status = 'failed', last_error = ? WHERE id = ?")
            .bind(error)
            .bind(episode_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn mark_episode_deleted(&self, episode_id: &str) -> Result<()> {
        sqlx::query("UPDATE episodes SET status = 'deleted', deleted_at = ? WHERE id = ?")
            .bind(now())
            .bind(episode_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn episode_by_id(&self, episode_id: &str) -> Result<Option<EpisodeRecord>> {
        let row = sqlx::query(EPISODE_SELECT_BY_ID)
            .bind(episode_id)
            .fetch_optional(&self.pool)
            .await?;
        row.map(episode_from_row).transpose()
    }

    pub async fn downloaded_episodes_for_feed(&self, feed_id: &str) -> Result<Vec<EpisodeRecord>> {
        let rows = sqlx::query(
            "SELECT id, feed_id, episode_key, raw_title, normalized_title, raw_author, published_at, media_url, media_content_type, media_length_bytes, status, file_path, first_seen_at, downloaded_at, deleted_at, last_error
             FROM episodes
             WHERE feed_id = ? AND status = 'downloaded'
             ORDER BY COALESCE(published_at, downloaded_at, first_seen_at) DESC, downloaded_at DESC",
        )
        .bind(feed_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(episode_from_row).collect()
    }

    pub async fn episodes_with_files_for_feed(&self, feed_id: &str) -> Result<Vec<EpisodeRecord>> {
        let rows = sqlx::query(
            "SELECT id, feed_id, episode_key, raw_title, normalized_title, raw_author, published_at, media_url, media_content_type, media_length_bytes, status, file_path, first_seen_at, downloaded_at, deleted_at, last_error
             FROM episodes
             WHERE feed_id = ? AND file_path IS NOT NULL
             ORDER BY COALESCE(published_at, downloaded_at, first_seen_at) DESC, downloaded_at DESC",
        )
        .bind(feed_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(episode_from_row).collect()
    }

    pub async fn episodes_for_feed(&self, feed_id: &str) -> Result<Vec<EpisodeRecord>> {
        let rows = sqlx::query(
            "SELECT id, feed_id, episode_key, raw_title, normalized_title, raw_author, published_at, media_url, media_content_type, media_length_bytes, status, file_path, first_seen_at, downloaded_at, deleted_at, last_error
             FROM episodes
             WHERE feed_id = ?
             ORDER BY COALESCE(published_at, downloaded_at, first_seen_at) DESC, first_seen_at DESC",
        )
        .bind(feed_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(episode_from_row).collect()
    }

    pub async fn start_attempt(&self, episode_id: &str) -> Result<String> {
        let id = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO download_attempts (id, episode_id, started_at, status) VALUES (?, ?, ?, 'started')")
            .bind(&id)
            .bind(episode_id)
            .bind(now())
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn finish_attempt(
        &self,
        attempt_id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE download_attempts SET finished_at = ?, status = ?, error = ? WHERE id = ?",
        )
        .bind(now())
        .bind(status)
        .bind(error)
        .bind(attempt_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn feed_from_row(row: sqlx::sqlite::SqliteRow) -> Result<FeedSubscription> {
    let retention_limit: Option<i64> = row.try_get("retention_limit")?;
    Ok(FeedSubscription {
        id: row.try_get("id")?,
        feed_url: row.try_get("feed_url")?,
        raw_title: row.try_get("raw_title")?,
        normalized_title: row.try_get("normalized_title")?,
        site_url: row.try_get("site_url")?,
        description: row.try_get("description")?,
        artwork_url: row.try_get("artwork_url")?,
        retention_limit: retention_limit.and_then(|value| u32::try_from(value).ok()),
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
        last_checked_at: row.try_get("last_checked_at")?,
    })
}

fn episode_from_row(row: sqlx::sqlite::SqliteRow) -> Result<EpisodeRecord> {
    let status: String = row.try_get("status")?;
    Ok(EpisodeRecord {
        id: row.try_get("id")?,
        feed_id: row.try_get("feed_id")?,
        episode_key: row.try_get("episode_key")?,
        raw_title: row.try_get("raw_title")?,
        normalized_title: row.try_get("normalized_title")?,
        raw_author: row.try_get("raw_author")?,
        published_at: row.try_get("published_at")?,
        media_url: row.try_get("media_url")?,
        media_content_type: row.try_get("media_content_type")?,
        media_length_bytes: row.try_get("media_length_bytes")?,
        status: EpisodeStatus::from_db(&status),
        file_path: row.try_get("file_path")?,
        first_seen_at: row.try_get("first_seen_at")?,
        downloaded_at: row.try_get("downloaded_at")?,
        deleted_at: row.try_get("deleted_at")?,
        last_error: row.try_get("last_error")?,
    })
}

const FEED_SELECT_BY_ID: &str = "SELECT id, feed_url, raw_title, normalized_title, site_url, description, artwork_url, retention_limit, created_at, updated_at, last_checked_at FROM feeds WHERE id = ?";
const FEED_SELECT_BY_URL: &str = "SELECT id, feed_url, raw_title, normalized_title, site_url, description, artwork_url, retention_limit, created_at, updated_at, last_checked_at FROM feeds WHERE feed_url = ?";
const EPISODE_SELECT_BY_ID: &str = "SELECT id, feed_id, episode_key, raw_title, normalized_title, raw_author, published_at, media_url, media_content_type, media_length_bytes, status, file_path, first_seen_at, downloaded_at, deleted_at, last_error FROM episodes WHERE id = ?";
const EPISODE_SELECT_BY_KEY: &str = "SELECT id, feed_id, episode_key, raw_title, normalized_title, raw_author, published_at, media_url, media_content_type, media_length_bytes, status, file_path, first_seen_at, downloaded_at, deleted_at, last_error FROM episodes WHERE feed_id = ? AND episode_key = ?";

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS feeds (
    id TEXT PRIMARY KEY NOT NULL,
    feed_url TEXT NOT NULL UNIQUE,
    raw_title TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    site_url TEXT,
    description TEXT,
    artwork_url TEXT,
    retention_limit INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_checked_at TEXT
);

CREATE TABLE IF NOT EXISTS episodes (
    id TEXT PRIMARY KEY NOT NULL,
    feed_id TEXT NOT NULL REFERENCES feeds(id) ON DELETE CASCADE,
    episode_key TEXT NOT NULL,
    raw_title TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    raw_author TEXT,
    published_at TEXT,
    media_url TEXT NOT NULL,
    media_content_type TEXT,
    media_length_bytes INTEGER,
    status TEXT NOT NULL,
    file_path TEXT,
    first_seen_at TEXT NOT NULL,
    downloaded_at TEXT,
    deleted_at TEXT,
    last_error TEXT,
    UNIQUE(feed_id, episode_key)
);

CREATE TABLE IF NOT EXISTS download_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    episode_id TEXT NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    error TEXT
);
"#;
