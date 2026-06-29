#![allow(dead_code)]

use std::{fs, path::PathBuf};

use podcast_downloader::{CoreConfig, PodcastApp};
use tempfile::TempDir;

pub struct TestApp {
    pub app: PodcastApp,
    pub _temp: TempDir,
    pub downloads: PathBuf,
    pub db_path: PathBuf,
}

impl TestApp {
    pub async fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let downloads = temp.path().join("downloads");
        let db_path = temp.path().join("podcasts.db");
        let config = CoreConfig::new(&db_path, &downloads);
        let app = PodcastApp::open(config).await.unwrap();
        Self {
            app,
            _temp: temp,
            downloads,
            db_path,
        }
    }

    pub fn downloaded_files(&self) -> Vec<PathBuf> {
        let mut files = fs::read_dir(&self.downloads)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.is_file())
            .collect::<Vec<_>>();
        files.sort();
        files
    }
}

pub fn rss_with_media(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Example Show</title>
    <link>https://example.com/show</link>
    <description>A show used in tests.</description>
    <item>
      <guid>episode-3</guid>
      <title>Newest Episode</title>
      <pubDate>Mon, 29 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-3.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <guid>episode-2</guid>
      <title>Middle Episode</title>
      <pubDate>Sun, 28 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-2.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <guid>episode-1</guid>
      <title>Oldest Episode</title>
      <pubDate>Sat, 27 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-1.mp3" length="123" type="audio/mpeg"/>
    </item>
  </channel>
</rss>"#
    )
}

pub fn rss_with_five_media(base: &str) -> String {
    let items = (1..=5)
        .rev()
        .map(|index| {
            let day = 24 + index;
            format!(
                r#"
    <item>
      <guid>episode-{index}</guid>
      <title>Episode {index}</title>
      <pubDate>Mon, {day} Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-{index}.mp3" length="123" type="audio/mpeg"/>
    </item>"#
            )
        })
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Five Episode Show</title>
    <link>https://example.com/show</link>
    <description>A show used in tests.</description>
    {items}
  </channel>
</rss>"#
    )
}

pub fn rss_with_single_mp3(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Single Episode Show</title>
    <link>https://example.com/show</link>
    <description>A show used in tests.</description>
    <item>
      <guid>episode-3</guid>
      <title>Newest Episode</title>
      <pubDate>Mon, 29 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-3.mp3" length="123" type="audio/mpeg"/>
    </item>
  </channel>
</rss>"#
    )
}

pub fn rss_with_extra_new_episode(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Example Show</title>
    <link>https://example.com/show</link>
    <description>A show used in tests.</description>
    <item>
      <guid>episode-4</guid>
      <title>Brand New Episode</title>
      <pubDate>Tue, 30 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-4.mp3" length="123" type="audio/mpeg"/>
    </item>
    <item>
      <guid>episode-3</guid>
      <title>Newest Episode</title>
      <pubDate>Mon, 29 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-3.mp3" length="123" type="audio/mpeg"/>
    </item>
  </channel>
</rss>"#
    )
}

pub fn rss_with_m4a(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>M4A Show</title>
    <link>https://example.com/show</link>
    <description>A show used in tests.</description>
    <item>
      <guid>m4a-episode-1</guid>
      <title>Only M4A Episode</title>
      <pubDate>Mon, 29 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-1.m4a" length="123" type="audio/mp4"/>
    </item>
  </channel>
</rss>"#
    )
}

pub fn rss_with_unknown_media(base: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0">
  <channel>
    <title>Unknown Media Show</title>
    <link>https://example.com/show</link>
    <description>A show used in tests.</description>
    <item>
      <guid>unknown-episode-1</guid>
      <title>Unknown Media Episode</title>
      <pubDate>Mon, 29 Jun 2026 10:00:00 GMT</pubDate>
      <enclosure url="{base}/episode-download" length="123"/>
    </item>
  </channel>
</rss>"#
    )
}
