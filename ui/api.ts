import { invoke } from "@tauri-apps/api/core";
import type {
  AudioEncoderStatus,
  DeleteDownloadSummary,
  DownloadedEpisode,
  EpisodeRecord,
  FeedPreview,
  FileConfig,
  InitialSnapshot,
  PodcastSearchResult,
  SaveSettingsResult,
  TaskAccepted,
} from "./types";

export const api = {
  initialSnapshot: () => invoke<InitialSnapshot>("get_initial_snapshot"),
  searchPodcasts: (query: string) =>
    invoke<PodcastSearchResult[]>("search_podcasts", { query }),
  previewFeed: (feedUrl: string) =>
    invoke<FeedPreview>("preview_feed", { feedUrl }),
  subscribeFeed: (feedUrl: string) =>
    invoke<TaskAccepted>("subscribe_feed", { feedUrl }),
  removeFeed: (feedId: string) => invoke<void>("remove_feed", { feedId }),
  checkFeed: (feedId: string) => invoke<TaskAccepted>("check_feed", { feedId }),
  checkAll: () => invoke<TaskAccepted>("check_all"),
  listEpisodes: (feedId: string) => invoke<EpisodeRecord[]>("list_episodes", { feedId }),
  listDownloadedEpisodes: () =>
    invoke<DownloadedEpisode[]>("list_downloaded_episodes"),
  downloadEpisodes: (episodeIds: string[]) =>
    invoke<TaskAccepted>("download_episodes", { episodeIds }),
  deleteDownloadedEpisode: (episodeId: string) =>
    invoke<DeleteDownloadSummary>("delete_downloaded_episode", { episodeId }),
  setFeedRetention: (feedId: string, retentionLimit: number | null) =>
    invoke<void>("set_feed_retention", { feedId, retentionLimit }),
  getSettings: () => invoke<FileConfig>("get_settings"),
  saveSettings: (config: FileConfig) =>
    invoke<SaveSettingsResult>("save_settings", { config }),
  detectFfmpeg: () => invoke<FileConfig>("detect_ffmpeg"),
  getAudioEncoderStatus: () =>
    invoke<AudioEncoderStatus>("get_audio_encoder_status"),
  openDownloadsFolder: () => invoke<void>("open_downloads_folder"),
};
