use podcast_downloader::{CoreConfig, PodcastApp};

#[tokio::test]
async fn apple_search_returns_addable_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .and(wiremock::matchers::path("/search"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_string(
            r#"{
              "resultCount": 1,
              "results": [{
                "collectionName": "Example Show",
                "artistName": "Example Author",
                "feedUrl": "https://example.com/feed.xml",
                "artworkUrl600": "https://example.com/art.png",
                "collectionViewUrl": "https://podcasts.apple.com/example"
              }]
            }"#,
        ))
        .mount(&server)
        .await;

    let mut config = CoreConfig::new(
        temp.path().join("podcasts.db"),
        temp.path().join("downloads"),
    );
    config.apple_search_base_url = format!("{}/search", server.uri());
    let app = PodcastApp::open(config).await.unwrap();
    let results = app.search_podcasts("example").await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Example Show");
    assert_eq!(
        results[0].feed_url.as_deref(),
        Some("https://example.com/feed.xml")
    );
}
