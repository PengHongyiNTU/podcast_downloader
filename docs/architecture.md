# Architecture

The project is currently a single Rust package.

- `src/core` defines public types, config, errors, summaries, and progress events.
- `src/app` coordinates discovery, feed checks, downloads, retention, and core logging.
- `src/db` owns SQLite schema and persistence.
- `src/discovery` searches Apple Podcasts.
- `src/feeds` parses RSS and Atom.
- `src/downloads` streams media to disk, reports progress, and invokes conversion.
- `src/decoder` detects media type and shells out to FFmpeg for MP3 conversion.
- `src/metadata` normalizes titles and filenames.
- `src/retention` deletes older downloaded files according to feed limits.
- `src/logging.rs` initializes the file logger backend for Rust's `log` facade.
- `src/tui` is the Ratatui frontend, split into app/render flow, progress state, and UI helpers.
- `src/config_file.rs` maps local `config.toml` into `CoreConfig`.

The core is exposed through `src/lib.rs` so a future Tauri frontend can call it directly.
The current executable launches the TUI.
