use podcast_downloader::{PodcastApp, Result, config_file::FileConfig, logging, tui};

#[tokio::main]
async fn main() -> Result<()> {
    let cwd = std::env::current_dir()?;
    let config_path = cwd.join("config.toml");
    let mut file_config = FileConfig::load_or_create(&config_path).await?;
    file_config.set_detected_ffmpeg_path();
    file_config.save(&config_path).await?;
    let config = file_config.into_core_config(&cwd);
    logging::init_file_logger(config.log_file_path.as_deref())?;
    let app = PodcastApp::open(config).await?;
    tui::run(app).await?;
    Ok(())
}
