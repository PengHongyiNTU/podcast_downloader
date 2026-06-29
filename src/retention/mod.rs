use crate::{
    core::{FeedSubscription, Result, RetentionSummary},
    db::Db,
};

pub async fn enforce_feed_retention(
    db: &Db,
    feed: &FeedSubscription,
    default_limit: u32,
) -> Result<RetentionSummary> {
    let limit = feed.retention_limit.unwrap_or(default_limit) as usize;
    let downloaded = db.downloaded_episodes_for_feed(&feed.id).await?;
    let mut summary = RetentionSummary {
        feeds_checked: 1,
        ..RetentionSummary::default()
    };

    for episode in downloaded.into_iter().skip(limit) {
        if let Some(file_path) = episode.file_path.as_deref() {
            match tokio::fs::remove_file(file_path).await {
                Ok(()) => summary.files_deleted += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    summary.errors.push(format!(
                        "failed deleting retained file for episode {}: {error}",
                        episode.id
                    ));
                    continue;
                }
            }
        }
        db.mark_episode_deleted(&episode.id).await?;
    }

    Ok(summary)
}
