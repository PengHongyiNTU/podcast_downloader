use podcast_downloader::{AudioEncoderStatus, CoreConfig, PodcastApp};

#[tokio::test]
async fn reports_missing_configured_encoder() {
    let temp = tempfile::tempdir().unwrap();
    let mut config = CoreConfig::new(
        temp.path().join("podcasts.db"),
        temp.path().join("downloads"),
    );
    config.mp3_encoder_path = Some(temp.path().join("definitely-missing-ffmpeg.exe"));
    let app = PodcastApp::open(config).await.unwrap();

    let status = app.audio_encoder_status().await;

    assert!(matches!(status, AudioEncoderStatus::Missing { .. }));
}
