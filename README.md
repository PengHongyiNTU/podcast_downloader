# Podcast Downloader

Rust podcast downloader with a reusable async core and Ratatui terminal UI.

Current scope:

- Apple podcast search and manual RSS subscriptions
- RSS/Atom feed checks with bounded async concurrency
- SQLite library state
- MP3-only downloads with FFmpeg conversion when needed
- Retention of latest downloaded episodes per feed
- Local `config.toml` settings and file logging through Rust's `log` facade

Run from the repository root:

```text
cargo run
```

Verify:

```text
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
