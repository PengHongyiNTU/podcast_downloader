mod common;

#[tokio::test]
async fn first_check_downloads_only_latest_and_second_check_does_not_duplicate() {
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
    let first = test.app.check_feed(&feed.id).await.unwrap();
    assert_eq!(first.queued, 1);
    assert_eq!(first.downloaded, 1);
    assert_eq!(first.skipped_initial, 2);
    assert_eq!(test.downloaded_files().len(), 1);

    let second = test.app.check_feed(&feed.id).await.unwrap();
    assert_eq!(second.queued, 0);
    assert_eq!(second.downloaded, 0);
    assert_eq!(test.downloaded_files().len(), 1);
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
                    .set_body_string(common::rss_with_media(&server.uri())),
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
