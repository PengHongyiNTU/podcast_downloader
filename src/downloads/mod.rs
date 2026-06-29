use std::path::{Path, PathBuf};

use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::{
    core::{CoreConfig, EpisodeRecord, Result},
    db::Db,
    decoder::{AudioFormat, classify_audio_format, convert_to_mp3},
    metadata::{date_prefix, filename_component, media_extension, short_hash},
};

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub feed_id: String,
    pub feed_title: String,
    pub episode: EpisodeRecord,
}

pub async fn download_job(
    config: &CoreConfig,
    db: &Db,
    client: &reqwest::Client,
    job: &DownloadJob,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(&config.download_dir).await?;
    let attempt_id = db.start_attempt(&job.episode.id).await?;
    db.mark_episode_pending(&job.episode.id).await?;

    let result = download_job_inner(config, client, job).await;
    match result {
        Ok(path) => {
            db.mark_episode_downloaded(&job.episode.id, &path).await?;
            db.finish_attempt(&attempt_id, "downloaded", None).await?;
            Ok(path)
        }
        Err(error) => {
            let message = error.to_string();
            db.mark_episode_failed(&job.episode.id, &message).await?;
            db.finish_attempt(&attempt_id, "failed", Some(&message))
                .await?;
            Err(error)
        }
    }
}

async fn download_job_inner(
    config: &CoreConfig,
    client: &reqwest::Client,
    job: &DownloadJob,
) -> Result<PathBuf> {
    let response = client
        .get(&job.episode.media_url)
        .send()
        .await?
        .error_for_status()?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
        .or_else(|| job.episode.media_content_type.clone());
    let source_extension = media_extension(&job.episode.media_url, content_type.as_deref());
    let needs_conversion = config.ensure_mp3
        && classify_audio_format(&source_extension, content_type.as_deref()) != AudioFormat::Mp3;
    let final_extension = if config.ensure_mp3 {
        "mp3"
    } else {
        source_extension.as_str()
    };
    let filename = base_filename(job, final_extension);
    let reservation = reserve_unique_paths(
        &config.download_dir,
        &filename,
        &job.episode.episode_key,
        if needs_conversion {
            &source_extension
        } else {
            final_extension
        },
    )
    .await?;

    let target = reservation.target.clone();
    let part_path = reservation.part_path.clone();
    let lock_path = reservation.lock_path.clone();
    let converted_part = part_path.with_extension("mp3.converted.part");

    let result = async {
        let mut file = tokio::fs::File::create(&part_path).await?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            file.write_all(&chunk?).await?;
        }
        file.flush().await?;
        drop(file);

        if needs_conversion {
            convert_to_mp3(
                config.mp3_encoder_path.as_deref(),
                &part_path,
                &converted_part,
            )
            .await?;
            let _ = tokio::fs::remove_file(&part_path).await;
            tokio::fs::rename(&converted_part, &target).await?;
        } else {
            tokio::fs::rename(&part_path, &target).await?;
        }

        Ok(target)
    }
    .await;

    let _ = tokio::fs::remove_file(&lock_path).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&part_path).await;
        let _ = tokio::fs::remove_file(&converted_part).await;
    }

    result
}

fn base_filename(job: &DownloadJob, extension: &str) -> String {
    format!(
        "{} - {} - {}.{}",
        filename_component(&job.feed_title, 70),
        date_prefix(job.episode.published_at.as_deref()),
        filename_component(&job.episode.normalized_title, 120),
        extension
    )
}

#[derive(Debug)]
struct PathReservation {
    target: PathBuf,
    part_path: PathBuf,
    lock_path: PathBuf,
}

async fn reserve_unique_paths(
    download_dir: &Path,
    filename: &str,
    key: &str,
    source_extension: &str,
) -> Result<PathReservation> {
    let hash = short_hash(key);
    let (stem, extension) = filename.rsplit_once('.').unwrap_or((filename, "mp3"));
    let candidates = std::iter::once(filename.to_string())
        .chain((0..100).map(|index| {
            if index == 0 {
                format!("{stem} - {hash}.{extension}")
            } else {
                format!("{stem} - {hash}-{index}.{extension}")
            }
        }))
        .collect::<Vec<_>>();

    for candidate in candidates {
        let target = download_dir.join(candidate);
        if target.exists() {
            continue;
        }

        let lock_path = target.with_extension(format!("{extension}.lock"));
        match tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .await
        {
            Ok(_) => {
                let part_path =
                    target.with_extension(format!("{source_extension}.{}.part", Uuid::new_v4()));
                return Ok(PathReservation {
                    target,
                    part_path,
                    lock_path,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        format!("could not reserve unique filename for {filename}"),
    )
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{EpisodeRecord, EpisodeStatus};

    #[test]
    fn builds_flat_filename_with_show_prefix() {
        let job = DownloadJob {
            feed_id: "feed".to_string(),
            feed_title: "Show: Name".to_string(),
            episode: EpisodeRecord {
                id: "episode".to_string(),
                feed_id: "feed".to_string(),
                episode_key: "key".to_string(),
                raw_title: "Episode".to_string(),
                normalized_title: "Episode / One".to_string(),
                raw_author: None,
                published_at: Some("2026-06-29T00:00:00Z".to_string()),
                media_url: "https://example.com/e.mp3".to_string(),
                media_content_type: None,
                media_length_bytes: None,
                status: EpisodeStatus::Pending,
                file_path: None,
                first_seen_at: "now".to_string(),
                downloaded_at: None,
                deleted_at: None,
                last_error: None,
            },
        };

        let filename = base_filename(&job, "mp3");
        assert!(filename.starts_with("Show Name - 2026-06-29 - Episode One"));
        assert!(filename.ends_with(".mp3"));
    }
}
