use std::sync::Arc;

use futures::{StreamExt, stream};

use crate::{
    core::{
        CheckSummary, CoreConfig, EpisodeStatus, FeedCheckSummary, FeedSubscription, PodcastError,
        PodcastSearchResult, Result, RetentionSummary,
    },
    db::Db,
    decoder, discovery,
    downloads::{DownloadJob, download_job},
    feeds::{ParsedEpisode, ParsedFeed, parse_feed, sort_newest_first},
    retention,
};

#[derive(Debug, Clone)]
pub struct PodcastApp {
    config: Arc<CoreConfig>,
    db: Db,
    client: reqwest::Client,
}

impl PodcastApp {
    pub async fn open(config: CoreConfig) -> Result<Self> {
        config.validate()?;
        tokio::fs::create_dir_all(&config.download_dir).await?;
        let db = Db::open(&config.database_path).await?;
        db.insert_config_defaults(config.default_retention_limit)
            .await?;
        let client = reqwest::Client::builder()
            .timeout(config.http_timeout)
            .user_agent(config.user_agent.clone())
            .build()?;
        Ok(Self {
            config: Arc::new(config),
            db,
            client,
        })
    }

    pub async fn search_podcasts(&self, query: &str) -> Result<Vec<PodcastSearchResult>> {
        discovery::search_apple(
            &self.client,
            &self.config.apple_search_base_url,
            &self.config.country,
            query,
        )
        .await
    }

    pub async fn audio_encoder_status(&self) -> crate::core::AudioEncoderStatus {
        decoder::encoder_status(&self.config).await
    }

    pub async fn add_feed(&self, feed_url: &str) -> Result<FeedSubscription> {
        if self.db.feed_by_url(feed_url).await?.is_some() {
            return Err(PodcastError::DuplicateFeed(feed_url.to_string()));
        }
        let parsed = self.fetch_and_parse(feed_url).await?;
        if parsed.episodes.is_empty() {
            return Err(PodcastError::NoDownloadableEpisodes(feed_url.to_string()));
        }
        self.db.insert_feed(feed_url, &parsed).await
    }

    pub async fn remove_feed(&self, feed_id: &str) -> Result<()> {
        self.db.remove_feed(feed_id).await
    }

    pub async fn list_feeds(&self) -> Result<Vec<FeedSubscription>> {
        self.db.list_feeds().await
    }

    pub async fn refresh_feed_metadata(&self, feed_id: &str) -> Result<FeedSubscription> {
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        let parsed = self.fetch_and_parse(&feed.feed_url).await?;
        self.db.update_feed_metadata(feed_id, &parsed).await?;
        self.db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))
    }

    pub async fn set_feed_retention(
        &self,
        feed_id: &str,
        retention_limit: Option<u32>,
    ) -> Result<()> {
        self.db.set_feed_retention(feed_id, retention_limit).await
    }

    pub async fn check_feed(&self, feed_id: &str) -> Result<FeedCheckSummary> {
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        let (mut summary, jobs) = self.prepare_feed_check(feed).await;
        self.process_download_jobs(&mut summary, jobs).await;
        match self.enforce_retention(feed_id).await {
            Ok(retention) => {
                summary.deleted_by_retention += retention.files_deleted;
                summary.errors.extend(retention.errors);
            }
            Err(error) => summary.errors.push(error.to_string()),
        }
        Ok(summary)
    }

    pub async fn check_all(&self) -> Result<CheckSummary> {
        let feeds = self.list_feeds().await?;
        let feed_count = feeds.len();
        let prepared = stream::iter(feeds)
            .map(|feed| self.prepare_feed_check(feed))
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
            .map(|job| self.download_job_for_summary(job))
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
                Err(error) => summary.errors.push(error.to_string()),
            }
        }

        Ok(summary)
    }

    pub async fn enforce_retention(&self, feed_id: &str) -> Result<RetentionSummary> {
        let feed = self
            .db
            .feed_by_id(feed_id)
            .await?
            .ok_or_else(|| PodcastError::NotFound(feed_id.to_string()))?;
        retention::enforce_feed_retention(&self.db, &feed, self.config.default_retention_limit)
            .await
    }

    pub async fn enforce_all_retention(&self) -> Result<RetentionSummary> {
        let feeds = self.list_feeds().await?;
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
        Ok(summary)
    }

    async fn fetch_and_parse(&self, feed_url: &str) -> Result<ParsedFeed> {
        let body = self
            .client
            .get(feed_url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;
        parse_feed(&body)
    }

    async fn prepare_feed_check(
        &self,
        feed: FeedSubscription,
    ) -> (FeedCheckSummary, Vec<DownloadJob>) {
        let mut summary = FeedCheckSummary {
            feed_id: feed.id.clone(),
            feed_title: feed.normalized_title.clone(),
            ..FeedCheckSummary::default()
        };

        let parsed = match self.fetch_and_parse(&feed.feed_url).await {
            Ok(parsed) => parsed,
            Err(error) => {
                summary.failed += 1;
                summary.errors.push(error.to_string());
                return (summary, Vec::new());
            }
        };

        summary.discovered = parsed.episodes.len();
        if let Err(error) = self.db.update_feed_metadata(&feed.id, &parsed).await {
            summary.failed += 1;
            summary.errors.push(error.to_string());
            return (summary, Vec::new());
        }

        let first_run = match self.db.count_episodes(&feed.id).await {
            Ok(count) => count == 0,
            Err(error) => {
                summary.failed += 1;
                summary.errors.push(error.to_string());
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
            summary.errors.push(error.to_string());
        }
        (summary, jobs)
    }

    async fn prepare_first_run_jobs(
        &self,
        feed: &FeedSubscription,
        episodes: &[ParsedEpisode],
        summary: &mut FeedCheckSummary,
    ) -> Vec<DownloadJob> {
        let mut jobs = Vec::new();
        for (index, episode) in episodes.iter().enumerate() {
            let status = if index == 0 {
                EpisodeStatus::Pending
            } else {
                EpisodeStatus::SkippedInitial
            };
            match self.db.insert_episode(&feed.id, episode, status).await {
                Ok(record) if index == 0 => {
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

    async fn process_download_jobs(&self, summary: &mut FeedCheckSummary, jobs: Vec<DownloadJob>) {
        let results = stream::iter(jobs)
            .map(|job| self.download_job_for_summary(job))
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
    }

    async fn download_job_for_summary(
        &self,
        job: DownloadJob,
    ) -> std::result::Result<String, (String, String)> {
        let feed_id = job.feed_id.clone();
        download_job(&self.config, &self.db, &self.client, &job)
            .await
            .map(|_| feed_id.clone())
            .map_err(|error| (feed_id, error.to_string()))
    }
}
