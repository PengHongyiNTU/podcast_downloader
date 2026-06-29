use std::{
    path::{Path, PathBuf},
    process::Command,
};

use podcast_downloader::{
    AppErrorDto, AudioEncoderStatus, CheckSummary, DownloadBatchSummary, DownloadProgress,
    EpisodeRecord, EpisodeStatus, FeedCheckSummary, FeedPreview, FeedSubscription, LibraryStats,
    PodcastApp, PodcastSearchResult, config_file::FileConfig, logging,
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
pub struct SaveSettingsResult {
    pub settings: FileConfig,
    pub stats: LibraryStats,
    pub encoder_status: AudioEncoderStatus,
}

pub struct DesktopState {
    app: RwLock<PodcastApp>,
    config: Mutex<FileConfig>,
    active_task: Mutex<Option<String>>,
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
            active_task: Mutex::new(None),
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

    async fn begin_task(&self, name: &str) -> Result<(), AppErrorDto> {
        let mut active = self.active_task.lock().await;
        if let Some(active_name) = active.as_ref() {
            return Err(AppErrorDto {
                kind: "task_active".to_string(),
                message: format!("{active_name} is already running"),
            });
        }
        *active = Some(name.to_string());
        Ok(())
    }

    async fn end_task(&self) {
        let mut active = self.active_task.lock().await;
        *active = None;
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
) -> Result<FeedCheckSummary, AppErrorDto> {
    let task_name = "subscribe_feed";
    state.begin_task(task_name).await?;
    emit_task_started(&app_handle, task_name);
    let result = subscribe_feed_inner(&app_handle, &state, feed_url).await;
    emit_task_finished(&app_handle, task_name, result.clone());
    state.end_task().await;
    result
}

#[tauri::command]
async fn remove_feed(state: State<'_, DesktopState>, feed_id: String) -> Result<(), AppErrorDto> {
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
) -> Result<FeedCheckSummary, AppErrorDto> {
    let task_name = "check_feed";
    state.begin_task(task_name).await?;
    emit_task_started(&app_handle, task_name);
    let app = state.app().await;
    let result = app
        .check_feed_with_progress(&feed_id, Some(progress_sender(app_handle.clone())))
        .await
        .map_err(AppErrorDto::from);
    emit_task_finished(&app_handle, task_name, result.clone());
    state.end_task().await;
    result
}

#[tauri::command]
async fn check_all(
    app_handle: AppHandle,
    state: State<'_, DesktopState>,
) -> Result<CheckSummary, AppErrorDto> {
    let task_name = "check_all";
    state.begin_task(task_name).await?;
    emit_task_started(&app_handle, task_name);
    let app = state.app().await;
    let result = app
        .check_all_with_progress(Some(progress_sender(app_handle.clone())))
        .await
        .map_err(AppErrorDto::from);
    emit_task_finished(&app_handle, task_name, result.clone());
    state.end_task().await;
    result
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
) -> Result<DownloadBatchSummary, AppErrorDto> {
    let task_name = "download_episodes";
    state.begin_task(task_name).await?;
    emit_task_started(&app_handle, task_name);
    let app = state.app().await;
    let result = app
        .download_episodes_with_progress(episode_ids, Some(progress_sender(app_handle.clone())))
        .await
        .map_err(AppErrorDto::from);
    emit_task_finished(&app_handle, task_name, result.clone());
    state.end_task().await;
    result
}

#[tauri::command]
async fn set_feed_retention(
    state: State<'_, DesktopState>,
    feed_id: String,
    retention_limit: Option<u32>,
) -> Result<(), AppErrorDto> {
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
    let task_name = "save_settings";
    state.begin_task(task_name).await?;
    let result = save_settings_inner(&state, config).await;
    state.end_task().await;
    result
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

async fn subscribe_feed_inner(
    app_handle: &AppHandle,
    state: &DesktopState,
    feed_url: String,
) -> Result<FeedCheckSummary, AppErrorDto> {
    let app = state.app().await;
    let feed = app.add_feed(&feed_url).await.map_err(AppErrorDto::from)?;
    app.check_feed_with_progress(&feed.id, Some(progress_sender(app_handle.clone())))
        .await
        .map_err(AppErrorDto::from)
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
                    let _ = check_all(handle.clone(), state).await;
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

    #[tokio::test]
    async fn active_task_blocks_overlapping_work() {
        let temp = tempfile::tempdir().unwrap();
        let state = DesktopState::new(temp.path().to_path_buf(), temp.path().join("config.toml"))
            .await
            .unwrap();

        state.begin_task("check_all").await.unwrap();
        let error = state.begin_task("download_episodes").await.unwrap_err();
        assert_eq!(error.kind, "task_active");
        state.end_task().await;
        state.begin_task("download_episodes").await.unwrap();
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
