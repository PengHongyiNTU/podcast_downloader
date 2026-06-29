use serde::Deserialize;

use crate::{
    core::{PodcastSearchResult, Result},
    metadata::normalize_title,
};

#[derive(Debug, Deserialize)]
struct AppleResponse {
    results: Vec<ApplePodcast>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplePodcast {
    collection_name: Option<String>,
    track_name: Option<String>,
    artist_name: Option<String>,
    feed_url: Option<String>,
    artwork_url600: Option<String>,
    artwork_url100: Option<String>,
    collection_view_url: Option<String>,
    track_view_url: Option<String>,
}

pub async fn search_apple(
    client: &reqwest::Client,
    base_url: &str,
    country: &str,
    query: &str,
) -> Result<Vec<PodcastSearchResult>> {
    let response = client
        .get(base_url)
        .query(&[
            ("media", "podcast"),
            ("entity", "podcast"),
            ("term", query),
            ("country", country),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<AppleResponse>()
        .await?;

    Ok(response
        .results
        .into_iter()
        .map(|item| {
            let title = item
                .collection_name
                .or(item.track_name)
                .unwrap_or_else(|| "Untitled Podcast".to_string());
            PodcastSearchResult {
                title: normalize_title(&title),
                author: item.artist_name,
                feed_url: item.feed_url,
                artwork_url: item.artwork_url600.or(item.artwork_url100),
                apple_url: item.collection_view_url.or(item.track_view_url),
            }
        })
        .collect())
}
