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
