# Architecture

The project is a single Rust core package with two frontends: a Ratatui terminal UI and a Tauri desktop app.

## Core Rust Package

- `src/core` defines public DTOs, config, errors, summaries, episode statuses, feed previews, and progress events.
- `src/app` coordinates discovery, feed previews, subscriptions, feed checks, downloads, retention, and logging.
- `src/db` owns SQLite schema and persistence.
- `src/discovery` searches Apple Podcasts.
- `src/feeds` parses RSS and Atom.
- `src/downloads` streams media to disk, reports progress, and invokes conversion.
- `src/decoder` detects media type and shells out to FFmpeg for MP3 conversion.
- `src/metadata` normalizes titles and filenames.
- `src/retention` deletes older downloaded files according to feed limits.
- `src/config_file.rs` maps local `config.toml` into `CoreConfig`.
- `src/logging.rs` initializes the file logger backend for Rust's `log` facade.
- `src/tui` contains the Ratatui frontend.

The public API is exported from `src/lib.rs` so frontends call the same behavior instead of duplicating podcast logic.

## Tauri Desktop App

- `src-tauri` is the Tauri v2 shell.
- `src-tauri/src/lib.rs` owns desktop app state, command handlers, task locking, progress event forwarding, config save/reopen behavior, and the downloads-folder opener.
- Tauri commands return frontend-safe DTOs or `AppErrorDto`.
- Long-running sync/download work emits `podcast-progress`, `podcast-task-started`, and `podcast-task-finished` events.

The desktop app stores its config and database in Tauri app data by default, while the TUI uses the repository working directory when launched with `cargo run`.

## React Frontend

- `ui/App.tsx` contains the desktop shell, app state, and shadcn-based views.
- `ui/api.ts` wraps Tauri command calls.
- `ui/types.ts` mirrors serializable Rust DTOs.
- `ui/progress.ts` reduces download/feed progress events into renderable UI state.
- `ui/components/ui` contains local shadcn/ui-style primitives.
- `ui/styles.css` defines Tailwind v4 theme tokens, shadcn CSS variables, automatic light/dark mode, and minimal app shell CSS.

The UI uses shadcn-style primitives for controls and cards, with Tailwind utilities for layout.

## Data Flow

1. Frontend invokes a Tauri command.
2. Tauri command calls `PodcastApp`.
3. `PodcastApp` reads/writes SQLite and performs HTTP/download work.
4. Long-running work emits `DownloadProgress` through a Tokio channel.
5. Tauri forwards progress to the frontend as events.
6. React updates the progress model and refreshes snapshots after task completion.

## Generated And Local Files

Ignored local outputs include:

- `target/`
- `src-tauri/target/`
- `src-tauri/gen/`
- `node_modules/`
- `dist/`
- `config.toml`
- database files
- logs
- downloaded audio
- release installers
