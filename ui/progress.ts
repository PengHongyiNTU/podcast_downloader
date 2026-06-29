import type { DownloadProgress } from "./types";

export type ProgressItem = {
  episodeId: string;
  title: string;
  downloadedBytes: number;
  totalBytes?: number | null;
  status: "queued" | "downloading" | "converting" | "done" | "failed";
  error?: string;
};

export type ProgressModel = {
  activeTask?: string;
  syncingFeed?: string;
  queueCount: number;
  doneCount: number;
  failedCount: number;
  current?: ProgressItem;
  items: Record<string, ProgressItem>;
};

export const initialProgress: ProgressModel = {
  queueCount: 0,
  doneCount: 0,
  failedCount: 0,
  items: {},
};

export function taskStarted(state: ProgressModel, name: string): ProgressModel {
  return {
    ...state,
    activeTask: name,
    queueCount: 0,
    doneCount: 0,
    failedCount: 0,
    current: undefined,
    syncingFeed: undefined,
    items: {},
  };
}

export function taskFinished(state: ProgressModel): ProgressModel {
  return {
    ...state,
    activeTask: undefined,
    syncingFeed: undefined,
    current: state.current?.status === "done" ? undefined : state.current,
  };
}

export function applyProgress(
  state: ProgressModel,
  event: DownloadProgress,
): ProgressModel {
  switch (event.type) {
    case "feed_started":
      return { ...state, syncingFeed: event.feed_title };
    case "feed_finished":
      return { ...state, syncingFeed: undefined, failedCount: state.failedCount + event.failed };
    case "download_queued": {
      const item: ProgressItem = {
        episodeId: event.episode_id,
        title: event.episode_title,
        downloadedBytes: 0,
        status: "queued",
      };
      return {
        ...state,
        queueCount: state.queueCount + 1,
        items: { ...state.items, [event.episode_id]: item },
      };
    }
    case "download_started": {
      const item: ProgressItem = {
        episodeId: event.episode_id,
        title: event.episode_title,
        downloadedBytes: 0,
        totalBytes: event.total_bytes,
        status: "downloading",
      };
      return {
        ...state,
        current: item,
        items: { ...state.items, [event.episode_id]: item },
      };
    }
    case "download_advanced": {
      const item: ProgressItem = {
        episodeId: event.episode_id,
        title: event.episode_title,
        downloadedBytes: event.downloaded_bytes,
        totalBytes: event.total_bytes,
        status: "downloading",
      };
      return {
        ...state,
        current: item,
        items: { ...state.items, [event.episode_id]: item },
      };
    }
    case "conversion_started": {
      const current = {
        episodeId: event.episode_id,
        title: event.episode_title,
        downloadedBytes: state.items[event.episode_id]?.downloadedBytes ?? 0,
        totalBytes: state.items[event.episode_id]?.totalBytes,
        status: "converting" as const,
      };
      return {
        ...state,
        current,
        items: { ...state.items, [event.episode_id]: current },
      };
    }
    case "conversion_finished":
      return state;
    case "download_finished": {
      const item = {
        episodeId: event.episode_id,
        title: event.episode_title,
        downloadedBytes: state.items[event.episode_id]?.downloadedBytes ?? 0,
        totalBytes: state.items[event.episode_id]?.totalBytes,
        status: "done" as const,
      };
      return {
        ...state,
        doneCount: state.doneCount + 1,
        queueCount: Math.max(0, state.queueCount - 1),
        current: state.current?.episodeId === event.episode_id ? undefined : state.current,
        items: { ...state.items, [event.episode_id]: item },
      };
    }
    case "download_failed": {
      const item = {
        episodeId: event.episode_id,
        title: event.episode_title,
        downloadedBytes: state.items[event.episode_id]?.downloadedBytes ?? 0,
        totalBytes: state.items[event.episode_id]?.totalBytes,
        status: "failed" as const,
        error: event.error,
      };
      return {
        ...state,
        failedCount: state.failedCount + 1,
        queueCount: Math.max(0, state.queueCount - 1),
        current: state.current?.episodeId === event.episode_id ? undefined : state.current,
        items: { ...state.items, [event.episode_id]: item },
      };
    }
  }
}

export function progressRatio(item?: ProgressItem): number {
  if (!item?.totalBytes || item.totalBytes <= 0) {
    return item?.status === "done" ? 1 : 0;
  }
  return Math.min(1, item.downloadedBytes / item.totalBytes);
}
