mod common;

use podcast_downloader::{CoreConfig, DownloadProgress, EpisodeStatus, PodcastApp};

#[tokio::test]
async fn first_check_downloads_latest_four_and_second_check_does_not_duplicate() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_five_media(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in [
        "/episode-2.mp3",
        "/episode-3.mp3",
        "/episode-4.mp3",
        "/episode-5.mp3",
    ] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let first = test.app.check_feed(&feed.id).await.unwrap();
    assert_eq!(first.queued, 4);
    assert_eq!(first.downloaded, 4);
    assert_eq!(first.skipped_initial, 1);
    assert_eq!(test.downloaded_files().len(), 4);

    let second = test.app.check_feed(&feed.id).await.unwrap();
    assert_eq!(second.queued, 0);
    assert_eq!(second.downloaded, 0);
    assert_eq!(test.downloaded_files().len(), 4);
}

#[tokio::test]
async fn list_episodes_returns_watched_show_history() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_five_media(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in [
        "/episode-2.mp3",
        "/episode-3.mp3",
        "/episode-4.mp3",
        "/episode-5.mp3",
    ] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    test.app.check_feed(&feed.id).await.unwrap();

    let episodes = test.app.list_episodes(&feed.id).await.unwrap();
    let stats = test.app.library_stats().await.unwrap();

    assert_eq!(episodes.len(), 5);
    assert_eq!(stats.feeds, 1);
    assert_eq!(stats.episodes, 5);
    assert_eq!(stats.downloaded, 4);
    assert_eq!(
        episodes
            .iter()
            .filter(|episode| episode.status == EpisodeStatus::Downloaded)
            .count(),
        4
    );
    assert!(
        episodes
            .iter()
            .any(|episode| episode.status == EpisodeStatus::SkippedInitial)
    );
}

#[tokio::test]
async fn check_feed_emits_download_progress() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_single_mp3(&server.uri())),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-3.mp3"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("newest audio"))
        .mount(&server)
        .await;

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    test.app
        .check_feed_with_progress(&feed.id, Some(progress_tx))
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = progress_rx.try_recv() {
        events.push(event);
    }

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadProgress::DownloadStarted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadProgress::DownloadFinished { .. }))
    );
}

#[tokio::test]
async fn skipped_episode_can_be_downloaded_manually() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_five_media(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in [
        "/episode-1.mp3",
        "/episode-2.mp3",
        "/episode-3.mp3",
        "/episode-4.mp3",
        "/episode-5.mp3",
    ] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    test.app.check_feed(&feed.id).await.unwrap();
    let skipped = test
        .app
        .list_episodes(&feed.id)
        .await
        .unwrap()
        .into_iter()
        .find(|episode| {
            episode.status == EpisodeStatus::SkippedInitial
                && episode.media_url.ends_with("/episode-1.mp3")
        })
        .unwrap();

    let summary = test
        .app
        .download_episode_with_progress(&skipped.id, None)
        .await
        .unwrap();

    assert_eq!(summary.queued, 1);
    assert_eq!(summary.downloaded, 1, "errors: {:?}", summary.errors);
    assert_eq!(summary.deleted_by_retention, 0);
    assert_eq!(test.downloaded_files().len(), 5);
    let downloaded = test
        .app
        .list_episodes(&feed.id)
        .await
        .unwrap()
        .into_iter()
        .find(|episode| episode.id == skipped.id)
        .unwrap();
    assert_eq!(downloaded.status, EpisodeStatus::Downloaded);
}

#[tokio::test]
async fn manual_batch_downloads_multiple_episodes_through_core_queue() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_five_media(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in [
        "/episode-1.mp3",
        "/episode-2.mp3",
        "/episode-3.mp3",
        "/episode-4.mp3",
        "/episode-5.mp3",
    ] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    test.app
        .set_feed_retention(&feed.id, Some(2))
        .await
        .unwrap();
    test.app.check_feed(&feed.id).await.unwrap();
    let skipped_ids = test
        .app
        .list_episodes(&feed.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|episode| episode.status == EpisodeStatus::SkippedInitial)
        .take(2)
        .map(|episode| episode.id)
        .collect::<Vec<_>>();

    let summary = test
        .app
        .download_episodes_with_progress(skipped_ids, None)
        .await
        .unwrap();

    assert_eq!(summary.requested, 2);
    assert_eq!(summary.queued, 2);
    assert_eq!(summary.downloaded, 2, "errors: {:?}", summary.errors);
    assert_eq!(summary.failed, 0);
    assert_eq!(summary.feed_summaries.len(), 1);
    assert_eq!(test.downloaded_files().len(), 4);
}

#[tokio::test]
async fn manual_batch_records_duplicate_and_missing_episode_failures() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_five_media(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in [
        "/episode-1.mp3",
        "/episode-2.mp3",
        "/episode-3.mp3",
        "/episode-4.mp3",
        "/episode-5.mp3",
    ] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    test.app
        .set_feed_retention(&feed.id, Some(2))
        .await
        .unwrap();
    test.app.check_feed(&feed.id).await.unwrap();
    let skipped_id = test
        .app
        .list_episodes(&feed.id)
        .await
        .unwrap()
        .into_iter()
        .find(|episode| episode.status == EpisodeStatus::SkippedInitial)
        .unwrap()
        .id;

    let summary = test
        .app
        .download_episodes_with_progress(
            vec![
                skipped_id.clone(),
                skipped_id,
                "missing-episode".to_string(),
            ],
            None,
        )
        .await
        .unwrap();

    assert_eq!(summary.requested, 3);
    assert_eq!(summary.queued, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(summary.failed, 2);
    assert_eq!(summary.errors.len(), 2);
    assert!(
        summary
            .errors
            .iter()
            .any(|error| error.contains("duplicate episode id"))
    );
    assert!(
        summary
            .errors
            .iter()
            .any(|error| error.contains("missing-episode"))
    );
}

#[tokio::test]
async fn manual_batch_respects_configured_download_concurrency() {
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let db_path = temp.path().join("podcasts.db");
    let mut config = CoreConfig::new(&db_path, &downloads);
    config.max_concurrent_downloads = 1;
    let app = PodcastApp::open(config).await.unwrap();
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_five_media(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in [
        "/episode-1.mp3",
        "/episode-2.mp3",
        "/episode-3.mp3",
        "/episode-4.mp3",
        "/episode-5.mp3",
    ] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    app.set_feed_retention(&feed.id, Some(2)).await.unwrap();
    app.check_feed(&feed.id).await.unwrap();
    let skipped_ids = app
        .list_episodes(&feed.id)
        .await
        .unwrap()
        .into_iter()
        .filter(|episode| episode.status == EpisodeStatus::SkippedInitial)
        .take(2)
        .map(|episode| episode.id)
        .collect::<Vec<_>>();
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();

    let summary = app
        .download_episodes_with_progress(skipped_ids, Some(progress_tx))
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = progress_rx.try_recv() {
        events.push(event);
    }

    assert_eq!(summary.downloaded, 2, "errors: {:?}", summary.errors);
    let first_started = events
        .iter()
        .position(|event| matches!(event, DownloadProgress::DownloadStarted { .. }))
        .unwrap();
    let first_finished = events
        .iter()
        .position(|event| matches!(event, DownloadProgress::DownloadFinished { .. }))
        .unwrap();
    let second_started = events
        .iter()
        .enumerate()
        .skip(first_started + 1)
        .find(|(_, event)| matches!(event, DownloadProgress::DownloadStarted { .. }))
        .map(|(index, _)| index)
        .unwrap();
    assert!(
        first_finished < second_started,
        "second download started before first finished: {events:?}"
    );
}

#[tokio::test]
async fn updated_feed_downloads_only_new_episodes() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    let feed_url = format!("{}/feed.xml", server.uri());
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_single_mp3(&server.uri())),
        )
        .up_to_n_times(2)
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_extra_new_episode(&server.uri())),
        )
        .mount(&server)
        .await;
    for path in ["/episode-3.mp3", "/episode-4.mp3"] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(path))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
            .mount(&server)
            .await;
    }

    let feed = test.app.add_feed(&feed_url).await.unwrap();
    test.app.check_feed(&feed.id).await.unwrap();
    let updated = test.app.check_feed(&feed.id).await.unwrap();

    assert_eq!(updated.queued, 1);
    assert_eq!(updated.downloaded, 1);
    assert_eq!(test.downloaded_files().len(), 2);
}

#[tokio::test]
async fn failed_media_download_is_recorded_and_does_not_stop_queue() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_single_mp3(&server.uri())),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-3.mp3"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let summary = test.app.check_feed(&feed.id).await.unwrap();

    assert_eq!(summary.queued, 1);
    assert_eq!(summary.failed, 1);
    assert!(test.downloaded_files().is_empty());
}

#[tokio::test]
async fn check_all_reports_per_feed_downloads() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    for feed_path in ["/feed-a.xml", "/feed-b.xml"] {
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path(feed_path))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_body_string(common::rss_with_single_mp3(&server.uri())),
            )
            .mount(&server)
            .await;
    }
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-3.mp3"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("newest audio"))
        .mount(&server)
        .await;

    test.app
        .add_feed(&format!("{}/feed-a.xml", server.uri()))
        .await
        .unwrap();
    test.app
        .add_feed(&format!("{}/feed-b.xml", server.uri()))
        .await
        .unwrap();

    let summary = test.app.check_all().await.unwrap();

    assert_eq!(summary.feeds_checked, 2);
    assert_eq!(summary.downloaded, 2);
    assert_eq!(summary.feed_summaries.len(), 2);
    assert!(
        summary
            .feed_summaries
            .iter()
            .all(|feed| feed.downloaded == 1)
    );
    let files = test.downloaded_files();
    assert_eq!(files.len(), 2);
    assert_ne!(files[0], files[1]);
    assert!(
        files
            .iter()
            .all(|path| path.extension().and_then(|ext| ext.to_str()) == Some("mp3"))
    );
    assert!(
        std::fs::read_dir(&test.downloads)
            .unwrap()
            .all(|entry| !entry.unwrap().path().to_string_lossy().contains(".lock"))
    );
}

#[tokio::test]
async fn startup_removes_stale_lock_and_part_files() {
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::write(downloads.join("得体广播站 - episode.mp3.lock"), b"").unwrap();
    std::fs::write(
        downloads.join("得体广播站 - episode.m4a.uuid.part"),
        b"partial",
    )
    .unwrap();

    let config = CoreConfig::new(temp.path().join("podcasts.db"), &downloads);
    PodcastApp::open(config).await.unwrap();

    assert!(std::fs::read_dir(&downloads).unwrap().next().is_none());
}

#[tokio::test]
async fn non_mp3_download_requires_encoder_and_does_not_complete_as_m4a() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_m4a(&server.uri())),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-1.m4a"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("m4a bytes"))
        .mount(&server)
        .await;

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let summary = test.app.check_feed(&feed.id).await.unwrap();

    assert_eq!(summary.queued, 1);
    assert_eq!(summary.downloaded, 0);
    assert_eq!(summary.failed, 1);
    assert!(summary.errors.iter().any(|error| error.contains("mp3")));
    assert!(test.downloaded_files().is_empty());
}

#[tokio::test]
async fn m4a_download_converts_to_mp3_when_ffmpeg_is_configured() {
    let Some(ffmpeg) = installed_ffmpeg_path() else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let db_path = temp.path().join("podcasts.db");
    let mut config = CoreConfig::new(&db_path, &downloads);
    config.mp3_encoder_path = Some(ffmpeg);
    let app = PodcastApp::open(config).await.unwrap();
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_m4a(&server.uri())),
        )
        .mount(&server)
        .await;
    let source_m4a = tiny_m4a_fixture().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-1.m4a"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(source_m4a))
        .mount(&server)
        .await;

    let feed = app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let summary = app.check_feed(&feed.id).await.unwrap();
    let files = std::fs::read_dir(&downloads)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();

    assert_eq!(summary.downloaded, 1, "errors: {:?}", summary.errors);
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].extension().and_then(|value| value.to_str()),
        Some("mp3")
    );
}

#[tokio::test]
async fn m4a_download_emits_conversion_progress() {
    let Some(ffmpeg) = installed_ffmpeg_path() else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let downloads = temp.path().join("downloads");
    let db_path = temp.path().join("podcasts.db");
    let mut config = CoreConfig::new(&db_path, &downloads);
    config.mp3_encoder_path = Some(ffmpeg);
    let app = PodcastApp::open(config).await.unwrap();
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_m4a(&server.uri())),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-1.m4a"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes(tiny_m4a_fixture().await))
        .mount(&server)
        .await;

    let feed = app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel();
    app.check_feed_with_progress(&feed.id, Some(progress_tx))
        .await
        .unwrap();
    let mut events = Vec::new();
    while let Ok(event) = progress_rx.try_recv() {
        events.push(event);
    }

    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadProgress::ConversionStarted { .. }))
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, DownloadProgress::ConversionFinished { .. }))
    );
}

async fn tiny_m4a_fixture() -> Vec<u8> {
    let path = std::env::temp_dir().join("podcast_downloader_tiny_fixture.m4a");
    if !path.exists() {
        let ffmpeg = installed_ffmpeg_path().unwrap();
        let status = tokio::process::Command::new(ffmpeg)
            .arg("-y")
            .arg("-f")
            .arg("lavfi")
            .arg("-i")
            .arg("sine=frequency=440:duration=0.25")
            .arg("-c:a")
            .arg("aac")
            .arg(&path)
            .status()
            .await
            .unwrap();
        assert!(status.success());
    }
    tokio::fs::read(path).await.unwrap()
}

fn installed_ffmpeg_path() -> Option<std::path::PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let root = std::path::PathBuf::from(local)
        .join("Microsoft")
        .join("WinGet")
        .join("Packages")
        .join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe");
    std::fs::read_dir(root)
        .ok()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path().join("bin").join("ffmpeg.exe"))
        .find(|path| path.exists())
}

#[tokio::test]
async fn unknown_media_requires_encoder_and_does_not_complete_as_mp3() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_unknown_media(&server.uri())),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-download"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("unknown bytes"))
        .mount(&server)
        .await;

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    let summary = test.app.check_feed(&feed.id).await.unwrap();

    assert_eq!(summary.downloaded, 0);
    assert_eq!(summary.failed, 1);
    assert!(test.downloaded_files().is_empty());
}

#[tokio::test]
async fn pending_episode_is_retried_on_later_check() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    let feed_url = format!("{}/feed.xml", server.uri());
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_single_mp3(&server.uri())),
        )
        .mount(&server)
        .await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/episode-3.mp3"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_bytes("audio"))
        .mount(&server)
        .await;

    let feed = test.app.add_feed(&feed_url).await.unwrap();
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        test.db_path.to_string_lossy().replace('\\', "/")
    ))
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO episodes
         (id, feed_id, episode_key, raw_title, normalized_title, published_at, media_url, media_content_type, status, first_seen_at)
         VALUES ('pending-episode', ?, 'episode-3', 'Newest Episode', 'Newest Episode', '2026-06-29T10:00:00Z', ?, 'audio/mpeg', 'pending', '2026-06-29T10:00:00Z')",
    )
    .bind(&feed.id)
    .bind(format!("{}/episode-3.mp3", server.uri()))
    .execute(&pool)
    .await
    .unwrap();

    let summary = test.app.check_feed(&feed.id).await.unwrap();

    assert_eq!(summary.queued, 1);
    assert_eq!(summary.downloaded, 1);
    assert_eq!(test.downloaded_files().len(), 1);
}
