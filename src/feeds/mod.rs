use chrono::Utc;
use feed_rs::model::{Entry, Feed, Link};

use crate::{
    core::{PodcastError, Result},
    metadata::{episode_key, normalize_title},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFeed {
    pub raw_title: String,
    pub normalized_title: String,
    pub site_url: Option<String>,
    pub description: Option<String>,
    pub artwork_url: Option<String>,
    pub episodes: Vec<ParsedEpisode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedEpisode {
    pub episode_key: String,
    pub raw_title: String,
    pub normalized_title: String,
    pub raw_author: Option<String>,
    pub published_at: Option<String>,
    pub media_url: String,
    pub media_content_type: Option<String>,
    pub media_length_bytes: Option<i64>,
}

pub fn parse_feed(body: &[u8]) -> Result<ParsedFeed> {
    let feed =
        feed_rs::parser::parse(body).map_err(|err| PodcastError::FeedParse(err.to_string()))?;
    let raw_title = feed
        .title
        .as_ref()
        .map(|title| title.content.clone())
        .unwrap_or_else(|| "Untitled Podcast".to_string());
    let episodes = feed
        .entries
        .iter()
        .filter_map(parse_entry)
        .collect::<Vec<_>>();

    Ok(ParsedFeed {
        raw_title: raw_title.clone(),
        normalized_title: normalize_title(&raw_title),
        site_url: first_alternate_link(&feed),
        description: feed.description.as_ref().map(|text| text.content.clone()),
        artwork_url: feed.icon.or(feed.logo).map(|image| image.uri),
        episodes,
    })
}

fn parse_entry(entry: &Entry) -> Option<ParsedEpisode> {
    let media = find_media(entry)?;
    let raw_title = entry
        .title
        .as_ref()
        .map(|title| title.content.clone())
        .unwrap_or_else(|| "Untitled Episode".to_string());
    let published_at = entry
        .published
        .or(entry.updated)
        .map(|date| date.with_timezone(&Utc).to_rfc3339());
    let raw_author = entry.authors.first().map(|person| person.name.clone());
    let content_type = media.content_type;
    let media_length_bytes = media.length.and_then(|length| i64::try_from(length).ok());
    let key = episode_key(Some(&entry.id), &media.url);

    Some(ParsedEpisode {
        episode_key: key,
        raw_title: raw_title.clone(),
        normalized_title: normalize_title(&raw_title),
        raw_author,
        published_at,
        media_url: media.url,
        media_content_type: content_type,
        media_length_bytes,
    })
}

fn first_alternate_link(feed: &Feed) -> Option<String> {
    feed.links
        .iter()
        .find(|link| link.rel.as_deref().unwrap_or("alternate") == "alternate")
        .or_else(|| feed.links.first())
        .map(|link| link.href.clone())
}

fn is_media_link(link: &Link) -> bool {
    if link.rel.as_deref() == Some("enclosure") {
        return true;
    }

    link.media_type
        .as_ref()
        .map(ToString::to_string)
        .map(|value| {
            let value = value.to_ascii_lowercase();
            value.starts_with("audio/") || value.starts_with("video/")
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
struct MediaRef {
    url: String,
    content_type: Option<String>,
    length: Option<u64>,
}

fn find_media(entry: &Entry) -> Option<MediaRef> {
    if let Some(link) = entry.links.iter().find(|link| is_media_link(link)) {
        return Some(MediaRef {
            url: link.href.clone(),
            content_type: link.media_type.clone(),
            length: link.length,
        });
    }

    if let Some(content) = entry
        .content
        .as_ref()
        .and_then(|content| content.src.as_ref())
        && is_media_link(content)
    {
        return Some(MediaRef {
            url: content.href.clone(),
            content_type: content.media_type.clone(),
            length: content.length,
        });
    }

    entry
        .media
        .iter()
        .flat_map(|media| media.content.iter())
        .find_map(|content| {
            let url = content.url.as_ref()?.to_string();
            let content_type = content.content_type.as_ref().map(ToString::to_string);
            let is_media = content_type
                .as_deref()
                .map(|value| {
                    let value = value.to_ascii_lowercase();
                    value.starts_with("audio/") || value.starts_with("video/")
                })
                .unwrap_or(true);
            is_media.then_some(MediaRef {
                url,
                content_type,
                length: content.size,
            })
        })
}

pub fn sort_newest_first(episodes: &mut [ParsedEpisode]) {
    episodes.sort_by(|left, right| {
        right
            .published_at
            .cmp(&left.published_at)
            .then_with(|| left.raw_title.cmp(&right.raw_title))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rss_feed_with_enclosure() {
        let parsed = parse_feed(include_bytes!("../../tests/fixtures/basic_rss.xml")).unwrap();
        assert_eq!(parsed.normalized_title, "Example Show");
        assert_eq!(parsed.episodes.len(), 3);
        assert_eq!(
            parsed.episodes[0].media_content_type.as_deref(),
            Some("audio/mpeg")
        );
    }

    #[test]
    fn parses_atom_feed_with_audio_link() {
        let parsed = parse_feed(include_bytes!("../../tests/fixtures/basic_atom.xml")).unwrap();
        assert_eq!(parsed.normalized_title, "Atom Show");
        assert_eq!(parsed.episodes.len(), 1);
    }
}
