pub mod app;
pub mod config_file;
pub mod core;
pub mod logging;
pub mod tui;

mod db;
mod decoder;
mod discovery;
mod downloads;
mod feeds;
mod metadata;
mod retention;

pub use app::PodcastApp;
pub use core::{
    AppErrorDto, AudioEncoderStatus, CheckSummary, CoreConfig, DownloadBatchSummary,
    DownloadFailure, DownloadProgress, EpisodePreview, EpisodeRecord, EpisodeStatus,
    FeedCheckSummary, FeedPreview, FeedSubscription, LibraryStats, PodcastError,
    PodcastSearchResult, Result, RetentionSummary,
};
