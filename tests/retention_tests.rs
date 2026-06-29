mod common;

#[tokio::test]
async fn retention_deletes_older_downloaded_files() {
    let test = common::TestApp::new().await;
    let server = wiremock::MockServer::start().await;
    let feed_url = format!("{}/feed.xml", server.uri());
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/feed.xml"))
        .respond_with(
            wiremock::ResponseTemplate::new(200)
                .set_body_string(common::rss_with_media(&server.uri())),
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
    test.app
        .set_feed_retention(&feed.id, Some(1))
        .await
        .unwrap();
    test.app.check_feed(&feed.id).await.unwrap();
    let second = test.app.check_feed(&feed.id).await.unwrap();

    assert_eq!(second.deleted_by_retention, 1);
    assert_eq!(test.downloaded_files().len(), 1);
}

#[tokio::test]
async fn default_retention_keeps_latest_four_downloads() {
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
        .set_feed_retention(&feed.id, Some(5))
        .await
        .unwrap();
    test.app.check_feed(&feed.id).await.unwrap();
    assert_eq!(test.downloaded_files().len(), 5);

    test.app.set_feed_retention(&feed.id, None).await.unwrap();
    let retention = test.app.enforce_retention(&feed.id).await.unwrap();

    assert_eq!(retention.files_deleted, 1);
    assert_eq!(test.downloaded_files().len(), 4);
}
