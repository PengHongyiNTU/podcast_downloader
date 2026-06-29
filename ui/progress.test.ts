import { describe, expect, it } from "vitest";
import { applyProgress, initialProgress, progressRatio, taskFinished, taskStarted } from "./progress";

describe("progress reducer", () => {
  it("tracks queue, current download, and completion", () => {
    let state = taskStarted(initialProgress, "download_episodes");
    state = applyProgress(state, {
      type: "download_queued",
      feed_id: "feed",
      episode_id: "ep",
      episode_title: "Episode",
    });
    state = applyProgress(state, {
      type: "download_advanced",
      feed_id: "feed",
      episode_id: "ep",
      episode_title: "Episode",
      downloaded_bytes: 50,
      total_bytes: 100,
    });

    expect(state.queueCount).toBe(1);
    expect(progressRatio(state.current)).toBe(0.5);

    state = applyProgress(state, {
      type: "download_finished",
      feed_id: "feed",
      episode_id: "ep",
      episode_title: "Episode",
      file_path: "Episode.mp3",
    });
    state = taskFinished(state);

    expect(state.queueCount).toBe(0);
    expect(state.doneCount).toBe(1);
    expect(state.activeTask).toBeUndefined();
  });
});
