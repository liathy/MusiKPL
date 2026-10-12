# YTMusic

YTMusic is a small native Windows audio player for YouTube. It has no browser UI, Chromium, Electron, WebView2, UWP dependency, or embedded web page. The GUI is Rust/egui; playback is delegated to `mpv.exe`, and YouTube search/extraction is handled by `yt-dlp.exe`.

## Why Rust

Rust produces a fast native executable with low idle memory use and no managed runtime requirement. `egui` keeps the UI dependency small, while mpv handles decoding, buffering, codecs, and the highest available YouTube audio format.

## Run in development

Run the included PowerShell setup script from the project directory:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\install-dependencies.ps1 -Build
```

The script installs Rust through the official rustup installer, downloads the latest official yt-dlp Windows executable, finds a current 64-bit Windows mpv build, and creates a portable `dist` folder. You can omit `-Build` if you only want to install the dependencies.

`mpv.exe` should be a normal Windows build of mpv. `yt-dlp.exe` should be the official standalone Windows executable. Keep both updated because YouTube changes frequently.

## Portable release layout

```text
YTMusic.exe
mpv.exe
yt-dlp.exe
```

The application uses `--ytdl-format=bestaudio/best`, so mpv asks yt-dlp for the best audio-only stream available. The app does not download or store media files.

## Current scope

The first native implementation includes YouTube search, URL lookup, audio-only playback, pause/stop/next/previous, a queue, volume, and automatic mix expansion based on the currently playing song. It intentionally leaves credentials, recommendations, and web-based UI out of the design.
