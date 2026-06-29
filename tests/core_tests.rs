mod common;

use podcast_downloader::{
    AppErrorDto, CoreConfig, DownloadBatchSummary, DownloadProgress, EpisodeStatus, PodcastApp,
    PodcastError, logging,
};

#[tokio::test]
async fn schema_initializes_and_feeds_can_be_added_listed_removed() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_media(&server.uri())),
        )
        .mount(&server)
        .await;

    let feed = test
        .app
        .add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();
    assert_eq!(feed.normalized_title, "Example Show");
    assert_eq!(test.app.list_feeds().await.unwrap().len(), 1);

    test.app.remove_feed(&feed.id).await.unwrap();
    assert!(test.app.list_feeds().await.unwrap().is_empty());
}

#[tokio::test]
async fn duplicate_feed_urls_are_rejected() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    let feed_url = format!("{}/feed.xml", server.uri());
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_media(&server.uri())),
        )
        .mount(&server)
        .await;

    test.app.add_feed(&feed_url).await.unwrap();
    let error = test.app.add_feed(&feed_url).await.unwrap_err();
    assert!(matches!(error, PodcastError::DuplicateFeed(_)));
}

#[tokio::test]
async fn missing_feed_mutations_return_not_found() {
    let test = common::TestApp::new().await;

    let remove_error = test.app.remove_feed("missing").await.unwrap_err();
    assert!(matches!(remove_error, PodcastError::NotFound(_)));

    let retention_error = test
        .app
        .set_feed_retention("missing", Some(1))
        .await
        .unwrap_err();
    assert!(matches!(retention_error, PodcastError::NotFound(_)));
}

#[tokio::test]
async fn core_logger_writes_level_and_event_fields() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = CoreConfig::new(
        temp.path().join("podcasts.db"),
        temp.path().join("downloads"),
    );
    let log_path = temp.path().join("podcast_downloader.log");
    config.log_file_path = Some(log_path.clone());
    logging::init_file_logger(config.log_file_path.as_deref()).unwrap();
    let app = PodcastApp::open(config).await.unwrap();
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_media(&server.uri())),
        )
        .mount(&server)
        .await;

    app.add_feed(&format!("{}/feed.xml", server.uri()))
        .await
        .unwrap();

    let log = std::fs::read_to_string(log_path).unwrap();
    assert!(log.contains("INFO"));
    assert!(log.contains("event=app.open"));
    assert!(log.contains("event=feed.add.start"));
    assert!(log.contains("event=feed.add.finish"));
}

#[test]
fn frontend_dtos_are_json_serializable() {
    let status = serde_json::to_string(&EpisodeStatus::SkippedInitial).unwrap();
    assert_eq!(status, "\"skipped_initial\"");

    let progress = DownloadProgress::DownloadAdvanced {
        feed_id: "feed".to_string(),
        episode_id: "episode".to_string(),
        episode_title: "Episode".to_string(),
        downloaded_bytes: 10,
        total_bytes: Some(20),
    };
    let progress_json = serde_json::to_string(&progress).unwrap();
    assert!(progress_json.contains("\"type\":\"download_advanced\""));
    assert!(progress_json.contains("\"downloaded_bytes\":10"));

    let batch = DownloadBatchSummary {
        requested: 2,
        queued: 1,
        downloaded: 1,
        failed: 1,
        ..DownloadBatchSummary::default()
    };
    let batch_json = serde_json::to_string(&batch).unwrap();
    assert!(batch_json.contains("\"requested\":2"));

    let error = AppErrorDto::from(PodcastError::NotFound("episode".to_string()));
    let error_json = serde_json::to_string(&error).unwrap();
    assert!(error_json.contains("\"kind\":\"not_found\""));
    assert!(error_json.contains("episode"));
}
