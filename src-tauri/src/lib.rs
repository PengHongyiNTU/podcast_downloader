use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

use podcast_downloader::{
    AppErrorDto, AudioEncoderStatus, DeleteDownloadSummary, DownloadProgress, EpisodeRecord,
    EpisodeStatus, FeedPreview, FeedSubscription, LibraryStats, PodcastApp, PodcastSearchResult,
    config_file::FileConfig, logging,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock, mpsc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitialSnapshot {
    pub stats: LibraryStats,
    pub feeds: Vec<FeedSubscription>,
    pub downloaded_episodes: Vec<DownloadedEpisode>,
    pub settings: FileConfig,
    pub encoder_status: AudioEncoderStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedEpisode {
    pub feed: FeedSubscription,
    pub episode: EpisodeRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStarted {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFinished<T> {
    pub name: String,
    pub result: Result<T, AppErrorDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAccepted {
    pub task_id: String,
    pub name: String,
    pub queued_position: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveSettingsResult {
    pub settings: FileConfig,
    pub stats: LibraryStats,
    pub encoder_status: AudioEncoderStatus,
}

#[derive(Debug)]
enum BackgroundTask {
    CheckFeed { id: String, feed_id: String },
    CheckAll { id: String },
    DownloadEpisodes { id: String, episode_ids: Vec<String> },
}

impl BackgroundTask {
    fn id(&self) -> &str {
        match self {
            Self::CheckFeed { id, .. }
            | Self::CheckAll { id }
            | Self::DownloadEpisodes { id, .. } => id,
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::CheckFeed { .. } => "check_feed",
            Self::CheckAll { .. } => "check_all",
            Self::DownloadEpisodes { .. } => "download_episodes",
        }
    }
}

#[derive(Debug, Default)]
struct TaskQueueState {
    active: Option<String>,
    queued: VecDeque<BackgroundTask>,
    worker_running: bool,
}

impl TaskQueueState {
    fn enqueue(&mut self, task: BackgroundTask) -> (TaskAccepted, bool) {
        let should_start_worker = !self.worker_running;
        self.worker_running = true;
        let accepted = TaskAccepted {
            task_id: task.id().to_string(),
            name: task.name().to_string(),
            queued_position: self.queued.len() + 1,
        };
        self.queued.push_back(task);
        (accepted, should_start_worker)
    }

    fn is_busy(&self) -> bool {
        self.worker_running || self.active.is_some() || !self.queued.is_empty()
    }
}

pub struct DesktopState {
    app: RwLock<PodcastApp>,
    config: Mutex<FileConfig>,
    task_queue: Mutex<TaskQueueState>,
    base_dir: PathBuf,
    config_path: PathBuf,
}

impl DesktopState {
    async fn new(base_dir: PathBuf, config_path: PathBuf) -> podcast_downloader::Result<Self> {
        let mut file_config = FileConfig::load_or_create(&config_path).await?;
        file_config.set_detected_ffmpeg_path();
        file_config.save(&config_path).await?;
        let core_config = file_config.clone().into_core_config(&base_dir);
        init_logger_once(core_config.log_file_path.as_deref())?;
        let app = PodcastApp::open(core_config).await?;
        Ok(Self {
            app: RwLock::new(app),
            config: Mutex::new(file_config),
            task_queue: Mutex::new(TaskQueueState::default()),
            base_dir,
            config_path,
        })
    }

    async fn app(&self) -> PodcastApp {
        self.app.read().await.clone()
    }

    async fn snapshot(&self) -> Result<InitialSnapshot, AppErrorDto> {
        let app = self.app().await;
        Ok(InitialSnapshot {
            stats: app.library_stats().await.map_err(AppErrorDto::from)?,
            feeds: app.list_feeds().await.map_err(AppErrorDto::from)?,
            downloaded_episodes: downloaded_episodes_from_app(&app)
                .await
                .map_err(AppErrorDto::from)?,
            settings: self.config.lock().await.clone(),
            encoder_status: app.audio_encoder_status().await,
        })
    }

    async fn enqueue_task(
        &self,
        app_handle: &AppHandle,
        task: BackgroundTask,
    ) -> Result<TaskAccepted, AppErrorDto> {
        let (accepted, should_start_worker) = self.task_queue.lock().await.enqueue(task);
        if should_start_worker {
            let worker_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                run_background_worker(worker_handle).await;
            });
        }
        Ok(accepted)
    }

    async fn ensure_no_background_task(&self, operation: &str) -> Result<(), AppErrorDto> {
        let queue = self.task_queue.lock().await;
        if queue.is_busy() {
            let active_name = queue
                .active
                .as_deref()
                .unwrap_or_else(|| queue.queued.front().map(BackgroundTask::name).unwrap_or("task"));
            return Err(AppErrorDto {
                kind: "task_active".to_string(),
                message: format!(
                    "{operation} cannot run while {active_name} is active"
                ),
            });
        }
        Ok(())
    }
}

#[tauri::command]
async fn get_initial_snapshot(
    state: State<'_, DesktopState>,
) -> Result<InitialSnapshot, AppErrorDto> {
    state.snapshot().await
}

#[tauri::command]
async fn search_podcasts(
    state: State<'_, DesktopState>,
    query: String,
) -> Result<Vec<PodcastSearchResult>, AppErrorDto> {
    state
        .app()
        .await
        .search_podcasts(&query)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn preview_feed(
    state: State<'_, DesktopState>,
    feed_url: String,
) -> Result<FeedPreview, AppErrorDto> {
    state
        .app()
        .await
        .preview_feed(&feed_url)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn subscribe_feed(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    feed_url: String,
) -> Result<TaskAccepted, AppErrorDto> {
    let app = state.app().await;
    let feed = app.add_feed(&feed_url).await.map_err(AppErrorDto::from)?;
    state
        .enqueue_task(
            &app_handle,
            BackgroundTask::CheckFeed {
                id: next_task_id("check_feed"),
                feed_id: feed.id,
            },
        )
        .await
}

#[tauri::command]
async fn remove_feed(state: State<'_, DesktopState>, feed_id: String) -> Result<(), AppErrorDto> {
    state.ensure_no_background_task("remove_feed").await?;
    state
        .app()
        .await
        .remove_feed(&feed_id)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn check_feed(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    feed_id: String,
) -> Result<TaskAccepted, AppErrorDto> {
    state
        .enqueue_task(
            &app_handle,
            BackgroundTask::CheckFeed {
                id: next_task_id("check_feed"),
                feed_id,
            },
        )
        .await
}

#[tauri::command]
async fn check_all(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<TaskAccepted, AppErrorDto> {
    state
        .enqueue_task(
            &app_handle,
            BackgroundTask::CheckAll {
                id: next_task_id("check_all"),
            },
        )
        .await
}

#[tauri::command]
async fn list_episodes(
    state: State<'_, DesktopState>,
    feed_id: String,
) -> Result<Vec<EpisodeRecord>, AppErrorDto> {
    state
        .app()
        .await
        .list_episodes(&feed_id)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn list_downloaded_episodes(
    state: State<'_, DesktopState>,
) -> Result<Vec<DownloadedEpisode>, AppErrorDto> {
    downloaded_episodes_from_app(&state.app().await)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn download_episodes(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
    episode_ids: Vec<String>,
) -> Result<TaskAccepted, AppErrorDto> {
    state
        .enqueue_task(
            &app_handle,
            BackgroundTask::DownloadEpisodes {
                id: next_task_id("download_episodes"),
                episode_ids,
            },
        )
        .await
}

#[tauri::command]
async fn delete_downloaded_episode(
    state: State<'_, DesktopState>,
    episode_id: String,
) -> Result<DeleteDownloadSummary, AppErrorDto> {
    state
        .ensure_no_background_task("delete_downloaded_episode")
        .await?;
    state
        .app()
        .await
        .delete_downloaded_episode(&episode_id)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn set_feed_retention(
    state: State<'_, DesktopState>,
    feed_id: String,
    retention_limit: Option<u32>,
) -> Result<(), AppErrorDto> {
    state
        .ensure_no_background_task("set_feed_retention")
        .await?;
    state
        .app()
        .await
        .set_feed_retention(&feed_id, retention_limit)
        .await
        .map_err(AppErrorDto::from)
}

#[tauri::command]
async fn get_settings(state: State<'_, DesktopState>) -> Result<FileConfig, AppErrorDto> {
    Ok(state.config.lock().await.clone())
}

#[tauri::command]
async fn save_settings(
    state: State<'_, DesktopState>,
    config: FileConfig,
) -> Result<SaveSettingsResult, AppErrorDto> {
    state.ensure_no_background_task("save_settings").await?;
    save_settings_inner(&state, config).await
}

#[tauri::command]
async fn detect_ffmpeg(state: State<'_, DesktopState>) -> Result<FileConfig, AppErrorDto> {
    let mut config = state.config.lock().await.clone();
    config.set_detected_ffmpeg_path();
    Ok(config)
}

#[tauri::command]
async fn get_audio_encoder_status(
    state: State<'_, DesktopState>,
) -> Result<AudioEncoderStatus, AppErrorDto> {
    Ok(state.app().await.audio_encoder_status().await)
}

#[tauri::command]
async fn open_downloads_folder(state: State<'_, DesktopState>) -> Result<(), AppErrorDto> {
    let config = state.config.lock().await.clone();
    let download_dir = resolve_config_path(&state.base_dir, &config.download_dir);
    tokio::fs::create_dir_all(&download_dir)
        .await
        .map_err(podcast_downloader::PodcastError::Io)
        .map_err(AppErrorDto::from)?;
    open_folder(&download_dir).map_err(AppErrorDto::from)
}

async fn run_background_worker(app_handle: AppHandle) {
    loop {
        let Some(state) = app_handle.try_state::<DesktopState>() else {
            return;
        };
        let task = {
            let mut queue = state.task_queue.lock().await;
            let Some(task) = queue.queued.pop_front() else {
                queue.active = None;
                queue.worker_running = false;
                return;
            };
            queue.active = Some(task.name().to_string());
            task
        };

        let task_name = task.name();
        emit_task_started(&app_handle, task_name);
        match task {
            BackgroundTask::CheckFeed { feed_id, .. } => {
                let app = state.app().await;
                let result = app
                    .check_feed_with_progress(&feed_id, Some(progress_sender(app_handle.clone())))
                    .await
                    .map_err(AppErrorDto::from);
                emit_task_finished(&app_handle, task_name, result);
            }
            BackgroundTask::CheckAll { .. } => {
                let app = state.app().await;
                let result = app
                    .check_all_with_progress(Some(progress_sender(app_handle.clone())))
                    .await
                    .map_err(AppErrorDto::from);
                emit_task_finished(&app_handle, task_name, result);
            }
            BackgroundTask::DownloadEpisodes { episode_ids, .. } => {
                let app = state.app().await;
                let result = app
                    .download_episodes_with_progress(
                        episode_ids,
                        Some(progress_sender(app_handle.clone())),
                    )
                    .await
                    .map_err(AppErrorDto::from);
                emit_task_finished(&app_handle, task_name, result);
            }
        }

        let mut queue = state.task_queue.lock().await;
        queue.active = None;
    }
}

async fn save_settings_inner(
    state: &DesktopState,
    config: FileConfig,
) -> Result<SaveSettingsResult, AppErrorDto> {
    config
        .save(&state.config_path)
        .await
        .map_err(AppErrorDto::from)?;
    let core_config = config.clone().into_core_config(&state.base_dir);
    let reopened = PodcastApp::open(core_config)
        .await
        .map_err(AppErrorDto::from)?;
    let encoder_status = reopened.audio_encoder_status().await;
    let stats = reopened.library_stats().await.map_err(AppErrorDto::from)?;
    *state.app.write().await = reopened;
    *state.config.lock().await = config.clone();
    Ok(SaveSettingsResult {
        settings: config,
        stats,
        encoder_status,
    })
}

async fn downloaded_episodes_from_app(
    app: &PodcastApp,
) -> podcast_downloader::Result<Vec<DownloadedEpisode>> {
    let mut items = Vec::new();
    for feed in app.list_feeds().await? {
        for episode in app.list_episodes(&feed.id).await? {
            if episode.status == EpisodeStatus::Downloaded {
                items.push(DownloadedEpisode {
                    feed: feed.clone(),
                    episode,
                });
            }
        }
    }
    items.sort_by(|left, right| {
        right
            .episode
            .published_at
            .cmp(&left.episode.published_at)
            .then_with(|| left.feed.normalized_title.cmp(&right.feed.normalized_title))
    });
    Ok(items)
}

fn progress_sender(app_handle: AppHandle) -> mpsc::UnboundedSender<DownloadProgress> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let _ = app_handle.emit("podcast-progress", event);
        }
    });
    tx
}

fn emit_task_started(app_handle: &AppHandle, name: &str) {
    let _ = app_handle.emit(
        "podcast-task-started",
        TaskStarted {
            name: name.to_string(),
        },
    );
}

fn emit_task_finished<T>(app_handle: &AppHandle, name: &str, result: Result<T, AppErrorDto>)
where
    T: Clone + Serialize,
{
    let _ = app_handle.emit(
        "podcast-task-finished",
        TaskFinished {
            name: name.to_string(),
            result,
        },
    );
}

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

fn next_task_id(name: &str) -> String {
    let id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);
    format!("{name}-{id}")
}

fn app_base_dir(app: &tauri::App) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn config_path(base_dir: &Path) -> PathBuf {
    base_dir.join("config.toml")
}

fn resolve_config_path(base_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn open_folder(path: &Path) -> podcast_downloader::Result<()> {
    let status = if cfg!(target_os = "windows") {
        Command::new("explorer").arg(path).status()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(path).status()
    } else {
        Command::new("xdg-open").arg(path).status()
    }?;

    if status.success() {
        Ok(())
    } else {
        Err(podcast_downloader::PodcastError::Io(std::io::Error::other(
            format!("open folder command failed with status {status}"),
        )))
    }
}

fn init_logger_once(log_file_path: Option<&Path>) -> podcast_downloader::Result<()> {
    match logging::init_file_logger(log_file_path) {
        Ok(()) => Ok(()),
        Err(error)
            if error.to_string().contains(
                "attempted to set a logger after the logging system was already initialized",
            ) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let base_dir = app_base_dir(app);
            let config_path = config_path(&base_dir);
            let state = tauri::async_runtime::block_on(DesktopState::new(base_dir, config_path))?;
            app.manage(state);
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = handle.try_state::<DesktopState>() {
                    let _ = state
                        .enqueue_task(
                            &handle,
                            BackgroundTask::CheckAll {
                                id: next_task_id("check_all"),
                            },
                        )
                        .await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_initial_snapshot,
            search_podcasts,
            preview_feed,
            subscribe_feed,
            remove_feed,
            check_feed,
            check_all,
            list_episodes,
            list_downloaded_episodes,
            download_episodes,
            delete_downloaded_episode,
            set_feed_retention,
            get_settings,
            save_settings,
            detect_ffmpeg,
            get_audio_encoder_status,
            open_downloads_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_queue_accepts_follow_up_work() {
        let mut queue = TaskQueueState::default();

        let (first, starts_worker) = queue.enqueue(BackgroundTask::CheckAll {
            id: "check_all-1".to_string(),
        });
        let (second, starts_second_worker) = queue.enqueue(BackgroundTask::DownloadEpisodes {
            id: "download_episodes-2".to_string(),
            episode_ids: vec!["episode-1".to_string()],
        });

        assert!(starts_worker);
        assert!(!starts_second_worker);
        assert_eq!(first.name, "check_all");
        assert_eq!(first.queued_position, 1);
        assert_eq!(second.name, "download_episodes");
        assert_eq!(second.queued_position, 2);
        assert!(queue.is_busy());
    }

    #[tokio::test]
    async fn destructive_operations_are_guarded_while_background_task_is_busy() {
        let temp = tempfile::tempdir().unwrap();
        let state = DesktopState::new(temp.path().to_path_buf(), temp.path().join("config.toml"))
            .await
            .unwrap();

        {
            let mut queue = state.task_queue.lock().await;
            queue.worker_running = true;
            queue.active = Some("download_episodes".to_string());
        }

        let error = state
            .ensure_no_background_task("remove_feed")
            .await
            .unwrap_err();

        assert_eq!(error.kind, "task_active");
        assert!(error.message.contains("download_episodes"));
    }

    #[tokio::test]
    async fn settings_save_reopens_core() {
        let temp = tempfile::tempdir().unwrap();
        let state = DesktopState::new(temp.path().to_path_buf(), temp.path().join("config.toml"))
            .await
            .unwrap();
        let mut config = state.config.lock().await.clone();
        config.country = "GB".to_string();
        config.max_concurrent_downloads = 3;

        let result = save_settings_inner(&state, config.clone()).await.unwrap();

        assert_eq!(result.settings.country, "GB");
        assert_eq!(state.config.lock().await.max_concurrent_downloads, 3);
    }

    #[tokio::test]
    async fn initial_snapshot_loads_empty_library() {
        let temp = tempfile::tempdir().unwrap();
        let state = DesktopState::new(temp.path().to_path_buf(), temp.path().join("config.toml"))
            .await
            .unwrap();

        let snapshot = state.snapshot().await.unwrap();

        assert_eq!(snapshot.stats.feeds, 0);
        assert!(snapshot.feeds.is_empty());
        assert!(snapshot.downloaded_episodes.is_empty());
    }
}
