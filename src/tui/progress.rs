use std::collections::HashMap;

use crate::core::DownloadProgress;

#[derive(Debug, Default)]
pub(super) struct ProgressState {
    pub(super) label: String,
    syncing_feeds: usize,
    downloads: HashMap<String, DownloadTaskProgress>,
    conversions: HashMap<String, String>,
    queued: usize,
    started: usize,
    finished: usize,
    failed: usize,
    tick: usize,
}

#[derive(Debug, Clone)]
pub(super) struct DownloadTaskProgress {
    pub(super) title: String,
    pub(super) downloaded_bytes: u64,
    pub(super) total_bytes: Option<u64>,
}

impl ProgressState {
    pub(super) fn apply(&mut self, event: DownloadProgress) {
        match event {
            DownloadProgress::FeedStarted { feed_title, .. } => {
                self.syncing_feeds = self.syncing_feeds.saturating_add(1);
                self.label = format!("Syncing {feed_title}");
            }
            DownloadProgress::FeedFinished {
                feed_title,
                queued,
                failed,
                ..
            } => {
                self.syncing_feeds = self.syncing_feeds.saturating_sub(1);
                self.failed = self.failed.saturating_add(failed);
                self.label = format!("Synced {feed_title}: {queued} queued, {failed} failed");
            }
            DownloadProgress::DownloadQueued { episode_title, .. } => {
                self.queued = self.queued.saturating_add(1);
                self.label = format!("Queued {episode_title}");
            }
            DownloadProgress::DownloadStarted {
                episode_id,
                episode_title,
                total_bytes,
                ..
            } => {
                self.started = self.started.saturating_add(1);
                self.downloads.insert(
                    episode_id,
                    DownloadTaskProgress {
                        title: episode_title.clone(),
                        downloaded_bytes: 0,
                        total_bytes,
                    },
                );
                self.label = format!("Downloading {episode_title}");
            }
            DownloadProgress::DownloadAdvanced {
                episode_id,
                episode_title,
                downloaded_bytes,
                total_bytes,
                ..
            } => {
                self.downloads
                    .entry(episode_id)
                    .and_modify(|task| {
                        task.downloaded_bytes = downloaded_bytes;
                        task.total_bytes = total_bytes;
                        task.title = episode_title.clone();
                    })
                    .or_insert_with(|| DownloadTaskProgress {
                        title: episode_title.clone(),
                        downloaded_bytes,
                        total_bytes,
                    });
                self.label = format!("Downloading {episode_title}");
            }
            DownloadProgress::DownloadFinished {
                episode_id,
                episode_title,
                ..
            } => {
                self.downloads.remove(&episode_id);
                self.finished = self.finished.saturating_add(1);
                self.label = format!("Downloaded {episode_title}");
            }
            DownloadProgress::ConversionStarted {
                episode_id,
                episode_title,
                ..
            } => {
                self.downloads.remove(&episode_id);
                self.conversions.insert(episode_id, episode_title.clone());
                self.label = format!("Converting {episode_title}");
            }
            DownloadProgress::ConversionFinished {
                episode_id,
                episode_title,
                ..
            } => {
                self.conversions.remove(&episode_id);
                self.label = format!("Converted {episode_title}");
            }
            DownloadProgress::DownloadFailed {
                episode_id,
                episode_title,
                error,
                ..
            } => {
                self.downloads.remove(&episode_id);
                self.conversions.remove(&episode_id);
                self.failed = self.failed.saturating_add(1);
                self.label = format!("{episode_title} failed: {error}");
            }
        }
    }

    pub(super) fn overall_ratio(&self) -> f64 {
        if self.queued == 0 {
            return 0.0;
        }
        let completed = self.finished.saturating_add(self.failed).min(self.queued);
        (completed as f64 / self.queued as f64).clamp(0.0, 1.0)
    }

    pub(super) fn active_downloads(&self) -> usize {
        self.downloads.len()
    }

    pub(super) fn active_conversions(&self) -> usize {
        self.conversions.len()
    }

    pub(super) fn queued_pending(&self) -> usize {
        self.queued.saturating_sub(self.started)
    }

    pub(super) fn completed(&self) -> usize {
        self.finished.saturating_add(self.failed)
    }

    pub(super) fn failed(&self) -> usize {
        self.failed
    }

    pub(super) fn queued(&self) -> usize {
        self.queued
    }

    pub(super) fn syncing_feeds(&self) -> usize {
        self.syncing_feeds
    }

    pub(super) fn current_download_bytes(&self) -> Option<(u64, u64)> {
        self.downloads.values().next().and_then(|task| {
            task.total_bytes
                .filter(|total| *total > 0)
                .map(|total| (task.downloaded_bytes, total))
        })
    }

    pub(super) fn current_download(&self) -> Option<&DownloadTaskProgress> {
        self.downloads.values().next()
    }

    pub(super) fn current_conversion_title(&self) -> Option<&str> {
        self.conversions.values().next().map(String::as_str)
    }

    pub(super) fn current_download_ratio(&self) -> f64 {
        self.current_download_bytes()
            .map(|(downloaded, total)| downloaded as f64 / total as f64)
            .unwrap_or_else(|| if self.downloads.is_empty() { 0.0 } else { 0.12 })
            .clamp(0.0, 1.0)
    }

    pub(super) fn tick(&mut self) {
        self.tick = self.tick.wrapping_add(1);
    }

    pub(super) fn monitor_label<'a>(&'a self, fallback: &'a str) -> &'a str {
        let fallback = if self.label.is_empty() {
            fallback
        } else {
            self.label.as_str()
        };
        self.downloads
            .values()
            .next()
            .map(|task| task.title.as_str())
            .or_else(|| self.conversions.values().next().map(String::as_str))
            .unwrap_or(fallback)
    }

    pub(super) fn spinner(&self) -> &'static str {
        match self.tick % 4 {
            0 => "|",
            1 => "/",
            2 => "-",
            _ => "\\",
        }
    }
}
