mod common;

use podcast_downloader::PodcastError;

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
