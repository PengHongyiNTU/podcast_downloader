# Podcast Downloader

A podcast subscription manager and downloader purpose-built for offline listening on devices with no internet connectivity, such as swimming earpods that function as USB MP3 players. Subscribe to podcasts, automatically download new episodes as MP3 files, and copy them to your device.

- **Rust async core** — reusable library for feed management, downloads, and retention
- **Ratatui terminal UI** — keyboard-driven, works over SSH and on headless servers
- **Tauri v2 desktop app** — React + TypeScript + Tailwind + shadcn/ui

## Motivation

Many waterproof earpods (e.g. for swimming) lack Bluetooth or Wi-Fi and instead behave like a USB mass storage device that plays MP3 files in alphabetical order. This tool automates the workflow of subscribing to podcasts, downloading episodes, converting audio to MP3 when needed, and maintaining a local library ready to sync to your device.

## Features

- Search Apple Podcasts via the iTunes API
- Subscribe to any RSS/Atom podcast feed
- Parse feeds and extract episode metadata and enclosures
- Download episodes with streaming progress
- Convert non-MP3 audio to MP3 via FFmpeg (configurable per feed)
- Enforce per-feed retention limits (auto-delete old episodes)
- Concurrent feed checking and downloads
- SQLite persistence
- Global and per-feed settings via `config.toml`
- File logging

## Requirements

| Dependency | Purpose |
|---|---|
| Rust (stable, via `rustup`) | Core library, TUI, and desktop backend |
| Node.js ≥18 + pnpm | Desktop frontend and Tauri bundling |
| FFmpeg | MP3 conversion for non-MP3 podcast enclosures |

FFmpeg must be on your PATH or its path set in `config.toml`. On Windows the app auto-detects FFmpeg via Winget if configured.

## Quick Start

### Terminal UI (all platforms)

```bash
cargo run
```

### Desktop app (all platforms)

```bash
pnpm install
pnpm tauri dev
```

The terminal UI is also available as a standalone binary — see [Releases](https://github.com/PengHongyiNTU/podcast_downloader/releases) for prebuilt Windows executables.

## Building

### Desktop installer

```bash
pnpm install
pnpm tauri build
```

Output: `src-tauri/target/release/bundle/` (`.msi` and `.exe` on Windows, `.deb`/`.AppImage` on Linux, `.dmg` on macOS).

### Linux

Install system dependencies for Tauri:

**Debian/Ubuntu:**
```bash
sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libasound2-dev
```

**Fedora:**
```bash
sudo dnf install webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel patchelf alsa-lib-devel
```

**Arch:**
```bash
sudo pacman -S webkit2gtk-4.1 libappindicator-gtk3 librsvg patchelf alsa-lib
```

Then follow the Quick Start or Building steps above.

### macOS

Install Xcode Command Line Tools, then follow the Quick Start or Building steps. Tauri uses `cargo-tauri` under the hood and will handle macOS-specific bundling automatically (requires an Apple Developer account for signing).

### TUI-only binary

To build just the terminal UI (no desktop frontend):

```bash
cargo build --release --bin podcast_downloader
```

The binary is at `target/release/podcast_downloader` (or `.exe` on Windows).

## Verify

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
pnpm typecheck
pnpm test
pnpm build
```

## Local Data

The app creates the following files in the working directory (all gitignored):

| File | Purpose |
|---|---|
| `config.toml` | App settings (download dir, concurrency, FFmpeg path, etc.) |
| `podcasts.db` | SQLite library database |
| `downloads/` | Downloaded MP3 files |
| `podcast_downloader.log` | File logs |

## Repository Layout

```text
podcast_downloader/
├── src/                 # Rust core, TUI, config, logging, SQLite, downloads
├── src-tauri/           # Tauri v2 desktop shell and command bridge
├── ui/                  # React + TypeScript frontend
├── tests/               # Rust integration tests and fixtures
├── docs/                # Architecture documentation
├── scripts/             # Build scripts (TUI sidecar)
└── packaging/           # Platform packaging stubs
```

## License

MIT
