# Releases

This folder is for local release staging notes only.

Generated installers and archives are ignored by git. Build fresh installers with:

```powershell
pnpm tauri build
```

The canonical build output is:

```text
src-tauri/target/release/bundle/
```

For manual local testing, copy installers here with simplified filenames, but do not commit them.
