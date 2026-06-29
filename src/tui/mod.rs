use std::{collections::HashMap, io, time::Duration};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Clear, Gauge, List, ListItem, ListState, Paragraph, Wrap},
};
use tokio::{sync::mpsc, task::JoinHandle};

mod helpers;
mod progress;

use helpers::*;
use progress::ProgressState;

use crate::{
    PodcastApp,
    core::{
        AudioEncoderStatus, CheckSummary, DownloadBatchSummary, DownloadProgress, EpisodeRecord,
        EpisodeStatus, FeedCheckSummary, FeedSubscription, LibraryStats, PodcastSearchResult,
        Result,
    },
};

type TuiTerminal = Terminal<CrosstermBackend<io::Stdout>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Search,
    Watching,
    Episodes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Normal,
    Search,
    ConfirmUnwatch,
}

#[derive(Debug, Clone)]
enum Dialog {
    Help,
}

#[derive(Debug)]
enum TaskEvent {
    CheckedAll(Result<CheckSummary>),
    CheckedFeed(Result<FeedCheckSummary>),
    DownloadedEpisodes {
        episode_ids: Vec<String>,
        result: Result<DownloadBatchSummary>,
    },
}

pub async fn run(app: PodcastApp) -> Result<()> {
    let mut app = TuiApp::new(app).await?;
    let mut terminal = setup_terminal()?;
    let result = app.run(&mut terminal).await;
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<TuiTerminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut TuiTerminal) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

struct TuiApp {
    app: PodcastApp,
    feeds: Vec<FeedSubscription>,
    episodes: Vec<EpisodeRecord>,
    episodes_feed_id: Option<String>,
    queued_episode_ids: HashMap<String, EpisodeStatus>,
    stats: LibraryStats,
    search_results: Vec<PodcastSearchResult>,
    search_query: String,
    selected_feed: usize,
    selected_episode: usize,
    selected_result: usize,
    panel: Panel,
    input_mode: InputMode,
    status: String,
    encoder_status: Option<AudioEncoderStatus>,
    dialog: Option<Dialog>,
    progress: ProgressState,
    progress_tx: Option<mpsc::UnboundedSender<DownloadProgress>>,
    progress_rx: Option<mpsc::UnboundedReceiver<DownloadProgress>>,
    task_rx: mpsc::UnboundedReceiver<TaskEvent>,
    task_tx: mpsc::UnboundedSender<TaskEvent>,
    check_task: Option<JoinHandle<()>>,
    download_task: Option<JoinHandle<()>>,
    pending_download_ids: Vec<String>,
    should_quit: bool,
}

impl TuiApp {
    async fn new(app: PodcastApp) -> Result<Self> {
        let feeds = app.list_feeds().await?;
        let stats = app.library_stats().await?;
        let encoder_status = Some(app.audio_encoder_status().await);
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        let mut tui = Self {
            app,
            feeds,
            episodes: Vec::new(),
            episodes_feed_id: None,
            queued_episode_ids: HashMap::new(),
            stats,
            search_results: Vec::new(),
            search_query: String::new(),
            selected_feed: 0,
            selected_episode: 0,
            selected_result: 0,
            panel: Panel::Search,
            input_mode: InputMode::Search,
            status: "Type a podcast name, then press Enter to search.".to_string(),
            encoder_status,
            dialog: None,
            progress: ProgressState::default(),
            progress_tx: None,
            progress_rx: None,
            task_rx,
            task_tx,
            check_task: None,
            download_task: None,
            pending_download_ids: Vec::new(),
            should_quit: false,
        };
        if !tui.feeds.is_empty() {
            tui.start_check_all();
        }
        Ok(tui)
    }

    async fn run(&mut self, terminal: &mut TuiTerminal) -> Result<()> {
        while !self.should_quit {
            self.progress.tick();
            self.drain_progress();
            self.drain_tasks().await?;
            terminal.draw(|frame| self.render(frame))?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.handle_key(key).await?;
            }
        }
        Ok(())
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if is_quit_key(key) {
            self.should_quit = true;
            return Ok(());
        }

        if self.dialog.is_some() {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                self.dialog = None;
            }
            return Ok(());
        }

        match self.input_mode {
            InputMode::Search => self.handle_search_key(key).await,
            InputMode::ConfirmUnwatch => self.handle_unwatch_key(key).await,
            InputMode::Normal => self.handle_normal_key(key).await,
        }
    }

    async fn handle_search_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status = "Search field unfocused. Press s to search again.".to_string();
            }
            KeyCode::Enter => self.search().await?,
            KeyCode::Backspace => {
                self.search_query.pop();
            }
            KeyCode::Down => self.move_selection(1),
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Tab => {
                self.input_mode = InputMode::Normal;
                self.next_panel();
            }
            KeyCode::Char(ch) if is_text_input_key(key) => self.search_query.push(ch),
            _ => {}
        }
        Ok(())
    }

    async fn handle_normal_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('s') => {
                self.panel = Panel::Search;
                self.input_mode = InputMode::Search;
                self.status = "Editing search.".to_string();
            }
            KeyCode::Char('?') => self.dialog = Some(Dialog::Help),
            KeyCode::Char('e') => self.refresh_encoder_status().await,
            KeyCode::Enter | KeyCode::Char(' ') => self.primary_action().await?,
            KeyCode::Tab => self.next_panel(),
            KeyCode::BackTab => self.previous_panel(),
            KeyCode::Char('a') => self.add_selected_result().await?,
            KeyCode::Char('c') => self.context_check(),
            KeyCode::Char('d') => self.start_download_selected_episode(),
            KeyCode::Char('r') => self.refresh_feeds().await?,
            KeyCode::Char('u') => self.prompt_unwatch(),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            _ => {}
        }
        Ok(())
    }

    async fn handle_unwatch_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.unwatch_selected_feed().await?,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.status = "Unwatch cancelled.".to_string();
            }
            _ => {}
        }
        Ok(())
    }

    async fn primary_action(&mut self) -> Result<()> {
        match self.panel {
            Panel::Search => self.add_selected_result().await,
            Panel::Watching => self.open_selected_feed_episodes().await,
            Panel::Episodes => {
                self.start_download_selected_episode();
                Ok(())
            }
        }
    }

    async fn search(&mut self) -> Result<()> {
        let query = self.search_query.trim();
        if query.is_empty() {
            self.status = "Search query is empty.".to_string();
            return Ok(());
        }
        self.panel = Panel::Search;
        self.status = format!("Searching for \"{query}\"...");
        self.search_results = self.app.search_podcasts(query).await?;
        self.selected_result = 0;
        self.input_mode = InputMode::Normal;
        self.status = format!(
            "Found {} result(s). Select one and press Subscribe.",
            self.search_results.len()
        );
        Ok(())
    }

    async fn add_selected_result(&mut self) -> Result<()> {
        self.panel = Panel::Search;
        let Some(result) = self.search_results.get(self.selected_result) else {
            self.status = "No selected result to subscribe.".to_string();
            return Ok(());
        };
        let Some(feed_url) = result.feed_url.clone() else {
            self.status = "Selected result has no RSS feed URL.".to_string();
            return Ok(());
        };

        match self.app.add_feed(&feed_url).await {
            Ok(feed) => {
                let title = feed.normalized_title.clone();
                self.refresh_feeds().await?;
                if let Some(index) = self.feeds.iter().position(|item| item.id == feed.id) {
                    self.selected_feed = index;
                }
                self.panel = Panel::Watching;
                self.status = format!("Now watching {title}. Checking feed...");
                self.start_check_feed(feed);
            }
            Err(error) => self.status = format!("Subscribe failed: {error}"),
        }
        Ok(())
    }

    async fn refresh_feeds(&mut self) -> Result<()> {
        self.feeds = self.app.list_feeds().await?;
        self.stats = self.app.library_stats().await?;
        if self.selected_feed >= self.feeds.len() {
            self.selected_feed = self.feeds.len().saturating_sub(1);
        }
        self.status = format!("Watching {} show(s).", self.feeds.len());
        Ok(())
    }

    async fn open_selected_feed_episodes(&mut self) -> Result<()> {
        let Some(feed) = self.feeds.get(self.selected_feed) else {
            self.status = "No watched show selected.".to_string();
            return Ok(());
        };
        self.episodes = self.app.list_episodes(&feed.id).await?;
        self.episodes_feed_id = Some(feed.id.clone());
        self.selected_episode = 0;
        self.panel = Panel::Episodes;
        self.status = format!(
            "{} episode record(s) for {}.",
            self.episodes.len(),
            feed.normalized_title
        );
        Ok(())
    }

    fn context_check(&mut self) {
        match self.panel {
            Panel::Search => self.start_check_all(),
            Panel::Watching => self.start_check_selected_feed(),
            Panel::Episodes => self.start_check_episode_feed(),
        }
    }

    fn start_check_all(&mut self) {
        if self.check_task.is_some() {
            self.status = "A check is already running.".to_string();
            return;
        }
        let app = self.app.clone();
        let task_tx = self.task_tx.clone();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        self.progress = ProgressState::default();
        self.progress.label = "Checking watched shows...".to_string();
        self.progress_tx = Some(progress_tx.clone());
        self.progress_rx = Some(progress_rx);
        self.check_task = Some(tokio::spawn(async move {
            let result = app.check_all_with_progress(Some(progress_tx)).await;
            let _ = task_tx.send(TaskEvent::CheckedAll(result));
        }));
    }

    fn start_check_selected_feed(&mut self) {
        let Some(feed) = self.feeds.get(self.selected_feed).cloned() else {
            self.status = "No watched show selected.".to_string();
            return;
        };
        self.start_check_feed(feed);
    }

    fn start_check_episode_feed(&mut self) {
        let Some(feed_id) = self.episodes_feed_id.clone() else {
            self.status = "No episode feed selected.".to_string();
            return;
        };
        let Some(feed) = self.feeds.iter().find(|feed| feed.id == feed_id).cloned() else {
            self.status = "Episode feed is no longer watched.".to_string();
            return;
        };
        self.start_check_feed(feed);
    }

    fn start_check_feed(&mut self, feed: FeedSubscription) {
        if self.check_task.is_some() {
            self.status = "A check is already running.".to_string();
            return;
        }
        let app = self.app.clone();
        let task_tx = self.task_tx.clone();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        self.progress = ProgressState::default();
        self.progress.label = format!("Checking {}...", feed.normalized_title);
        self.progress_tx = Some(progress_tx.clone());
        self.progress_rx = Some(progress_rx);
        self.check_task = Some(tokio::spawn(async move {
            let result = app
                .check_feed_with_progress(&feed.id, Some(progress_tx))
                .await;
            let _ = task_tx.send(TaskEvent::CheckedFeed(result));
        }));
    }

    fn start_download_selected_episode(&mut self) {
        let Some(episode) = self.episodes.get(self.selected_episode).cloned() else {
            self.status = "No episode selected.".to_string();
            return;
        };
        if self.queued_episode_ids.contains_key(&episode.id) {
            self.status = "Episode is already queued.".to_string();
            return;
        }
        if episode.status == EpisodeStatus::Downloaded {
            self.status = "Episode is already downloaded.".to_string();
            return;
        }
        let title = episode.normalized_title.clone();
        if let Some(selected) = self.episodes.get_mut(self.selected_episode) {
            self.queued_episode_ids
                .insert(selected.id.clone(), selected.status);
            selected.status = EpisodeStatus::Pending;
        }
        let episode_id = episode.id.clone();
        self.pending_download_ids.push(episode_id);
        self.progress.label = format!("Queued {title}");
        self.start_pending_downloads_if_idle();
    }

    fn start_pending_downloads_if_idle(&mut self) {
        if self.check_task.is_some()
            || self.download_task.is_some()
            || self.pending_download_ids.is_empty()
        {
            return;
        }
        let episode_ids = std::mem::take(&mut self.pending_download_ids);
        let app = self.app.clone();
        let task_tx = self.task_tx.clone();
        let (progress_tx, progress_rx) = mpsc::unbounded_channel();
        self.progress = ProgressState::default();
        self.progress_tx = Some(progress_tx.clone());
        self.progress_rx = Some(progress_rx);
        self.download_task = Some(tokio::spawn(async move {
            let result = app
                .download_episodes_with_progress(episode_ids.clone(), Some(progress_tx))
                .await;
            let _ = task_tx.send(TaskEvent::DownloadedEpisodes {
                episode_ids,
                result,
            });
        }));
    }

    fn prompt_unwatch(&mut self) {
        let Some(feed) = self.feeds.get(self.selected_feed) else {
            self.status = "No watched show selected.".to_string();
            return;
        };
        self.panel = Panel::Watching;
        self.input_mode = InputMode::ConfirmUnwatch;
        self.status = format!(
            "Unwatch {}? Press y to confirm, n to cancel.",
            feed.normalized_title
        );
    }

    async fn unwatch_selected_feed(&mut self) -> Result<()> {
        let Some(feed) = self.feeds.get(self.selected_feed).cloned() else {
            self.status = "No watched show selected.".to_string();
            self.input_mode = InputMode::Normal;
            return Ok(());
        };
        self.app.remove_feed(&feed.id).await?;
        self.refresh_feeds().await?;
        self.episodes.clear();
        self.episodes_feed_id = None;
        self.input_mode = InputMode::Normal;
        self.status = format!("Unwatched {}.", feed.normalized_title);
        Ok(())
    }

    async fn refresh_encoder_status(&mut self) {
        self.encoder_status = Some(self.app.audio_encoder_status().await);
        self.status = "Encoder status refreshed.".to_string();
    }

    async fn drain_tasks(&mut self) -> Result<()> {
        while let Ok(event) = self.task_rx.try_recv() {
            match event {
                TaskEvent::CheckedAll(Ok(summary)) => {
                    self.check_task = None;
                    if self.download_task.is_none() && self.pending_download_ids.is_empty() {
                        self.progress_rx = None;
                        self.progress_tx = None;
                    }
                    self.refresh_feeds().await?;
                    if let Some(feed_id) = self.episodes_feed_id.clone() {
                        self.episodes = self.app.list_episodes(&feed_id).await?;
                    }
                    self.status = format!(
                        "Checked {} show(s): {} downloaded, {} queued, {} failed.",
                        summary.feeds_checked, summary.downloaded, summary.queued, summary.failed
                    );
                }
                TaskEvent::CheckedAll(Err(error)) => {
                    self.check_task = None;
                    if self.download_task.is_none() && self.pending_download_ids.is_empty() {
                        self.progress_rx = None;
                        self.progress_tx = None;
                    }
                    self.status = format!("Check all failed: {error}");
                }
                TaskEvent::CheckedFeed(Ok(summary)) => {
                    self.check_task = None;
                    if self.download_task.is_none() && self.pending_download_ids.is_empty() {
                        self.progress_rx = None;
                        self.progress_tx = None;
                    }
                    self.refresh_feeds().await?;
                    self.episodes = self.app.list_episodes(&summary.feed_id).await?;
                    self.episodes_feed_id = Some(summary.feed_id.clone());
                    self.status = format!(
                        "{}: {} downloaded, {} queued, {} failed.",
                        summary.feed_title, summary.downloaded, summary.queued, summary.failed
                    );
                }
                TaskEvent::CheckedFeed(Err(error)) => {
                    self.check_task = None;
                    if self.download_task.is_none() && self.pending_download_ids.is_empty() {
                        self.progress_rx = None;
                        self.progress_tx = None;
                    }
                    self.status = format!("Operation failed: {error}");
                }
                TaskEvent::DownloadedEpisodes {
                    episode_ids,
                    result,
                } => {
                    self.download_task = None;
                    for episode_id in &episode_ids {
                        self.queued_episode_ids.remove(episode_id);
                    }
                    match result {
                        Ok(summary) => {
                            self.refresh_feeds().await?;
                            if let Some(feed_id) = self.episodes_feed_id.clone().or_else(|| {
                                summary
                                    .feed_summaries
                                    .first()
                                    .map(|feed_summary| feed_summary.feed_id.clone())
                            }) {
                                self.episodes = self.app.list_episodes(&feed_id).await?;
                                self.episodes_feed_id = Some(feed_id);
                            }
                            self.status = format!(
                                "Downloaded {} episode(s), {} failed. {} still queued.",
                                summary.downloaded,
                                summary.failed,
                                self.queued_episode_ids.len()
                            );
                        }
                        Err(error) => {
                            for episode_id in &episode_ids {
                                if let Some(episode) =
                                    self.episodes.iter_mut().find(|item| &item.id == episode_id)
                                {
                                    episode.status = EpisodeStatus::Failed;
                                }
                            }
                            self.status = format!("Download failed: {error}");
                        }
                    }
                    if self.check_task.is_none()
                        && self.download_task.is_none()
                        && self.pending_download_ids.is_empty()
                    {
                        self.progress_rx = None;
                        self.progress_tx = None;
                    }
                }
            }
            self.start_pending_downloads_if_idle();
        }
        Ok(())
    }

    fn drain_progress(&mut self) {
        if let Some(rx) = &mut self.progress_rx {
            while let Ok(event) = rx.try_recv() {
                self.progress.apply(event);
            }
        }
    }

    fn next_panel(&mut self) {
        self.input_mode = InputMode::Normal;
        self.panel = match self.panel {
            Panel::Search => Panel::Watching,
            Panel::Watching => Panel::Episodes,
            Panel::Episodes => Panel::Search,
        };
    }

    fn previous_panel(&mut self) {
        self.input_mode = InputMode::Normal;
        self.panel = match self.panel {
            Panel::Search => Panel::Episodes,
            Panel::Watching => Panel::Search,
            Panel::Episodes => Panel::Watching,
        };
    }

    fn move_selection(&mut self, delta: isize) {
        match self.panel {
            Panel::Search => {
                self.selected_result =
                    move_index(self.selected_result, self.search_results.len(), delta);
            }
            Panel::Watching => {
                self.selected_feed = move_index(self.selected_feed, self.feeds.len(), delta);
            }
            Panel::Episodes => {
                self.selected_episode =
                    move_index(self.selected_episode, self.episodes.len(), delta);
            }
        }
    }

    fn render(&mut self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let root = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(14),
                Constraint::Length(3),
                Constraint::Length(6),
            ])
            .split(area);

        self.render_header(frame, root[0]);
        self.render_body(frame, root[1]);
        self.render_action_bar(frame, root[2]);
        self.render_monitor(frame, root[3]);
        if let Some(dialog) = &self.dialog {
            self.render_dialog(frame, centered_rect(74, 70, area), dialog);
        }
    }

    fn render_header(&self, frame: &mut ratatui::Frame, area: Rect) {
        let header = Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Podcast Downloader", title_style()),
                Span::raw("  "),
                Span::styled("Search", nav_style(self.panel == Panel::Search)),
                Span::raw(" / "),
                Span::styled("Watching", nav_style(self.panel == Panel::Watching)),
                Span::raw(" / "),
                Span::styled("Episodes", nav_style(self.panel == Panel::Episodes)),
            ]),
            Line::from(vec![
                Span::styled(format!("{} watched", self.stats.feeds), muted_style()),
                Span::raw("   "),
                Span::styled(
                    format!("{} recorded episode(s)", self.stats.episodes),
                    muted_style(),
                ),
                Span::raw("   "),
                Span::styled(
                    format!("{} downloaded", self.stats.downloaded),
                    muted_style(),
                ),
            ]),
        ])
        .block(shell_block());
        frame.render_widget(header, area);
    }

    fn render_body(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(56), Constraint::Percentage(44)])
            .split(area);
        let search = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(8)])
            .split(chunks[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[1]);

        self.render_search_box(frame, search[0]);
        self.render_search_results(frame, search[1]);
        self.render_feeds(frame, right[0]);
        self.render_episodes(frame, right[1]);
    }

    fn render_search_box(&self, frame: &mut ratatui::Frame, area: Rect) {
        let text = if self.search_query.is_empty() {
            Line::from(Span::styled("Search podcast feeds", subdued_style()))
        } else {
            Line::from(Span::styled(self.search_query.as_str(), input_style()))
        };
        let title = if self.input_mode == InputMode::Search {
            "Search - typing"
        } else {
            "Search"
        };
        let input = Paragraph::new(text)
            .block(panel_block(title, self.panel == Panel::Search))
            .wrap(Wrap { trim: false });
        frame.render_widget(input, area);
    }

    fn render_search_results(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        if self.search_results.is_empty() {
            render_empty(
                frame,
                area,
                "Results",
                "Press s, type a show name, then Enter.",
            );
            return;
        }
        let items = self
            .search_results
            .iter()
            .map(|result| {
                let (marker, marker_style) = if result.feed_url.is_some() {
                    ("RSS", Style::default().fg(Color::Rgb(134, 239, 172)))
                } else {
                    ("no RSS", Style::default().fg(Color::Rgb(248, 113, 113)))
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(result.title.clone(), text_style()),
                        Span::raw("  "),
                        Span::styled(marker, marker_style),
                    ]),
                    Line::from(Span::styled(
                        result.author.as_deref().unwrap_or("unknown").to_string(),
                        muted_style(),
                    )),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default();
        state.select(Some(self.selected_result.min(items.len() - 1)));
        let list = List::new(items)
            .block(panel_block("Results", self.panel == Panel::Search))
            .highlight_style(highlight_style())
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_feeds(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        if self.feeds.is_empty() {
            render_empty(
                frame,
                area,
                "Watching",
                "Subscribe to a search result first.",
            );
            return;
        }
        let items = self
            .feeds
            .iter()
            .map(|feed| {
                let checked = short_date(feed.last_checked_at.as_deref()).unwrap_or("never");
                ListItem::new(vec![
                    Line::from(Span::styled(feed.normalized_title.clone(), text_style())),
                    Line::from(vec![
                        Span::styled("last checked ", muted_style()),
                        Span::styled(checked.to_string(), subdued_style()),
                    ]),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default();
        state.select(Some(self.selected_feed.min(items.len() - 1)));
        let list = List::new(items)
            .block(panel_block("Watching", self.panel == Panel::Watching))
            .highlight_style(highlight_style())
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_episodes(&mut self, frame: &mut ratatui::Frame, area: Rect) {
        if self.episodes.is_empty() {
            render_empty(
                frame,
                area,
                "Episodes",
                "Open a watched show to see episodes.",
            );
            return;
        }
        let items = self
            .episodes
            .iter()
            .map(|episode| {
                let date = short_date(episode.published_at.as_deref()).unwrap_or("unknown date");
                let status = if self.queued_episode_ids.contains_key(&episode.id) {
                    EpisodeStatus::Pending
                } else {
                    episode.status
                };
                ListItem::new(vec![
                    Line::from(vec![
                        Span::styled(status_label(status), status_style(status)),
                        Span::raw(" "),
                        Span::styled(episode.normalized_title.clone(), text_style()),
                    ]),
                    Line::from(Span::styled(date.to_string(), muted_style())),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default();
        state.select(Some(self.selected_episode.min(items.len() - 1)));
        let list = List::new(items)
            .block(panel_block("Episodes", self.panel == Panel::Episodes))
            .highlight_style(highlight_style())
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut state);
    }

    fn render_action_bar(&self, frame: &mut ratatui::Frame, area: Rect) {
        let actions = match self.panel {
            Panel::Search => vec![
                ("S", "Search"),
                ("A", "Subscribe"),
                ("C", "Check All"),
                ("?", "Help"),
            ],
            Panel::Watching => vec![
                ("Enter", "Episodes"),
                ("C", "Check"),
                ("U", "Unwatch"),
                ("R", "Refresh"),
            ],
            Panel::Episodes => vec![
                ("Enter", "Download"),
                ("D", "Download"),
                ("C", "Check Show"),
                ("Tab", "Switch"),
            ],
        };
        let mut spans = Vec::new();
        for (index, (key, label)) in actions.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw("  "));
            }
            spans.push(Span::styled(format!("[{key}]"), button_key_style()));
            spans.push(Span::raw(" "));
            spans.push(Span::styled(*label, button_label_style()));
        }
        let action_bar = Paragraph::new(Line::from(spans))
            .block(shell_block().title("Actions"))
            .wrap(Wrap { trim: true });
        frame.render_widget(action_bar, area);
    }

    fn render_monitor(&self, frame: &mut ratatui::Frame, area: Rect) {
        let block = shell_block().title("Monitor");
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(inner);
        let active = self.progress.active_downloads();
        let converting = self.progress.active_conversions();
        let pending = self.progress.queued_pending();
        let completed = self.progress.completed();
        let queued = self.progress.queued();
        let failed = self.progress.failed();
        let syncing_feeds = self.progress.syncing_feeds();

        let status_line = if syncing_feeds == 0 {
            Line::from(vec![
                Span::styled("sync ", muted_style()),
                Span::styled("ok", success_style()),
                Span::raw("   "),
                Span::styled("queue ", muted_style()),
                Span::styled(format!("{pending}/{queued}"), text_style()),
                Span::raw("   "),
                Span::styled("download ", muted_style()),
                Span::styled(active.to_string(), text_style()),
                Span::raw("   "),
                Span::styled("convert ", muted_style()),
                Span::styled(
                    if converting > 0 { "running" } else { "idle" },
                    if converting > 0 {
                        warning_style()
                    } else {
                        muted_style()
                    },
                ),
            ])
        } else {
            Line::from(vec![
                Span::styled("sync ", muted_style()),
                Span::styled(
                    format!("{} {} feed(s)", self.progress.spinner(), syncing_feeds),
                    warning_style(),
                ),
                Span::raw("   "),
                Span::styled("queue ", muted_style()),
                Span::styled(format!("{pending}/{queued}"), text_style()),
                Span::raw("   "),
                Span::styled("download ", muted_style()),
                Span::styled(active.to_string(), text_style()),
                Span::raw("   "),
                Span::styled("convert ", muted_style()),
                Span::styled(
                    if converting > 0 { "running" } else { "idle" },
                    if converting > 0 {
                        warning_style()
                    } else {
                        muted_style()
                    },
                ),
            ])
        };
        frame.render_widget(Paragraph::new(status_line), rows[0]);

        let (task_line, current_ratio) = if let Some(task) = self.progress.current_download() {
            let bytes = task
                .total_bytes
                .filter(|total| *total > 0)
                .map(|total| {
                    format!(
                        "  {} / {}",
                        format_bytes(task.downloaded_bytes),
                        format_bytes(total)
                    )
                })
                .unwrap_or_else(|| format!("  {}", format_bytes(task.downloaded_bytes)));
            (
                Line::from(vec![
                    Span::styled("current ", muted_style()),
                    Span::styled("downloading ", success_style()),
                    Span::raw(task.title.clone()),
                    Span::styled(bytes, muted_style()),
                ]),
                self.progress.current_download_ratio(),
            )
        } else if let Some(title) = self.progress.current_conversion_title() {
            (
                Line::from(vec![
                    Span::styled("current ", muted_style()),
                    Span::styled("converting ", warning_style()),
                    Span::raw(title.to_string()),
                ]),
                0.0,
            )
        } else {
            (
                Line::from(vec![
                    Span::styled("current ", muted_style()),
                    Span::styled(
                        self.progress.monitor_label(self.status.as_str()),
                        muted_style(),
                    ),
                ]),
                0.0,
            )
        };
        frame.render_widget(Paragraph::new(task_line), rows[1]);

        let progress_ratio = if active > 0 {
            current_ratio
        } else {
            self.progress.overall_ratio()
        };
        let progress_bar = Gauge::default()
            .gauge_style(
                Style::default()
                    .fg(Color::Rgb(45, 212, 191))
                    .bg(Color::Rgb(15, 23, 42)),
            )
            .ratio(progress_ratio)
            .label("");
        frame.render_widget(progress_bar, inset_horizontal(rows[2], 1));

        let detail = format!(
            "done: {completed}/{}   failed: {}   pending: {}   active: {}",
            queued, failed, pending, active
        );
        frame.render_widget(Paragraph::new(detail).style(subdued_style()), rows[3]);
    }

    fn render_dialog(&self, frame: &mut ratatui::Frame, area: Rect, dialog: &Dialog) {
        frame.render_widget(Clear, area);
        let (title, lines) = match dialog {
            Dialog::Help => (
                "Help",
                vec![
                    Line::from("This is an app-style TUI, not a command prompt."),
                    Line::from(
                        "s search | a subscribe | enter open/download | c check | u unwatch",
                    ),
                    Line::from("tab switch panels | e encoder | ? help | q quit"),
                ],
            ),
        };
        let popup = Paragraph::new(lines)
            .block(panel_block(title, true))
            .style(text_style())
            .wrap(Wrap { trim: false });
        frame.render_widget(popup, area);
    }
}
