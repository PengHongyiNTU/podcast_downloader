export type EpisodeStatus =
  | "pending"
  | "downloaded"
  | "skipped_initial"
  | "failed"
  | "deleted";

export type PodcastSearchResult = {
  title: string;
  author?: string | null;
  feed_url?: string | null;
  artwork_url?: string | null;
  apple_url?: string | null;
};

export type EpisodePreview = {
  episode_key: string;
  raw_title: string;
  normalized_title: string;
  raw_author?: string | null;
  published_at?: string | null;
  media_url: string;
  media_content_type?: string | null;
  media_length_bytes?: number | null;
};

export type FeedPreview = {
  feed_url: string;
  raw_title: string;
  normalized_title: string;
  site_url?: string | null;
  description?: string | null;
  artwork_url?: string | null;
  episodes: EpisodePreview[];
};

export type FeedSubscription = {
  id: string;
  feed_url: string;
  raw_title: string;
  normalized_title: string;
  site_url?: string | null;
  description?: string | null;
  artwork_url?: string | null;
  retention_limit?: number | null;
  created_at: string;
  updated_at: string;
  last_checked_at?: string | null;
};

export type EpisodeRecord = {
  id: string;
  feed_id: string;
  episode_key: string;
  raw_title: string;
  normalized_title: string;
  raw_author?: string | null;
  published_at?: string | null;
  media_url: string;
  media_content_type?: string | null;
  media_length_bytes?: number | null;
  status: EpisodeStatus;
  file_path?: string | null;
  first_seen_at: string;
  downloaded_at?: string | null;
  deleted_at?: string | null;
  last_error?: string | null;
};

export type LibraryStats = {
  feeds: number;
  episodes: number;
  downloaded: number;
};

export type DownloadedEpisode = {
  feed: FeedSubscription;
  episode: EpisodeRecord;
};

export type FileConfig = {
  database_path: string;
  download_dir: string;
  log_file_path: string;
  country: string;
  http_timeout_seconds: number;
  user_agent: string;
  default_retention_limit: number;
  max_concurrent_feed_fetches: number;
  max_concurrent_downloads: number;
  ensure_mp3: boolean;
  ffmpeg_path?: string | null;
  mp3_encoder_path?: string | null;
};

export type AudioEncoderStatus =
  | { Available: { path: string; version: string } }
  | { Missing: { path: string } }
  | { Error: { path: string; error: string } };

export type InitialSnapshot = {
  stats: LibraryStats;
  feeds: FeedSubscription[];
  downloaded_episodes: DownloadedEpisode[];
  settings: FileConfig;
  encoder_status: AudioEncoderStatus;
};

export type FeedCheckSummary = {
  feed_id: string;
  feed_title: string;
  discovered: number;
  queued: number;
  downloaded: number;
  skipped_initial: number;
  failed: number;
  deleted_by_retention: number;
  errors: string[];
};

export type CheckSummary = {
  feeds_checked: number;
  discovered: number;
  queued: number;
  downloaded: number;
  skipped_initial: number;
  failed: number;
  deleted_by_retention: number;
  feed_summaries: FeedCheckSummary[];
  errors: string[];
};

export type DownloadBatchSummary = {
  requested: number;
  queued: number;
  downloaded: number;
  failed: number;
  feed_summaries: FeedCheckSummary[];
  errors: string[];
};

export type DeleteDownloadSummary = {
  requested: number;
  deleted: number;
  files_deleted: number;
  errors: string[];
};

export type AppErrorDto = {
  kind: string;
  message: string;
};

export type SaveSettingsResult = {
  settings: FileConfig;
  stats: LibraryStats;
  encoder_status: AudioEncoderStatus;
};

export type DownloadProgress =
  | { type: "feed_started"; feed_id: string; feed_title: string }
  | {
      type: "feed_finished";
      feed_id: string;
      feed_title: string;
      queued: number;
      downloaded: number;
      failed: number;
    }
  | {
      type: "download_queued";
      feed_id: string;
      episode_id: string;
      episode_title: string;
    }
  | {
      type: "download_started";
      feed_id: string;
      episode_id: string;
      episode_title: string;
      total_bytes?: number | null;
    }
  | {
      type: "download_advanced";
      feed_id: string;
      episode_id: string;
      episode_title: string;
      downloaded_bytes: number;
      total_bytes?: number | null;
    }
  | {
      type: "conversion_started";
      feed_id: string;
      episode_id: string;
      episode_title: string;
    }
  | {
      type: "conversion_finished";
      feed_id: string;
      episode_id: string;
      episode_title: string;
    }
  | {
      type: "download_finished";
      feed_id: string;
      episode_id: string;
      episode_title: string;
      file_path: string;
    }
  | {
      type: "download_failed";
      feed_id: string;
      episode_id: string;
      episode_title: string;
      error: string;
    };

export type TaskStarted = {
  name: string;
};

export type TaskAccepted = {
  task_id: string;
  name: string;
  queued_position: number;
};

export type TaskFinished<T = unknown> = {
  name: string;
  result: T | AppErrorDto;
};
