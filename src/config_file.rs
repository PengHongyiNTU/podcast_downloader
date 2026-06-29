use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::core::{CoreConfig, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FileConfig {
    pub database_path: PathBuf,
    pub download_dir: PathBuf,
    pub log_file_path: PathBuf,
    pub country: String,
    pub http_timeout_seconds: u64,
    pub user_agent: String,
    pub default_retention_limit: u32,
    pub max_concurrent_feed_fetches: usize,
    pub max_concurrent_downloads: usize,
    pub ensure_mp3: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ffmpeg_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mp3_encoder_path: Option<PathBuf>,
}

impl Default for FileConfig {
    fn default() -> Self {
        Self {
            database_path: PathBuf::from("podcasts.db"),
            download_dir: PathBuf::from("downloads"),
            log_file_path: PathBuf::from("podcast_downloader.log"),
            country: "US".to_string(),
            http_timeout_seconds: 30,
            user_agent: "podcast-downloader/0.1".to_string(),
            default_retention_limit: 4,
            max_concurrent_feed_fetches: 4,
            max_concurrent_downloads: 2,
            ensure_mp3: true,
            ffmpeg_path: None,
            mp3_encoder_path: None,
        }
    }
}

impl FileConfig {
    pub async fn load_or_create(path: &Path) -> Result<Self> {
        if !path.exists() {
            let config = Self::default();
            config.save(path).await?;
            return Ok(config);
        }

        let contents = tokio::fs::read_to_string(path).await?;
        let config = toml::from_str(&contents)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        Ok(config)
    }

    pub async fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let contents = toml::to_string_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        tokio::fs::write(path, contents).await?;
        Ok(())
    }

    pub fn into_core_config(self, base_dir: &Path) -> CoreConfig {
        let mut config = CoreConfig::new(
            resolve_path(base_dir, self.database_path),
            resolve_path(base_dir, self.download_dir),
        );
        config.log_file_path = Some(resolve_path(base_dir, self.log_file_path));
        config.country = self.country;
        config.http_timeout = Duration::from_secs(self.http_timeout_seconds);
        config.user_agent = self.user_agent;
        config.default_retention_limit = self.default_retention_limit;
        config.max_concurrent_feed_fetches = self.max_concurrent_feed_fetches;
        config.max_concurrent_downloads = self.max_concurrent_downloads;
        config.ensure_mp3 = self.ensure_mp3;
        config.mp3_encoder_path = self
            .ffmpeg_path
            .or(self.mp3_encoder_path)
            .map(|path| resolve_path(base_dir, path));
        config
    }

    pub fn set_detected_ffmpeg_path(&mut self) {
        if self.ffmpeg_path.is_none()
            && self.mp3_encoder_path.is_none()
            && let Some(path) = detected_ffmpeg_path()
        {
            self.ffmpeg_path = Some(path);
        }
    }
}

fn resolve_path(base_dir: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    }
}

fn detected_ffmpeg_path() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let winget = PathBuf::from(local)
        .join("Microsoft")
        .join("WinGet")
        .join("Packages")
        .join("Gyan.FFmpeg_Microsoft.Winget.Source_8wekyb3d8bbwe");
    let entries = std::fs::read_dir(winget).ok()?;
    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path().join("bin").join("ffmpeg.exe"))
        .find(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_resolved_from_base_dir() {
        let base = Path::new("C:/podcasts");
        let config = FileConfig {
            database_path: PathBuf::from("db.sqlite"),
            download_dir: PathBuf::from("library"),
            ..FileConfig::default()
        }
        .into_core_config(base);

        assert!(config.database_path.ends_with("db.sqlite"));
        assert!(config.download_dir.ends_with("library"));
    }
}
