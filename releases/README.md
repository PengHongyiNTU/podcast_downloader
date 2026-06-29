# Releases

This folder is for local release staging notes only.

Generated installers and archives are ignored by git. Build fresh installers with:

```powershell
pnpm tauri build
```

The build also compiles the terminal TUI binary and stages it in this folder:

```text
PodcastDownloaderTui-<version>-<target-triple>.exe
podcast-downloader-tui-<version>-<target-triple>
```

The canonical build output is:

```text
src-tauri/target/release/bundle/
```

For manual local testing, copy installers here with simplified filenames, but do not commit them.
The TUI executable is also bundled into the desktop installer as a Tauri sidecar. Automatic PATH registration is not enabled yet; for now run the staged TUI binary directly or add its folder to PATH manually.
