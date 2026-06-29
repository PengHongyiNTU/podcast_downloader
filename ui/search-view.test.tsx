import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SearchView } from "./App";
import type { FeedPreview, PodcastSearchResult } from "./types";

describe("SearchView", () => {
  it("expands a result card with feed details and recent episodes", () => {
    const result: PodcastSearchResult = {
      title: "Example Podcast",
      author: "Example Author",
      feed_url: "https://example.test/feed.xml",
      artwork_url: null,
      apple_url: null,
    };
    const preview: FeedPreview = {
      feed_url: result.feed_url!,
      raw_title: "Example Podcast",
      normalized_title: "Example Podcast",
      site_url: null,
      description: "A compact podcast preview.",
      artwork_url: null as string | null,
      episodes: [
        {
          episode_key: "one",
          raw_title: "Episode One",
          normalized_title: "Episode One",
          raw_author: null,
          published_at: "2026-06-01T00:00:00Z",
          media_url: "https://example.test/one.mp3",
          media_content_type: "audio/mpeg",
          media_length_bytes: 1024,
        },
      ],
    };

    render(
      <SearchView
        query="example"
        results={[result]}
        busy={false}
        expandedResult={result.feed_url!}
        previews={{ [result.feed_url!]: preview }}
        previewLoading={null}
        onQuery={vi.fn()}
        onSubmit={vi.fn()}
        onSubscribe={vi.fn()}
        onTogglePreview={vi.fn()}
      />,
    );

    expect(screen.getByText("Example Podcast")).toBeTruthy();
    expect(screen.getByText("A compact podcast preview.")).toBeTruthy();
    expect(screen.getByText("Episode One")).toBeTruthy();
    expect(screen.getByRole("button", { name: /details/i })).toBeTruthy();
    expect(screen.getByRole("button", { name: /subscribe/i })).toBeTruthy();
  });
});
