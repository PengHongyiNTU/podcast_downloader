use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use futures::{StreamExt, stream};
use tokio::sync::mpsc;

use crate::{
    core::{
        CheckSummary, CoreConfig, DeleteDownloadSummary, DownloadBatchSummary, DownloadProgress,
        EpisodePreview, EpisodeRecord, EpisodeStatus, FeedCheckSummary, FeedPreview,
        FeedSubscription, LibraryStats, PodcastError, PodcastSearchResult, Result,
        RetentionSummary,
    },
    db::Db,
    decoder, discovery,
    downloads::{DownloadJob, cleanup_stale_temp_files, download_job_with_progress},
    feeds::{ParsedEpisode, ParsedFeed, parse_feed, sort_newest_first},
    retention,
};

#[derive(Debug, Clone)]
pub struct PodcastApp {
    config: Arc<CoreConfig>,
    db: Db,
    client: reqwest::Client,
    media_client: reqwest::Client,
}

impl PodcastApp {
    pub async fn open(config: CoreConfig) -> Result<Self> {
        config.validate()?;
        tokio::fs::create_dir_all(&config.download_dir).await?;
        let stale_temp_files_removed = cleanup_stale_temp_files(&config.download_dir).await?;
        let db = Db::open(&config.database_path).await?;
        db.insert_config_defaults(config.default_retention_limit)
            .await?;
        let client = reqwest::Client::builder()
            .timeout(config.http_timeout)
            .user_agent(config.user_agent.clone())
            .build()?;
        let media_client = reqwest::Client::builder()
            .user_agent(config.user_agent.clone())
            .build()?;
        let app = Self {
            config: Arc::new(config),
            db,
            client,
            media_client,
        };
        log::info!(
            "event=app.open {}",
            format_args!(
                "database={} download_dir={} stale_temp_files_removed={} max_feed_concurrency={} max_download_concurrency={} ensure_mp3={}",
                app.config.database_path.display(),
                app.config.download_dir.display(),
                stale_temp_files_removed,
                app.config.max_concurrent_feed_fetches,
                app.config.max_concurrent_downloads,
                app.config.ensure_mp3
            )
        );
        Ok(app)
    }

    pub async fn search_podcasts(&self, query: &str) -> Result<Vec<PodcastSearchResult>> {
        let started = Instant::now();
        log::info!(
            "event=search.start {}",
            format_args!("query={} country={}", log_value(query), self.config.country)
        );
        let result = discovery::search_apple(
            &self.client,
            &self.config.apple_search_base_url,
            &self.config.country,
            query,
        )
        .await;
        match &result {
            Ok(results) => {
                log::info!(
                    "event=search.finish {}",
                    format_args!(
                        "query={} results={} elapsed_ms={}",
                        log_value(query),
                        results.len(),
                        elapsed_ms(started.elapsed())
                    )
                );
            }
            Err(error) => {
                log::error!(
                    "event=search.error {}",
                    format_args!(
                        "query={} error={} elapsed_ms={}",
                        log_value(query),
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
            }
        }
        result
    }

    pub async fn preview_feed(&self, feed_url: &str) -> Result<FeedPreview> {
        let parsed = self.fetch_and_parse(feed_url).await?;
        Ok(FeedPreview {
            feed_url: feed_url.to_string(),
            raw_title: parsed.raw_title,
            normalized_title: parsed.normalized_title,
            site_url: parsed.site_url,
            description: parsed.description,
            artwork_url: parsed.artwork_url,
            episodes: parsed
                .episodes
                .into_iter()
                .map(|episode| EpisodePreview {
                    episode_key: episode.episode_key,
                    raw_title: episode.raw_title,
                    normalized_title: episode.normalized_title,
                    raw_author: episode.raw_author,
                    published_at: episode.published_at,
                    media_url: episode.media_url,
                    media_content_type: episode.media_content_type,
                    media_length_bytes: episode.media_length_bytes,
                })
                .collect(),
        })
    }

    pub async fn audio_encoder_status(&self) -> crate::core::AudioEncoderStatus {
        let status = decoder::encoder_status(&self.config).await;
        match &status {
            crate::core::AudioEncoderStatus::Available { path, version } => {
                log::info!(
                    "event=encoder.available {}",
                    format_args!("path={} version={}", path.display(), log_value(version))
                );
            }
            crate::core::AudioEncoderStatus::Missing { path } => {
                log::warn!(
                    "event=encoder.missing {}",
                    format_args!("path={}", path.display())
                );
            }
            crate::core::AudioEncoderStatus::Error { path, error } => {
                log::error!(
                    "event=encoder.error {}",
                    format_args!("path={} error={}", path.display(), log_value(error))
                );
            }
        }
        status
    }

    pub async fn add_feed(&self, feed_url: &str) -> Result<FeedSubscription> {
        let started = Instant::now();
        log::info!("event=feed.add.start {}", format_args!("url={feed_url}"));
        if self.db.feed_by_url(feed_url).await?.is_some() {
            log::warn!(
                "event=feed.add.duplicate {}",
                format_args!("url={feed_url}")
            );
            return Err(PodcastError::DuplicateFeed(feed_url.to_string()));
        }
        let parsed = self.fetch_and_parse(feed_url).await?;
        if parsed.episodes.is_empty() {
            log::warn!(
                "event=feed.add.no_downloadable_episodes {}",
                format_args!("url={feed_url}")
            );
            return Err(PodcastError::NoDownloadableEpisodes(feed_url.to_string()));
        }
        let feed = match self.db.insert_feed(feed_url, &parsed).await {
            Ok(feed) => feed,
            Err(error) => {
                log::error!(
                    "event=feed.add.error {}",
                    format_args!(
                        "url={feed_url} stage=db_insert error={} elapsed_ms={}",
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                return Err(error);
            }
        };
        log::info!(
            "event=feed.add.finish {}",
            format_args!(
                "feed_id={} title={} episodes={} elapsed_ms={}",
                feed.id,
                log_value(&feed.normalized_title),
                parsed.episodes.len(),
                elapsed_ms(started.elapsed())
            )
        );
        Ok(feed)
    }

    pub async fn remove_feed(&self, feed_id: &str) -> Result<()> {
        let started = Instant::now();
        log::warn!(
            "event=feed.remove.start {}",
            format_args!("feed_id={feed_id}")
        );
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        let episodes = self.db.episodes_with_files_for_feed(feed_id).await?;
        let mut files_deleted = 0;
        for episode in &episodes {
            if self.delete_episode_file(episode).await? {
                files_deleted += 1;
            }
        }
        let result = self.db.remove_feed(feed_id).await;
        match &result {
            Ok(()) => {
                log::warn!(
                    "event=feed.remove.finish {}",
                    format_args!(
                        "feed_id={feed_id} title={} files_deleted={} elapsed_ms={}",
                        log_value(&feed.normalized_title),
                        files_deleted,
                        elapsed_ms(started.elapsed())
                    )
                );
            }
            Err(error) => {
                log::error!(
                    "event=feed.remove.error {}",
                    format_args!(
                        "feed_id={feed_id} error={} elapsed_ms={}",
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
            }
        }
        result
    }

    pub async fn list_feeds(&self) -> Result<Vec<FeedSubscription>> {
        self.db.list_feeds().await
    }

    pub async fn library_stats(&self) -> Result<LibraryStats> {
        self.db.library_stats().await
    }

    pub fn max_concurrent_downloads(&self) -> usize {
        self.config.max_concurrent_downloads
    }

    pub async fn list_episodes(&self, feed_id: &str) -> Result<Vec<EpisodeRecord>> {
        self.db.episodes_for_feed(feed_id).await
    }

    pub async fn delete_downloaded_episode(
        &self,
        episode_id: &str,
    ) -> Result<DeleteDownloadSummary> {
        let started = Instant::now();
        log::warn!(
            "event=episode.delete_download.start {}",
            format_args!("episode_id={episode_id}")
        );
        let episode = self
            .db
            .episode_by_id(episode_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(episode_id.to_string()))?;

        let mut summary = DeleteDownloadSummary {
            requested: 1,
            ..DeleteDownloadSummary::default()
        };

        if episode.status != EpisodeStatus::Downloaded {
            log::info!(
                "event=episode.delete_download.skip {}",
                format_args!(
                    "episode_id={episode_id} status={} elapsed_ms={}",
                    episode.status.as_str(),
                    elapsed_ms(started.elapsed())
                )
            );
            return Ok(summary);
        }

        if self.delete_episode_file(&episode).await? {
            summary.files_deleted = 1;
        }
        self.db.mark_episode_deleted(episode_id).await?;
        summary.deleted = 1;
        log::warn!(
            "event=episode.delete_download.finish {}",
            format_args!(
                "episode_id={episode_id} files_deleted={} elapsed_ms={}",
                summary.files_deleted,
                elapsed_ms(started.elapsed())
            )
        );
        Ok(summary)
    }

    pub async fn refresh_feed_metadata(&self, feed_id: &str) -> Result<FeedSubscription> {
        let started = Instant::now();
        log::info!(
            "event=feed.refresh.start {}",
            format_args!("feed_id={feed_id}")
        );
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        let parsed = self.fetch_and_parse(&feed.feed_url).await?;
        if let Err(error) = self.db.update_feed_metadata(feed_id, &parsed).await {
            log::error!(
                "event=feed.refresh.error {}",
                format_args!(
                    "feed_id={feed_id} stage=db_update error={} elapsed_ms={}",
                    log_value(&error.to_string()),
                    elapsed_ms(started.elapsed())
                )
            );
            return Err(error);
        }
        let refreshed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        log::info!(
            "event=feed.refresh.finish {}",
            format_args!(
                "feed_id={feed_id} title={} episodes={} elapsed_ms={}",
                log_value(&refreshed.normalized_title),
                parsed.episodes.len(),
                elapsed_ms(started.elapsed())
            )
        );
        Ok(refreshed)
    }

    pub async fn set_feed_retention(
        &self,
        feed_id: &str,
        retention_limit: Option<u32>,
    ) -> Result<()> {
        log::info!(
            "event=retention.set {}",
            format_args!("feed_id={feed_id} retention_limit={retention_limit:?}")
        );
        let result = self.db.set_feed_retention(feed_id, retention_limit).await;
        if let Err(error) = &result {
            log::error!(
                "event=retention.set.error {}",
                format_args!(
                    "feed_id={feed_id} retention_limit={retention_limit:?} error={}",
                    log_value(&error.to_string())
                )
            );
        }
        result
    }

    pub async fn check_feed(&self, feed_id: &str) -> Result<FeedCheckSummary> {
        self.check_feed_with_progress(feed_id, None).await
    }

    pub async fn check_feed_with_progress(
        &self,
        feed_id: &str,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) -> Result<FeedCheckSummary> {
        let started = Instant::now();
        log::info!(
            "event=check_feed.start {}",
            format_args!("feed_id={feed_id}")
        );
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        let (mut summary, jobs) = self.prepare_feed_check(feed, progress.clone()).await;
        self.process_download_jobs(&mut summary, jobs, progress)
            .await;
        match self.enforce_retention(feed_id).await {
            Ok(retention) => {
                summary.deleted_by_retention += retention.files_deleted;
                summary.errors.extend(retention.errors);
            }
            Err(error) => {
                let error = error.to_string();
                log::warn!(
                    "event=retention.error {}",
                    format_args!("feed_id={feed_id} error={}", log_value(&error))
                );
                summary.errors.push(error);
            }
        }
        if !summary.errors.is_empty() {
            log::warn!(
                "event=check_feed.warning {}",
                format_args!(
                    "feed_id={feed_id} errors={} first_error={}",
                    summary.errors.len(),
                    log_value(summary.errors.first().map(String::as_str).unwrap_or(""))
                )
            );
        }
        log::info!(
            "event=check_feed.finish {}",
            format_args!(
                "feed_id={feed_id} queued={} downloaded={} failed={} deleted_by_retention={} elapsed_ms={}",
                summary.queued,
                summary.downloaded,
                summary.failed,
                summary.deleted_by_retention,
                elapsed_ms(started.elapsed())
            )
        );
        Ok(summary)
    }

    pub async fn download_episode_with_progress(
        &self,
        episode_id: &str,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) -> Result<FeedCheckSummary> {
        let mut batch = self
            .download_episodes_with_progress(vec![episode_id.to_string()], progress)
            .await?;
        if let Some(summary) = batch.feed_summaries.pop() {
            return Ok(summary);
        }
        Err(PodcastError::NotFound(episode_id.to_string()))
    }

    pub async fn download_episodes_with_progress(
        &self,
        episode_ids: Vec<String>,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) -> Result<DownloadBatchSummary> {
        let started = Instant::now();
        log::info!(
            "event=manual_download_batch.start {}",
            format_args!("requested={}", episode_ids.len())
        );
        let mut batch = DownloadBatchSummary {
            requested: episode_ids.len(),
            ..DownloadBatchSummary::default()
        };
        let mut seen = HashSet::new();
        let mut feed_summaries: HashMap<String, FeedCheckSummary> = HashMap::new();
        let mut jobs = Vec::new();

        for episode_id in episode_ids {
            if !seen.insert(episode_id.clone()) {
                batch.failed += 1;
                batch
                    .errors
                    .push(format!("duplicate episode id requested: {episode_id}"));
                continue;
            }

            let episode = match self.db.episode_by_id(&episode_id).await? {
                Some(episode) => episode,
                None => {
                    batch.failed += 1;
                    batch.errors.push(format!("not found: {episode_id}"));
                    continue;
                }
            };
            let feed = match self.db.feed_by_id(&episode.feed_id).await? {
                Some(feed) => feed,
                None => {
                    batch.failed += 1;
                    batch.errors.push(format!("not found: {}", episode.feed_id));
                    continue;
                }
            };

            let entry = feed_summaries
                .entry(feed.id.clone())
                .or_insert_with(|| FeedCheckSummary {
                    feed_id: feed.id.clone(),
                    feed_title: feed.normalized_title.clone(),
                    ..FeedCheckSummary::default()
                });
            entry.queued += 1;
            batch.queued += 1;

            let job = DownloadJob {
                feed_id: feed.id.clone(),
                feed_title: feed.normalized_title.clone(),
                episode,
            };
            emit_progress(
                progress.as_ref(),
                DownloadProgress::DownloadQueued {
                    feed_id: job.feed_id.clone(),
                    episode_id: job.episode.id.clone(),
                    episode_title: job.episode.normalized_title.clone(),
                },
            );
            jobs.push(job);
        }

        let results = stream::iter(jobs)
            .map(|job| self.download_job_for_summary(job, progress.clone()))
            .buffer_unordered(self.config.max_concurrent_downloads)
            .collect::<Vec<_>>()
            .await;

        for result in results {
            match result {
                Ok(feed_id) => {
                    batch.downloaded += 1;
                    if let Some(summary) = feed_summaries.get_mut(&feed_id) {
                        summary.downloaded += 1;
                    }
                }
                Err((feed_id, error)) => {
                    batch.failed += 1;
                    batch.errors.push(error.clone());
                    if let Some(summary) = feed_summaries.get_mut(&feed_id) {
                        summary.failed += 1;
                        summary.errors.push(error);
                    }
                }
            }
        }

        batch.feed_summaries = feed_summaries.into_values().collect();
        batch
            .feed_summaries
            .sort_by(|left, right| left.feed_title.cmp(&right.feed_title));

        if !batch.errors.is_empty() {
            log::warn!(
                "event=manual_download_batch.warning {}",
                format_args!(
                    "requested={} queued={} downloaded={} failed={} first_error={}",
                    batch.requested,
                    batch.queued,
                    batch.downloaded,
                    batch.failed,
                    log_value(batch.errors.first().map(String::as_str).unwrap_or(""))
                )
            );
        }
        log::info!(
            "event=manual_download_batch.finish {}",
            format_args!(
                "requested={} queued={} downloaded={} failed={} elapsed_ms={}",
                batch.requested,
                batch.queued,
                batch.downloaded,
                batch.failed,
                elapsed_ms(started.elapsed())
            )
        );
        Ok(batch)
    }

    pub async fn check_all(&self) -> Result<CheckSummary> {
        self.check_all_with_progress(None).await
    }

    pub async fn check_all_with_progress(
        &self,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) -> Result<CheckSummary> {
        let started = Instant::now();
        let feeds = self.list_feeds().await?;
        let feed_count = feeds.len();
        log::info!(
            "event=check_all.start {}",
            format_args!(
                "feeds={feed_count} max_feed_concurrency={} max_download_concurrency={}",
                self.config.max_concurrent_feed_fetches, self.config.max_concurrent_downloads
            )
        );
        let prepared = stream::iter(feeds)
            .map(|feed| self.prepare_feed_check(feed, progress.clone()))
            .buffer_unordered(self.config.max_concurrent_feed_fetches)
            .collect::<Vec<_>>()
            .await;

        let mut summary = CheckSummary {
            feeds_checked: feed_count,
            ..CheckSummary::default()
        };
        let mut jobs = Vec::new();

        for (feed_summary, feed_jobs) in prepared {
            summary.discovered += feed_summary.discovered;
            summary.queued += feed_summary.queued;
            summary.skipped_initial += feed_summary.skipped_initial;
            summary.failed += feed_summary.failed;
            summary.errors.extend(feed_summary.errors.clone());
            summary.feed_summaries.push(feed_summary);
            jobs.extend(feed_jobs);
        }

        let results = stream::iter(jobs)
            .map(|job| self.download_job_for_summary(job, progress.clone()))
            .buffer_unordered(self.config.max_concurrent_downloads)
            .collect::<Vec<_>>()
            .await;

        let mut affected_feed_ids = Vec::new();
        for result in results {
            match result {
                Ok(feed_id) => {
                    summary.downloaded += 1;
                    if let Some(feed_summary) = summary
                        .feed_summaries
                        .iter_mut()
                        .find(|item| item.feed_id == feed_id)
                    {
                        feed_summary.downloaded += 1;
                    }
                    if !affected_feed_ids.contains(&feed_id) {
                        affected_feed_ids.push(feed_id);
                    }
                }
                Err((feed_id, error)) => {
                    summary.failed += 1;
                    summary.errors.push(error.clone());
                    if let Some(feed_summary) = summary
                        .feed_summaries
                        .iter_mut()
                        .find(|item| item.feed_id == feed_id)
                    {
                        feed_summary.failed += 1;
                        feed_summary.errors.push(error);
                    }
                    if !affected_feed_ids.contains(&feed_id) {
                        affected_feed_ids.push(feed_id);
                    }
                }
            }
        }

        for feed_id in affected_feed_ids {
            match self.enforce_retention(&feed_id).await {
                Ok(retention) => {
                    summary.deleted_by_retention += retention.files_deleted;
                    if let Some(feed_summary) = summary
                        .feed_summaries
                        .iter_mut()
                        .find(|item| item.feed_id == feed_id)
                    {
                        feed_summary.deleted_by_retention += retention.files_deleted;
                        feed_summary.errors.extend(retention.errors.clone());
                    }
                    summary.errors.extend(retention.errors);
                }
                Err(error) => {
                    let error = error.to_string();
                    log::warn!(
                        "event=retention.error {}",
                        format_args!("feed_id={feed_id} error={}", log_value(&error))
                    );
                    summary.errors.push(error);
                }
            }
        }

        if !summary.errors.is_empty() {
            log::warn!(
                "event=check_all.warning {}",
                format_args!(
                    "errors={} first_error={}",
                    summary.errors.len(),
                    log_value(summary.errors.first().map(String::as_str).unwrap_or(""))
                )
            );
        }
        log::info!(
            "event=check_all.finish {}",
            format_args!(
                "feeds={} queued={} downloaded={} failed={} deleted_by_retention={} errors={} elapsed_ms={}",
                summary.feeds_checked,
                summary.queued,
                summary.downloaded,
                summary.failed,
                summary.deleted_by_retention,
                summary.errors.len(),
                elapsed_ms(started.elapsed())
            )
        );
        Ok(summary)
    }

    pub async fn enforce_retention(&self, feed_id: &str) -> Result<RetentionSummary> {
        let started = Instant::now();
        log::info!(
            "event=retention.start {}",
            format_args!("feed_id={feed_id}")
        );
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        let summary =
            retention::enforce_feed_retention(&self.db, &feed, self.config.default_retention_limit)
                .await?;
        if !summary.errors.is_empty() {
            log::warn!(
                "event=retention.warning {}",
                format_args!(
                    "feed_id={feed_id} files_deleted={} errors={} first_error={} elapsed_ms={}",
                    summary.files_deleted,
                    summary.errors.len(),
                    log_value(summary.errors.first().map(String::as_str).unwrap_or("")),
                    elapsed_ms(started.elapsed())
                )
            );
        } else {
            log::info!(
                "event=retention.finish {}",
                format_args!(
                    "feed_id={feed_id} files_deleted={} elapsed_ms={}",
                    summary.files_deleted,
                    elapsed_ms(started.elapsed())
                )
            );
        }
        Ok(summary)
    }

    pub async fn enforce_all_retention(&self) -> Result<RetentionSummary> {
        let started = Instant::now();
        let feeds = self.list_feeds().await?;
        log::info!(
            "event=retention_all.start {}",
            format_args!("feeds={}", feeds.len())
        );
        let mut summary = RetentionSummary::default();
        for feed in feeds {
            let result = retention::enforce_feed_retention(
                &self.db,
                &feed,
                self.config.default_retention_limit,
            )
            .await?;
            summary.feeds_checked += result.feeds_checked;
            summary.files_deleted += result.files_deleted;
            summary.errors.extend(result.errors);
        }
        if !summary.errors.is_empty() {
            log::warn!(
                "event=retention_all.warning {}",
                format_args!(
                    "feeds_checked={} files_deleted={} errors={} first_error={} elapsed_ms={}",
                    summary.feeds_checked,
                    summary.files_deleted,
                    summary.errors.len(),
                    log_value(summary.errors.first().map(String::as_str).unwrap_or("")),
                    elapsed_ms(started.elapsed())
                )
            );
        } else {
            log::info!(
                "event=retention_all.finish {}",
                format_args!(
                    "feeds_checked={} files_deleted={} elapsed_ms={}",
                    summary.feeds_checked,
                    summary.files_deleted,
                    elapsed_ms(started.elapsed())
                )
            );
        }
        Ok(summary)
    }

    async fn delete_episode_file(&self, episode: &EpisodeRecord) -> Result<bool> {
        let Some(file_path) = episode.file_path.as_deref() else {
            return Ok(false);
        };
        let stored_path = PathBuf::from(file_path);
        let path = if stored_path.is_absolute() {
            stored_path
        } else {
            self.config.download_dir.join(stored_path)
        };

        let canonical_file = match tokio::fs::canonicalize(&path).await {
            Ok(path) => path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(PodcastError::Io(error)),
        };
        let canonical_download_dir = tokio::fs::canonicalize(&self.config.download_dir).await?;
        if !canonical_file.starts_with(&canonical_download_dir) {
            return Err(PodcastError::InvalidConfig(format!(
                "refusing to delete file outside download directory: {}",
                canonical_file.display()
            )));
        }

        tokio::fs::remove_file(&canonical_file).await?;
        Ok(true)
    }

    async fn fetch_and_parse(&self, feed_url: &str) -> Result<ParsedFeed> {
        let started = Instant::now();
        log::info!("event=feed.fetch.start {}", format_args!("url={feed_url}"));
        let response = match self.client.get(feed_url).send().await {
            Ok(response) => response,
            Err(error) => {
                log::error!(
                    "event=feed.fetch.error {}",
                    format_args!(
                        "url={feed_url} stage=send error={} elapsed_ms={}",
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                return Err(error.into());
            }
        };
        let response = match response.error_for_status() {
            Ok(response) => response,
            Err(error) => {
                log::error!(
                    "event=feed.fetch.error {}",
                    format_args!(
                        "url={feed_url} stage=status error={} elapsed_ms={}",
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                return Err(error.into());
            }
        };
        let body = match response.bytes().await {
            Ok(body) => body,
            Err(error) => {
                log::error!(
                    "event=feed.fetch.error {}",
                    format_args!(
                        "url={feed_url} stage=body error={} elapsed_ms={}",
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                return Err(error.into());
            }
        };
        let parsed = match parse_feed(&body) {
            Ok(parsed) => parsed,
            Err(error) => {
                log::error!(
                    "event=feed.parse.error {}",
                    format_args!(
                        "url={feed_url} bytes={} error={} elapsed_ms={}",
                        body.len(),
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                return Err(error);
            }
        };
        log::info!(
            "event=feed.fetch.finish {}",
            format_args!(
                "url={feed_url} bytes={} episodes={} elapsed_ms={}",
                body.len(),
                parsed.episodes.len(),
                elapsed_ms(started.elapsed())
            )
        );
        Ok(parsed)
    }

    async fn prepare_feed_check(
        &self,
        feed: FeedSubscription,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) -> (FeedCheckSummary, Vec<DownloadJob>) {
        let started = Instant::now();
        let mut summary = FeedCheckSummary {
            feed_id: feed.id.clone(),
            feed_title: feed.normalized_title.clone(),
            ..FeedCheckSummary::default()
        };
        log::info!(
            "event=feed.prepare.start {}",
            format_args!(
                "feed_id={} title={}",
                feed.id,
                log_value(&feed.normalized_title)
            )
        );
        emit_progress(
            progress.as_ref(),
            DownloadProgress::FeedStarted {
                feed_id: feed.id.clone(),
                feed_title: feed.normalized_title.clone(),
            },
        );

        let parsed = match self.fetch_and_parse(&feed.feed_url).await {
            Ok(parsed) => parsed,
            Err(error) => {
                summary.failed += 1;
                summary.errors.push(error.to_string());
                log::error!(
                    "event=feed.prepare.error {}",
                    format_args!(
                        "feed_id={} error={} elapsed_ms={}",
                        feed.id,
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                emit_feed_finished(progress.as_ref(), &summary);
                return (summary, Vec::new());
            }
        };

        summary.discovered = parsed.episodes.len();
        if let Err(error) = self.db.update_feed_metadata(&feed.id, &parsed).await {
            summary.failed += 1;
            summary.errors.push(error.to_string());
            log::error!(
                "event=feed.prepare.error {}",
                format_args!(
                    "feed_id={} error={} elapsed_ms={}",
                    feed.id,
                    log_value(&error.to_string()),
                    elapsed_ms(started.elapsed())
                )
            );
            emit_feed_finished(progress.as_ref(), &summary);
            return (summary, Vec::new());
        }

        let first_run = match self.db.count_episodes(&feed.id).await {
            Ok(count) => count == 0,
            Err(error) => {
                summary.failed += 1;
                summary.errors.push(error.to_string());
                log::error!(
                    "event=feed.prepare.error {}",
                    format_args!(
                        "feed_id={} error={} elapsed_ms={}",
                        feed.id,
                        log_value(&error.to_string()),
                        elapsed_ms(started.elapsed())
                    )
                );
                emit_feed_finished(progress.as_ref(), &summary);
                return (summary, Vec::new());
            }
        };

        let mut episodes = parsed.episodes;
        sort_newest_first(&mut episodes);
        let jobs = if first_run {
            self.prepare_first_run_jobs(&feed, &episodes, &mut summary)
                .await
        } else {
            self.prepare_existing_feed_jobs(&feed, &episodes, &mut summary)
                .await
        };

        if let Err(error) = self.db.mark_feed_checked(&feed.id).await {
            log::warn!(
                "event=feed.mark_checked.error {}",
                format_args!(
                    "feed_id={} error={} elapsed_ms={}",
                    feed.id,
                    log_value(&error.to_string()),
                    elapsed_ms(started.elapsed())
                )
            );
            summary.errors.push(error.to_string());
        }
        for job in &jobs {
            emit_progress(
                progress.as_ref(),
                DownloadProgress::DownloadQueued {
                    feed_id: job.feed_id.clone(),
                    episode_id: job.episode.id.clone(),
                    episode_title: job.episode.normalized_title.clone(),
                },
            );
        }
        if !summary.errors.is_empty() {
            log::warn!(
                "event=feed.prepare.warning {}",
                format_args!(
                    "feed_id={} errors={} first_error={}",
                    feed.id,
                    summary.errors.len(),
                    log_value(summary.errors.first().map(String::as_str).unwrap_or(""))
                )
            );
        }
        log::info!(
            "event=feed.prepare.finish {}",
            format_args!(
                "feed_id={} discovered={} queued={} skipped_initial={} failed={} elapsed_ms={}",
                feed.id,
                summary.discovered,
                summary.queued,
                summary.skipped_initial,
                summary.failed,
                elapsed_ms(started.elapsed())
            )
        );
        emit_feed_finished(progress.as_ref(), &summary);
        (summary, jobs)
    }

    async fn prepare_first_run_jobs(
        &self,
        feed: &FeedSubscription,
        episodes: &[ParsedEpisode],
        summary: &mut FeedCheckSummary,
    ) -> Vec<DownloadJob> {
        let mut jobs = Vec::new();
        let initial_download_limit =
            feed.retention_limit
                .unwrap_or(self.config.default_retention_limit) as usize;
        for (index, episode) in episodes.iter().enumerate() {
            let should_download = index < initial_download_limit;
            let status = if should_download {
                EpisodeStatus::Pending
            } else {
                EpisodeStatus::SkippedInitial
            };
            match self.db.insert_episode(&feed.id, episode, status).await {
                Ok(record) if should_download => {
                    summary.queued += 1;
                    jobs.push(DownloadJob {
                        feed_id: feed.id.clone(),
                        feed_title: feed.normalized_title.clone(),
                        episode: record,
                    });
                }
                Ok(_) => summary.skipped_initial += 1,
                Err(error) => {
                    summary.failed += 1;
                    summary.errors.push(error.to_string());
                }
            }
        }
        jobs
    }

    async fn prepare_existing_feed_jobs(
        &self,
        feed: &FeedSubscription,
        episodes: &[ParsedEpisode],
        summary: &mut FeedCheckSummary,
    ) -> Vec<DownloadJob> {
        let mut jobs = Vec::new();
        for episode in episodes {
            match self.db.episode_by_key(&feed.id, &episode.episode_key).await {
                Ok(Some(record))
                    if matches!(
                        record.status,
                        EpisodeStatus::Failed | EpisodeStatus::Pending
                    ) =>
                {
                    summary.queued += 1;
                    jobs.push(DownloadJob {
                        feed_id: feed.id.clone(),
                        feed_title: feed.normalized_title.clone(),
                        episode: record,
                    });
                }
                Ok(Some(_)) => {}
                Ok(None) => match self
                    .db
                    .insert_episode(&feed.id, episode, EpisodeStatus::Pending)
                    .await
                {
                    Ok(record) => {
                        summary.queued += 1;
                        jobs.push(DownloadJob {
                            feed_id: feed.id.clone(),
                            feed_title: feed.normalized_title.clone(),
                            episode: record,
                        });
                    }
                    Err(error) => {
                        summary.failed += 1;
                        summary.errors.push(error.to_string());
                    }
                },
                Err(error) => {
                    summary.failed += 1;
                    summary.errors.push(error.to_string());
                }
            }
        }
        jobs
    }

    async fn process_download_jobs(
        &self,
        summary: &mut FeedCheckSummary,
        jobs: Vec<DownloadJob>,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) {
        log::info!(
            "event=download_queue.start {}",
            format_args!(
                "feed_id={} jobs={} max_download_concurrency={}",
                summary.feed_id,
                jobs.len(),
                self.config.max_concurrent_downloads
            )
        );
        let results = stream::iter(jobs)
            .map(|job| self.download_job_for_summary(job, progress.clone()))
            .buffer_unordered(self.config.max_concurrent_downloads)
            .collect::<Vec<_>>()
            .await;

        for result in results {
            match result {
                Ok(_) => summary.downloaded += 1,
                Err((_, error)) => {
                    summary.failed += 1;
                    summary.errors.push(error);
                }
            }
        }
        if summary.failed > 0 {
            log::warn!(
                "event=download_queue.warning {}",
                format_args!(
                    "feed_id={} downloaded={} failed={} first_error={}",
                    summary.feed_id,
                    summary.downloaded,
                    summary.failed,
                    log_value(summary.errors.first().map(String::as_str).unwrap_or(""))
                )
            );
        } else {
            log::info!(
                "event=download_queue.finish {}",
                format_args!(
                    "feed_id={} downloaded={} failed={}",
                    summary.feed_id, summary.downloaded, summary.failed
                )
            );
        }
    }

    async fn download_job_for_summary(
        &self,
        job: DownloadJob,
        progress: Option<mpsc::UnboundedSender<DownloadProgress>>,
    ) -> std::result::Result<String, (String, String)> {
        let started = Instant::now();
        let feed_id = job.feed_id.clone();
        let episode_id = job.episode.id.clone();
        log::info!(
            "event=download.start {}",
            format_args!(
                "feed_id={} episode_id={} title={}",
                feed_id,
                episode_id,
                log_value(&job.episode.normalized_title)
            )
        );
        match download_job_with_progress(&self.config, &self.db, &self.media_client, &job, progress)
            .await
        {
            Ok(_) => {
                log::info!(
                    "event=download.finish {}",
                    format_args!(
                        "feed_id={} episode_id={} elapsed_ms={}",
                        feed_id,
                        episode_id,
                        elapsed_ms(started.elapsed())
                    )
                );
                Ok(feed_id)
            }
            Err(error) => {
                let error = error.to_string();
                log::error!(
                    "event=download.error {}",
                    format_args!(
                        "feed_id={} episode_id={} error={} elapsed_ms={}",
                        feed_id,
                        episode_id,
                        log_value(&error),
                        elapsed_ms(started.elapsed())
                    )
                );
                Err((feed_id, error))
            }
        }
    }
}

fn emit_feed_finished(
    progress: Option<&mpsc::UnboundedSender<DownloadProgress>>,
    summary: &FeedCheckSummary,
) {
    emit_progress(
        progress,
        DownloadProgress::FeedFinished {
            feed_id: summary.feed_id.clone(),
            feed_title: summary.feed_title.clone(),
            queued: summary.queued,
            downloaded: summary.downloaded,
            failed: summary.failed,
        },
    );
}

fn emit_progress(
    progress: Option<&mpsc::UnboundedSender<DownloadProgress>>,
    event: DownloadProgress,
) {
    if let Some(progress) = progress {
        let _ = progress.send(event);
    }
}

fn elapsed_ms(duration: Duration) -> u128 {
    duration.as_millis()
}

fn log_value(value: &str) -> String {
    format!("{value:?}")
}
