import { listen } from "@tauri-apps/api/event";
import {
  CheckCircle2,
  ChevronDown,
  Download,
  ExternalLink,
  FolderOpen,
  Home,
  Library,
  Loader2,
  Mic2,
  RefreshCw,
  Search,
  Settings,
  Trash2,
} from "lucide-react";
import { FormEvent, useEffect, useMemo, useState } from "react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardFooter,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { api } from "./api";
import {
  applyProgress,
  initialProgress,
  progressRatio,
  taskFinished,
  taskStarted,
  type ProgressModel,
} from "./progress";
import type {
  AppErrorDto,
  AudioEncoderStatus,
  DownloadProgress,
  DownloadedEpisode,
  EpisodePreview,
  EpisodeRecord,
  FeedPreview,
  FeedSubscription,
  FileConfig,
  InitialSnapshot,
  LibraryStats,
  PodcastSearchResult,
  TaskFinished,
  TaskStarted,
} from "./types";

type View = "home" | "watched" | "search" | "downloads" | "settings";
type EpisodeMap = Record<string, EpisodeRecord[]>;

const navItems: Array<{ id: View; label: string; icon: typeof Home }> = [
  { id: "home", label: "Home", icon: Home },
  { id: "watched", label: "Watched", icon: Library },
  { id: "search", label: "Search", icon: Search },
  { id: "downloads", label: "Downloads", icon: Download },
  { id: "settings", label: "Settings", icon: Settings },
];

const emptyStats: LibraryStats = { feeds: 0, episodes: 0, downloaded: 0 };

export function App() {
  const [view, setView] = useState<View>("home");
  const [stats, setStats] = useState<LibraryStats>(emptyStats);
  const [feeds, setFeeds] = useState<FeedSubscription[]>([]);
  const [episodes, setEpisodes] = useState<EpisodeMap>({});
  const [downloadedEpisodes, setDownloadedEpisodes] = useState<DownloadedEpisode[]>([]);
  const [settings, setSettings] = useState<FileConfig | null>(null);
  const [encoderStatus, setEncoderStatus] = useState<AudioEncoderStatus | null>(null);
  const [selectedFeedId, setSelectedFeedId] = useState<string | null>(null);
  const [searchResults, setSearchResults] = useState<PodcastSearchResult[]>([]);
  const [expandedResult, setExpandedResult] = useState<string | null>(null);
  const [previews, setPreviews] = useState<Record<string, FeedPreview>>({});
  const [previewLoading, setPreviewLoading] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [progress, setProgress] = useState<ProgressModel>(initialProgress);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("Loading library");

  const selectedFeed = feeds.find((feed) => feed.id === selectedFeedId) ?? feeds[0];
  const selectedEpisodes = selectedFeed ? episodes[selectedFeed.id] ?? [] : [];

  useEffect(() => {
    void loadSnapshot();

    const unsubscribers = [
      listen<DownloadProgress>("podcast-progress", (event) => {
        setProgress((current) => applyProgress(current, event.payload));
      }),
      listen<TaskStarted>("podcast-task-started", (event) => {
        setProgress((current) => taskStarted(current, event.payload.name));
      }),
      listen<TaskFinished>("podcast-task-finished", (event) => {
        setProgress((current) => taskFinished(current));
        if (isError(event.payload.result)) {
          setMessage(event.payload.result.message);
        } else {
          setMessage("Library updated");
          void loadSnapshot();
        }
      }),
    ];

    return () => {
      void Promise.all(unsubscribers).then((callbacks) => {
        callbacks.forEach((callback) => callback());
      });
    };
  }, []);

  async function loadSnapshot() {
    try {
      const snapshot = await api.initialSnapshot();
      applySnapshot(snapshot);
      if (!selectedFeedId && snapshot.feeds[0]) {
        setSelectedFeedId(snapshot.feeds[0].id);
      }
      await loadEpisodesForFeeds(snapshot.feeds);
      setMessage("Ready");
    } catch (error) {
      setMessage(errorMessage(error));
    }
  }

  function applySnapshot(snapshot: InitialSnapshot) {
    setStats(snapshot.stats);
    setFeeds(snapshot.feeds);
    setDownloadedEpisodes(snapshot.downloaded_episodes);
    setSettings(snapshot.settings);
    setEncoderStatus(snapshot.encoder_status);
  }

  async function loadEpisodesForFeeds(nextFeeds: FeedSubscription[]) {
    const entries = await Promise.all(
      nextFeeds.map(async (feed) => [feed.id, await api.listEpisodes(feed.id)] as const),
    );
    setEpisodes(Object.fromEntries(entries));
  }

  async function runAction<T>(action: () => Promise<T>, success: string) {
    setBusy(true);
    try {
      await action();
      setMessage(success);
      await loadSnapshot();
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function submitSearch(event: FormEvent) {
    event.preventDefault();
    if (!query.trim()) return;
    setBusy(true);
    setMessage("Searching Apple Podcasts");
    try {
      setSearchResults(await api.searchPodcasts(query.trim()));
      setExpandedResult(null);
      setPreviews({});
      setMessage("Search complete");
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setBusy(false);
    }
  }

  async function subscribe(result: PodcastSearchResult) {
    if (!result.feed_url) return;
    await runAction(async () => {
      await api.subscribeFeed(result.feed_url!);
      setView("watched");
    }, `Subscribed to ${result.title}`);
  }

  async function togglePreview(result: PodcastSearchResult) {
    const key = resultKey(result);
    if (expandedResult === key) {
      setExpandedResult(null);
      return;
    }
    setExpandedResult(key);
    if (!result.feed_url || previews[key]) return;
    setPreviewLoading(key);
    try {
      const preview = await api.previewFeed(result.feed_url);
      setPreviews((current) => ({ ...current, [key]: preview }));
    } catch (error) {
      setMessage(errorMessage(error));
    } finally {
      setPreviewLoading(null);
    }
  }

  async function downloadEpisode(episodeId: string) {
    await runAction(() => api.downloadEpisodes([episodeId]), "Download queued");
  }

  async function syncAll() {
    await runAction(() => api.checkAll(), "All feeds checked");
  }

  async function saveConfig(config: FileConfig) {
    await runAction(async () => {
      const result = await api.saveSettings(config);
      setSettings(result.settings);
      setStats(result.stats);
      setEncoderStatus(result.encoder_status);
    }, "Settings saved");
  }

  async function detectFfmpeg() {
    await runAction(async () => {
      setSettings(await api.detectFfmpeg());
    }, "FFmpeg detection complete");
  }

  async function openDownloadsFolder() {
    await runAction(() => api.openDownloadsFolder(), "Downloads folder opened");
  }

  return (
    <TooltipProvider>
      <div className="app-shell bg-background">
        <Sidebar stats={stats} view={view} onView={setView} />
        <main className="content-scroll">
          <div className="mx-auto flex min-h-full w-full max-w-[1480px] flex-col gap-6 px-6 py-6 lg:px-8">
            <TopBar
              title={titleForView(view, selectedFeed)}
              message={message}
              busy={busy}
              syncing={progress.activeTask === "check_all"}
              onSync={syncAll}
            />

            {view === "home" && (
              <HomeView
                stats={stats}
                feeds={feeds}
                downloads={downloadedEpisodes}
                onOpenFeed={(feedId) => {
                  setSelectedFeedId(feedId);
                  setView("watched");
                }}
              />
            )}
            {view === "watched" && (
              <WatchedView
                feeds={feeds}
                selectedFeed={selectedFeed}
                episodes={selectedEpisodes}
                defaultRetention={settings?.default_retention_limit ?? 4}
                onSelect={setSelectedFeedId}
                onCheck={(feedId) => runAction(() => api.checkFeed(feedId), "Feed checked")}
                onDownload={downloadEpisode}
                onRemove={(feedId) =>
                  runAction(async () => {
                    await api.removeFeed(feedId);
                    setSelectedFeedId(null);
                  }, "Podcast removed")
                }
                onRetention={(feedId, limit) =>
                  runAction(() => api.setFeedRetention(feedId, limit), "Retention updated")
                }
              />
            )}
            {view === "search" && (
              <SearchView
                query={query}
                results={searchResults}
                busy={busy}
                expandedResult={expandedResult}
                previews={previews}
                previewLoading={previewLoading}
                onQuery={setQuery}
                onSubmit={submitSearch}
                onSubscribe={subscribe}
                onTogglePreview={togglePreview}
              />
            )}
            {view === "downloads" && (
              <DownloadsView
                downloads={downloadedEpisodes}
                progress={progress}
                onOpenFolder={openDownloadsFolder}
              />
            )}
            {view === "settings" && settings && (
              <SettingsView
                settings={settings}
                encoderStatus={encoderStatus}
                onSave={saveConfig}
                onDetect={detectFfmpeg}
              />
            )}
          </div>
        </main>
        <MonitorBar progress={progress} compact />
      </div>
    </TooltipProvider>
  );
}

function Sidebar({
  stats,
  view,
  onView,
}: {
  stats: LibraryStats;
  view: View;
  onView: (view: View) => void;
}) {
  return (
    <aside className="row-span-2 flex min-w-0 flex-col gap-6 border-r bg-muted/45 p-4">
      <div className="flex items-center gap-3 px-2">
        <div className="flex size-10 items-center justify-center rounded-xl bg-primary text-primary-foreground shadow-sm">
          <Mic2 size={21} />
        </div>
        <div className="min-w-0 max-[980px]:hidden">
          <div className="truncate text-sm font-semibold">Podcasts</div>
          <div className="text-xs text-muted-foreground">{stats.feeds} watched</div>
        </div>
      </div>

      <nav className="grid gap-1">
        {navItems.map((item) => {
          const Icon = item.icon;
          return (
            <Tooltip key={item.id}>
              <TooltipTrigger asChild>
                <Button
                  type="button"
                  variant={view === item.id ? "secondary" : "ghost"}
                  className={cn(
                    "justify-start gap-3 px-3 max-[980px]:justify-center max-[980px]:px-0",
                    view === item.id && "text-primary",
                  )}
                  onClick={() => onView(item.id)}
                >
                  <Icon size={18} />
                  <span className="max-[980px]:hidden">{item.label}</span>
                </Button>
              </TooltipTrigger>
              <TooltipContent side="right" className="min-[981px]:hidden">
                {item.label}
              </TooltipContent>
            </Tooltip>
          );
        })}
      </nav>

      <Card className="mt-auto max-[980px]:hidden">
        <CardContent className="grid gap-1 p-4 text-sm">
          <span className="text-muted-foreground">{stats.episodes} episodes</span>
          <strong>{stats.downloaded} downloaded</strong>
        </CardContent>
      </Card>
    </aside>
  );
}

function TopBar({
  title,
  message,
  busy,
  syncing,
  onSync,
}: {
  title: string;
  message: string;
  busy: boolean;
  syncing: boolean;
  onSync: () => void;
}) {
  return (
    <header className="flex items-start justify-between gap-4 max-md:flex-col">
      <div className="min-w-0">
        <p className="text-sm text-muted-foreground">{message}</p>
        <h1 className="truncate text-3xl font-semibold tracking-tight md:text-4xl">{title}</h1>
      </div>
      <Button onClick={onSync} disabled={busy}>
        {syncing ? <Loader2 className="animate-spin" /> : <RefreshCw />}
        Sync All
      </Button>
    </header>
  );
}

function HomeView({
  stats,
  feeds,
  downloads,
  onOpenFeed,
}: {
  stats: LibraryStats;
  feeds: FeedSubscription[];
  downloads: DownloadedEpisode[];
  onOpenFeed: (feedId: string) => void;
}) {
  return (
    <div className="grid gap-6">
      <div className="grid gap-4 md:grid-cols-3">
        <Metric label="Watched" value={stats.feeds} />
        <Metric label="Episodes" value={stats.episodes} />
        <Metric label="Downloaded" value={stats.downloaded} />
      </div>
      <section className="grid gap-3">
        <SectionTitle title="Recently Watched" />
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {feeds.slice(0, 8).map((feed) => (
            <ShowCard key={feed.id} feed={feed} onClick={() => onOpenFeed(feed.id)} />
          ))}
        </div>
      </section>
      <section className="grid gap-3">
        <SectionTitle title="Latest Downloads" />
        <EpisodeList
          episodes={downloads.slice(0, 8).map((item) => item.episode)}
          feedsById={Object.fromEntries(downloads.map((item) => [item.feed.id, item.feed]))}
        />
      </section>
    </div>
  );
}

function WatchedView({
  feeds,
  selectedFeed,
  episodes,
  defaultRetention,
  onSelect,
  onCheck,
  onDownload,
  onRemove,
  onRetention,
}: {
  feeds: FeedSubscription[];
  selectedFeed?: FeedSubscription;
  episodes: EpisodeRecord[];
  defaultRetention: number;
  onSelect: (feedId: string) => void;
  onCheck: (feedId: string) => void;
  onDownload: (episodeId: string) => void;
  onRemove: (feedId: string) => void;
  onRetention: (feedId: string, retentionLimit: number | null) => void;
}) {
  if (!selectedFeed) {
    return <EmptyState title="No watched podcasts" text="Search for a podcast and subscribe to start a library." />;
  }

  return (
    <div className="grid min-h-0 gap-6 xl:grid-cols-[340px_minmax(0,1fr)]">
      <Card className="min-h-0">
        <CardHeader className="pb-3">
          <CardTitle>Watched Shows</CardTitle>
          <CardDescription>{feeds.length} subscriptions</CardDescription>
        </CardHeader>
        <CardContent className="p-0">
          <ScrollArea className="h-[min(680px,calc(100vh-260px))] px-3 pb-3">
            <div className="grid gap-2">
              {feeds.map((feed) => (
                <button
                  className={cn(
                    "flex items-center gap-3 rounded-lg p-2 text-left transition-colors hover:bg-accent",
                    feed.id === selectedFeed.id && "bg-accent text-accent-foreground",
                  )}
                  key={feed.id}
                  onClick={() => onSelect(feed.id)}
                  type="button"
                >
                  <PodcastArtwork title={feed.normalized_title} url={feed.artwork_url} size="sm" />
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium">{feed.normalized_title}</div>
                    <div className="truncate text-xs text-muted-foreground">
                      {feed.last_checked_at ? `Checked ${formatDate(feed.last_checked_at)}` : "Not checked yet"}
                    </div>
                  </div>
                </button>
              ))}
            </div>
          </ScrollArea>
        </CardContent>
      </Card>

      <div className="grid gap-4">
        <Card className="overflow-hidden">
          <CardContent className="grid gap-6 p-5 lg:grid-cols-[220px_minmax(0,1fr)]">
            <PodcastArtwork
              title={selectedFeed.normalized_title}
              url={selectedFeed.artwork_url}
              size="xl"
              className="mx-auto lg:mx-0"
            />
            <div className="grid min-w-0 content-start gap-4">
              <div className="min-w-0">
                <div className="mb-2 flex flex-wrap items-center gap-2">
                  <Badge variant="secondary">
                    {selectedFeed.last_checked_at ? `Checked ${formatDate(selectedFeed.last_checked_at)}` : "New"}
                  </Badge>
                  <Badge variant="outline">{episodes.length} episodes</Badge>
                </div>
                <h2 className="text-3xl font-semibold tracking-tight">{selectedFeed.normalized_title}</h2>
                <p className="mt-3 max-w-3xl text-sm leading-6 text-muted-foreground">
                  {selectedFeed.description ?? selectedFeed.feed_url}
                </p>
              </div>

              <div className="grid gap-2 rounded-lg border bg-muted/30 p-3 text-sm">
                {selectedFeed.site_url && (
                  <a
                    className="flex min-w-0 items-center gap-2 text-primary hover:underline"
                    href={selectedFeed.site_url}
                  >
                    <ExternalLink className="size-4 shrink-0" />
                    <span className="truncate">{selectedFeed.site_url}</span>
                  </a>
                )}
                <div className="truncate text-muted-foreground">{selectedFeed.feed_url}</div>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <Button onClick={() => onCheck(selectedFeed.id)}>
                  <RefreshCw />
                  Check Feed
                </Button>
                <Button variant="destructive" onClick={() => onRemove(selectedFeed.id)}>
                  <Trash2 />
                  Unwatch
                </Button>
                <label className="flex items-center gap-2 text-sm text-muted-foreground">
                  Keep
                  <Input
                    className="h-9 w-20"
                    type="number"
                    min={1}
                    defaultValue={selectedFeed.retention_limit ?? defaultRetention}
                    onBlur={(event) => onRetention(selectedFeed.id, Number(event.currentTarget.value))}
                  />
                </label>
              </div>
            </div>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <CardTitle>Episodes</CardTitle>
            <CardDescription>All known episodes from this watched show.</CardDescription>
          </CardHeader>
          <CardContent>
            <EpisodeList episodes={episodes} onDownload={onDownload} />
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

export function SearchView({
  query,
  results,
  busy,
  expandedResult,
  previews,
  previewLoading,
  onQuery,
  onSubmit,
  onSubscribe,
  onTogglePreview,
}: {
  query: string;
  results: PodcastSearchResult[];
  busy: boolean;
  expandedResult: string | null;
  previews: Record<string, FeedPreview>;
  previewLoading: string | null;
  onQuery: (query: string) => void;
  onSubmit: (event: FormEvent) => void;
  onSubscribe: (result: PodcastSearchResult) => void;
  onTogglePreview: (result: PodcastSearchResult) => void;
}) {
  return (
    <section className="grid gap-5">
      <form className="flex items-center gap-2 rounded-xl border bg-card p-2 shadow-sm" onSubmit={onSubmit}>
        <Search className="ml-2 size-5 text-muted-foreground" />
        <Input
          value={query}
          onChange={(event) => onQuery(event.currentTarget.value)}
          placeholder="Search Apple Podcasts"
          className="border-0 shadow-none focus-visible:ring-0"
        />
        <Button type="submit" disabled={busy} className="min-w-24">
          {busy ? <Loader2 className="animate-spin" /> : null}
          Search
        </Button>
      </form>

      <div className="grid gap-4 md:grid-cols-2 2xl:grid-cols-3">
        {results.map((result) => {
          const key = resultKey(result);
          const preview = previews[key];
          const expanded = expandedResult === key;
          return (
            <Card key={key} className={cn("overflow-hidden", expanded && "md:col-span-2 2xl:col-span-2")}>
              <CardContent className="grid h-full gap-4 p-4">
                <button
                  className="grid grid-cols-[112px_minmax(0,1fr)_auto] gap-4 text-left"
                  type="button"
                  onClick={() => onTogglePreview(result)}
                >
                  <PodcastArtwork title={result.title} url={result.artwork_url} size="lg" />
                  <div className="min-w-0">
                    <h3 className="line-clamp-2 min-h-12 text-base font-semibold leading-6">{result.title}</h3>
                    <p className="mt-1 truncate text-sm text-muted-foreground">{result.author ?? "Unknown author"}</p>
                    <Badge className="mt-3" variant={result.feed_url ? "secondary" : "outline"}>
                      {result.feed_url ? "RSS available" : "No RSS source"}
                    </Badge>
                  </div>
                  <ChevronDown className={cn("mt-1 size-4 text-muted-foreground transition-transform", expanded && "rotate-180")} />
                </button>

                <div className="mt-auto grid grid-cols-2 gap-2">
                  <Button
                    variant="secondary"
                    type="button"
                    disabled={!result.feed_url}
                    onClick={() => onTogglePreview(result)}
                  >
                    {previewLoading === key ? <Loader2 className="animate-spin" /> : null}
                    Details
                  </Button>
                  <Button type="button" disabled={!result.feed_url} onClick={() => onSubscribe(result)}>
                    Subscribe
                  </Button>
                </div>

                {expanded && <SearchPreview preview={preview} loading={previewLoading === key} />}
              </CardContent>
            </Card>
          );
        })}
      </div>
    </section>
  );
}

function SearchPreview({ preview, loading }: { preview?: FeedPreview; loading: boolean }) {
  if (loading) {
    return (
      <div className="grid gap-3 border-t pt-4">
        <Skeleton className="h-4 w-3/4" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-14 w-full" />
      </div>
    );
  }
  if (!preview) {
    return (
      <Alert className="border-dashed">
        <AlertTitle>Preview not loaded</AlertTitle>
        <AlertDescription>Open details to fetch feed metadata and recent episodes.</AlertDescription>
      </Alert>
    );
  }
  return (
    <div className="grid gap-4 border-t pt-4">
      <p className="line-clamp-3 text-sm leading-6 text-muted-foreground">
        {preview.description ?? preview.site_url ?? preview.feed_url}
      </p>
      <div className="grid gap-2">
        {preview.episodes.slice(0, 4).map((episode) => (
          <PreviewEpisodeRow episode={episode} key={episode.episode_key} />
        ))}
      </div>
    </div>
  );
}

function PreviewEpisodeRow({ episode }: { episode: EpisodePreview }) {
  return (
    <div className="rounded-lg bg-muted/50 p-3">
      <div className="line-clamp-1 text-sm font-medium">{episode.normalized_title}</div>
      <div className="mt-1 text-xs text-muted-foreground">
        {formatDate(episode.published_at)}
        {episode.media_length_bytes ? ` · ${formatBytes(episode.media_length_bytes)}` : ""}
      </div>
    </div>
  );
}

function DownloadsView({
  downloads,
  progress,
  onOpenFolder,
}: {
  downloads: DownloadedEpisode[];
  progress: ProgressModel;
  onOpenFolder: () => void;
}) {
  const feedsById = useMemo(
    () => Object.fromEntries(downloads.map((item) => [item.feed.id, item.feed])),
    [downloads],
  );
  return (
    <section className="grid gap-5">
      <div className="flex items-start justify-between gap-4 max-md:flex-col">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Downloaded Episodes</h2>
          <p className="text-sm text-muted-foreground">Downloaded files and active download work live here.</p>
        </div>
        <Button variant="secondary" type="button" onClick={onOpenFolder}>
          <FolderOpen />
          Open Folder
        </Button>
      </div>
      <MonitorBar progress={progress} />
      <Card>
        <CardContent className="p-4">
          <EpisodeList episodes={downloads.map((item) => item.episode)} feedsById={feedsById} />
        </CardContent>
      </Card>
    </section>
  );
}

function SettingsView({
  settings,
  encoderStatus,
  onSave,
  onDetect,
}: {
  settings: FileConfig;
  encoderStatus: AudioEncoderStatus | null;
  onSave: (config: FileConfig) => void;
  onDetect: () => void;
}) {
  const [draft, setDraft] = useState(settings);
  useEffect(() => setDraft(settings), [settings]);

  function update<K extends keyof FileConfig>(key: K, value: FileConfig[K]) {
    setDraft((current) => ({ ...current, [key]: value }));
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle>Settings</CardTitle>
        <CardDescription>These values are saved to the local config.toml used by the core.</CardDescription>
      </CardHeader>
      <CardContent>
        <form
          className="grid gap-5"
          onSubmit={(event) => {
            event.preventDefault();
            onSave(draft);
          }}
        >
          <div className="grid gap-4 md:grid-cols-2">
            <TextField label="Database" value={draft.database_path} onChange={(value) => update("database_path", value)} />
            <TextField label="Download Folder" value={draft.download_dir} onChange={(value) => update("download_dir", value)} />
            <TextField label="Log File" value={draft.log_file_path} onChange={(value) => update("log_file_path", value)} />
            <TextField label="Country" value={draft.country} onChange={(value) => update("country", value.toUpperCase())} />
            <NumberField label="Timeout" value={draft.http_timeout_seconds} onChange={(value) => update("http_timeout_seconds", value)} />
            <NumberField label="Default Keep" value={draft.default_retention_limit} onChange={(value) => update("default_retention_limit", value)} />
            <NumberField label="Feed Concurrency" value={draft.max_concurrent_feed_fetches} onChange={(value) => update("max_concurrent_feed_fetches", value)} />
            <NumberField label="Download Concurrency" value={draft.max_concurrent_downloads} onChange={(value) => update("max_concurrent_downloads", value)} />
            <TextField label="User Agent" value={draft.user_agent} onChange={(value) => update("user_agent", value)} />
            <TextField label="FFmpeg Path" value={draft.ffmpeg_path ?? ""} onChange={(value) => update("ffmpeg_path", value || null)} />
          </div>
          <label className="flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={draft.ensure_mp3}
              onChange={(event) => update("ensure_mp3", event.currentTarget.checked)}
            />
            Convert downloads to MP3
          </label>
          <div className="flex flex-wrap items-center gap-2">
            <Button type="submit">Save Settings</Button>
            <Button type="button" variant="secondary" onClick={onDetect}>
              Detect FFmpeg
            </Button>
            <Badge variant="outline">{encoderStatusText(encoderStatus)}</Badge>
          </div>
        </form>
      </CardContent>
    </Card>
  );
}

function MonitorBar({ progress, compact = false }: { progress: ProgressModel; compact?: boolean }) {
  const ratio = Math.round(progressRatio(progress.current) * 100);
  return (
    <Card className={cn(compact && "col-start-2 rounded-none border-x-0 border-b-0 max-[980px]:col-start-2")}>
      <CardContent className={cn("grid gap-3 p-4", compact ? "md:grid-cols-[1fr_110px_2fr]" : "md:grid-cols-[1fr_120px_2fr]")}>
        <div className="flex min-w-0 items-center gap-2 text-sm">
          {progress.syncingFeed ? <Loader2 className="size-4 animate-spin text-primary" /> : <CheckCircle2 className="size-4 text-emerald-500" />}
          <span className="truncate">{progress.syncingFeed ?? "Sync idle"}</span>
        </div>
        <Badge variant="secondary" className="w-fit">Queue {progress.queueCount}</Badge>
        <div className="grid min-w-0 gap-2">
          <div className="truncate text-sm text-muted-foreground">
            {progress.current
              ? `${progress.current.status} ${progress.current.title}`
              : `${progress.doneCount} done, ${progress.failedCount} failed`}
          </div>
          <Progress value={ratio} />
        </div>
      </CardContent>
    </Card>
  );
}

function EpisodeList({
  episodes,
  feedsById = {},
  onDownload,
}: {
  episodes: EpisodeRecord[];
  feedsById?: Record<string, FeedSubscription>;
  onDownload?: (episodeId: string) => void;
}) {
  if (!episodes.length) {
    return <EmptyState title="No episodes" text="Check the feed to discover available episodes." />;
  }
  return (
    <div className="grid gap-2">
      {episodes.map((episode) => (
        <EpisodeRow
          episode={episode}
          feed={feedsById[episode.feed_id]}
          key={episode.id}
          onDownload={onDownload}
        />
      ))}
    </div>
  );
}

function EpisodeRow({
  episode,
  feed,
  onDownload,
}: {
  episode: EpisodeRecord;
  feed?: FeedSubscription;
  onDownload?: (episodeId: string) => void;
}) {
  return (
    <div className="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-lg border bg-card p-3 md:grid-cols-[minmax(0,1fr)_auto_auto]">
      <div className="min-w-0">
        <div className="line-clamp-2 text-sm font-medium">{episode.normalized_title}</div>
        <div className="mt-1 truncate text-xs text-muted-foreground">
          {feed?.normalized_title ?? formatDate(episode.published_at)}
          {episode.media_length_bytes ? ` · ${formatBytes(episode.media_length_bytes)}` : ""}
        </div>
        {episode.last_error && <div className="mt-1 line-clamp-1 text-xs text-destructive">{episode.last_error}</div>}
      </div>
      <StatusBadge status={episode.status} />
      {onDownload && episode.status !== "downloaded" && episode.status !== "deleted" && (
        <Button type="button" size="icon" variant="secondary" onClick={() => onDownload(episode.id)}>
          <Download />
        </Button>
      )}
    </div>
  );
}

function StatusBadge({ status }: { status: EpisodeRecord["status"] }) {
  if (status === "downloaded") return <Badge variant="success">downloaded</Badge>;
  if (status === "failed") return <Badge variant="destructive">failed</Badge>;
  if (status === "deleted") return <Badge variant="outline">deleted</Badge>;
  return <Badge variant="secondary">{status.replace("_", " ")}</Badge>;
}

function Metric({ label, value }: { label: string; value: number }) {
  return (
    <Card>
      <CardContent className="p-5">
        <div className="text-sm text-muted-foreground">{label}</div>
        <div className="mt-2 text-3xl font-semibold">{value}</div>
      </CardContent>
    </Card>
  );
}

function SectionTitle({ title }: { title: string }) {
  return <h2 className="text-xl font-semibold tracking-tight">{title}</h2>;
}

function ShowCard({ feed, onClick }: { feed: FeedSubscription; onClick: () => void }) {
  return (
    <Card>
      <button className="grid w-full gap-3 p-4 text-left" onClick={onClick} type="button">
        <PodcastArtwork title={feed.normalized_title} url={feed.artwork_url} size="lg" />
        <div className="min-w-0">
          <div className="line-clamp-2 font-medium">{feed.normalized_title}</div>
          <div className="mt-1 text-sm text-muted-foreground">
            {feed.last_checked_at ? formatDate(feed.last_checked_at) : "New"}
          </div>
        </div>
      </button>
    </Card>
  );
}

function PodcastArtwork({
  title,
  url,
  size,
  className,
}: {
  title: string;
  url?: string | null;
  size: "sm" | "lg" | "xl";
  className?: string;
}) {
  const sizeClass = {
    sm: "size-11 rounded-md text-xs",
    lg: "size-28 rounded-xl text-lg",
    xl: "size-52 rounded-2xl text-3xl",
  }[size];
  return url ? (
    <img
      className={cn(sizeClass, "shrink-0 object-cover shadow-sm", className)}
      src={url}
      alt=""
    />
  ) : (
    <div
      className={cn(
        sizeClass,
        "grid shrink-0 place-items-center bg-gradient-to-br from-pink-200 to-orange-200 font-bold text-pink-800 shadow-sm",
        className,
      )}
    >
      {initials(title)}
    </div>
  );
}

function TextField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  return (
    <label className="grid gap-2 text-sm">
      <span className="font-medium">{label}</span>
      <Input value={value} onChange={(event) => onChange(event.currentTarget.value)} />
    </label>
  );
}

function NumberField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="grid gap-2 text-sm">
      <span className="font-medium">{label}</span>
      <Input
        min={1}
        type="number"
        value={value}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
      />
    </label>
  );
}

function EmptyState({ title, text }: { title: string; text: string }) {
  return (
    <Alert className="border-dashed">
      <AlertTitle>{title}</AlertTitle>
      <AlertDescription>{text}</AlertDescription>
    </Alert>
  );
}

function titleForView(view: View, feed?: FeedSubscription) {
  if (view === "watched" && feed) return feed.normalized_title;
  return {
    home: "Home",
    watched: "Watched",
    search: "Search",
    downloads: "Downloads",
    settings: "Settings",
  }[view];
}

function resultKey(result: PodcastSearchResult) {
  return result.feed_url ?? result.apple_url ?? result.title;
}

function isError(value: unknown): value is AppErrorDto {
  return Boolean(value && typeof value === "object" && "kind" in value && "message" in value);
}

function errorMessage(error: unknown) {
  if (isError(error)) return error.message;
  if (error instanceof Error) return error.message;
  return String(error);
}

function encoderStatusText(status: AudioEncoderStatus | null) {
  if (!status) return "Encoder unknown";
  if ("Available" in status) return `FFmpeg ready`;
  if ("Missing" in status) return `FFmpeg missing`;
  return `FFmpeg error`;
}

function formatDate(value?: string | null) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

function formatBytes(value: number) {
  if (value < 1024 * 1024) return `${Math.round(value / 1024)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function initials(value: string) {
  return value
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => word[0]?.toUpperCase())
    .join("");
}
