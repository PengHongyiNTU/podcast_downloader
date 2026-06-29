use sha2::{Digest, Sha256};
use url::Url;

pub fn normalize_title(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = compact.trim();
    if normalized.is_empty() {
        "Untitled".to_string()
    } else {
        normalized.to_string()
    }
}

pub fn filename_component(value: &str, max_chars: usize) -> String {
    let normalized = normalize_title(value);
    let sanitized = sanitize_filename::sanitize(normalized);
    let compact = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let safe = compact.trim_matches(['.', ' ']).trim();
    let fallback = if safe.is_empty() { "Untitled" } else { safe };
    truncate_chars(fallback, max_chars)
}

pub fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    format!("{digest:x}").chars().take(8).collect()
}

pub fn episode_key(guid: Option<&str>, media_url: &str) -> String {
    guid.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("media:{}", short_hash(media_url)))
}

pub fn date_prefix(published_at: Option<&str>) -> String {
    published_at
        .and_then(|value| value.get(0..10))
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown-date")
        .to_string()
}

pub fn media_extension(media_url: &str, content_type: Option<&str>) -> String {
    if let Ok(url) = Url::parse(media_url)
        && let Some(segment) = url
            .path_segments()
            .and_then(|mut segments| segments.next_back())
        && let Some((_, extension)) = segment.rsplit_once('.')
    {
        let clean = extension
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        if matches!(
            clean.as_str(),
            "mp3" | "m4a" | "mp4" | "aac" | "ogg" | "wav"
        ) {
            return clean;
        }
    }

    match content_type
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "audio/mp4" | "audio/x-m4a" => "m4a".to_string(),
        "video/mp4" => "mp4".to_string(),
        "audio/aac" => "aac".to_string(),
        "audio/ogg" | "application/ogg" => "ogg".to_string(),
        "audio/wav" | "audio/x-wav" => "wav".to_string(),
        _ => "unknown".to_string(),
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    let mut output = String::new();
    for ch in value.chars().take(max_chars) {
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_title("  A   Good\tEpisode  "), "A Good Episode");
    }

    #[test]
    fn derives_episode_key_from_guid_or_media_url() {
        assert_eq!(episode_key(Some("abc"), "https://example.com/a.mp3"), "abc");
        assert!(episode_key(None, "https://example.com/a.mp3").starts_with("media:"));
    }

    #[test]
    fn selects_extension_from_url_then_content_type_then_default() {
        assert_eq!(
            media_extension("https://example.com/a.M4A?x=1", None),
            "m4a"
        );
        assert_eq!(
            media_extension("https://example.com/a", Some("video/mp4")),
            "mp4"
        );
        assert_eq!(media_extension("https://example.com/a", None), "unknown");
    }

    #[test]
    fn sanitizes_and_truncates_filename_components() {
        let value = filename_component(" bad:/name with lots of text ", 8);
        assert_eq!(value.chars().count(), 8);
        assert!(!value.contains(':'));
    }
}
