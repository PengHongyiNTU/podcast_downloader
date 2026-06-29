# Podcast Downloader

Podcast Downloader is a Rust podcast library app with:

- A reusable async Rust core
- A Ratatui terminal UI
- A Tauri v2 desktop app built with React, TypeScript, Tailwind, and shadcn/ui-style components

The app can search Apple Podcasts, subscribe to RSS/Atom feeds, check feeds concurrently, download episodes, convert non-MP3 audio to MP3 through FFmpeg, enforce per-feed retention, and persist library state in SQLite.

## Current Features

- Apple Podcasts search with addable RSS feed candidates
- RSS/Atom feed ingestion and preview
- Watched podcast library with feed metadata and episode history
- Manual and automatic feed checks
- Bounded concurrent feed fetches and downloads
- Download progress events for the desktop UI
- MP3-only library output with FFmpeg conversion when configured
- SQLite persistence
- Global and per-feed retention limits
- Local `config.toml` settings
- File logging through Rust's `log` facade
- Windows Tauri installers through MSI and NSIS

## Repository Layout

```text
podcast_downloader/
  src/                 Rust core, TUI, config, logging, SQLite, downloads
  src-tauri/           Tauri v2 desktop shell and command bridge
  ui/                  React + TypeScript frontend
  tests/               Rust integration tests and fixtures
  docs/                Architecture notes
  releases/            Local release staging notes; generated binaries are ignored
```

## Requirements

- Rust via `rustup`
- Node.js and pnpm
- FFmpeg for non-MP3 downloads when `ensure_mp3 = true`

In this Codex desktop workspace, Node and pnpm are available through the bundled runtime. If your shell does not have Node/Cargo on PATH, use:

```powershell
$env:PATH = "C:\Users\admin\.cargo\bin;C:\Users\admin\.cache\codex-runtimes\codex-primary-runtime\dependencies\node\bin;C:\Users\admin\.cache\codex-runtimes\codex-primary-runtime\dependencies\bin;$env:PATH"
```

## Run

Install frontend dependencies:

```powershell
pnpm install
```

Run the Tauri desktop app in debug mode:

```powershell
pnpm tauri dev
```

Run the terminal UI:

```powershell
cargo run
```

## Build Installers

```powershell
pnpm tauri build
```

Build outputs are generated under:

```text
src-tauri/target/release/bundle/
```

For local testing, installers can be copied into `releases/`, but generated release binaries are intentionally ignored by git.

## Verify

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
pnpm typecheck
pnpm test
pnpm build
```

Run `pnpm tauri build` before cutting a release to verify installer packaging and icons.

## Local Data

The app creates local data such as:

- `config.toml`
- SQLite database files
- downloaded audio files
- log files
- Tauri/frontend build outputs

These are ignored by git.
