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

export function aggregateProgressRatio(state: ProgressModel): number {
  const items = Object.values(state.items);
  if (!items.length) {
    return 0;
  }

  const total = items.reduce((sum, item) => {
    if (item.status === "done" || item.status === "failed") {
      return sum + 1;
    }
    if (item.status === "converting") {
      return sum + 0.94;
    }
    if (item.status === "downloading") {
      return sum + progressRatio(item);
    }
    return sum;
  }, 0);

  return Math.min(1, total / items.length);
}

export function progressStatus(state: ProgressModel) {
  const items = Object.values(state.items);
  const total = items.length;
  const converting = items.filter((item) => item.status === "converting").length;
  const downloading = items.filter((item) => item.status === "downloading").length;
  const queued = items.filter((item) => item.status === "queued").length;
  const completed = items.filter((item) => item.status === "done" || item.status === "failed").length;

  if (state.syncingFeed) {
    return {
      kind: "syncing" as const,
      label: "Syncing",
      detail: state.syncingFeed,
      total,
      completed,
      converting,
      downloading,
      queued,
    };
  }
  if (converting > 0) {
    return {
      kind: "converting" as const,
      label: "Converting",
      detail: `${converting} file${converting === 1 ? "" : "s"}`,
      total,
      completed,
      converting,
      downloading,
      queued,
    };
  }
  if (downloading > 0) {
    return {
      kind: "downloading" as const,
      label: "Downloading",
      detail: `${completed}/${total} complete`,
      total,
      completed,
      converting,
      downloading,
      queued,
    };
  }
  if (queued > 0 || state.activeTask) {
    return {
      kind: "working" as const,
      label: "Working",
      detail: total ? `${completed}/${total} complete` : "Preparing",
      total,
      completed,
      converting,
      downloading,
      queued,
    };
  }
  if (state.failedCount > 0) {
    return {
      kind: "failed" as const,
      label: "Finished with errors",
      detail: `${state.failedCount} failed`,
      total,
      completed,
      converting,
      downloading,
      queued,
    };
  }
  if (state.doneCount > 0) {
    return {
      kind: "complete" as const,
      label: "Complete",
      detail: `${state.doneCount} done`,
      total,
      completed,
      converting,
      downloading,
      queued,
    };
  }
  return {
    kind: "idle" as const,
    label: "Ready",
    detail: "No active work",
    total,
    completed,
    converting,
    downloading,
    queued,
  };
}
