# MusiKPL 🎵

**A lightweight desktop music player built with Rust, powered by YouTube audio streaming.**

MusiKPL brings music discovery, queue management, automatic mixes, synchronized listening with friends, and Last.fm scrobbling into a clean, minimalist desktop application that ships as a single `MusiKPL.exe`.

<p align="center">
  <strong>Rust · egui · mpv · yt-dlp</strong>
</p>

---

## ✨ Features

- **YouTube Music Search** — Search for songs, artists, or albums, or paste a YouTube link. Results appear as a grid of cover cards.
- **Audio Streaming** — Best-quality audio resolved with `yt-dlp` and played with `mpv`.
- **Playback Controls** — Play, pause, skip tracks, seek through songs, and adjust volume.
- **Cover Art** — YouTube thumbnails, or square album art from Last.fm when it knows the song.
- **Queue Management** — A queue drawer to see what's playing and up next, play any song, or remove it.
- **Auto Mix** — Keeps the queue filled with songs like the one playing. It refills in the background before the queue runs out, falls back to the artist's songs when YouTube has no Mix, retries when a search fails, and skips songs you've already heard. Songs you add with **+** play first and are kept when you start a new mix.
- **LAN Jam** — Listen together over your local network with synchronized playback; join nearby Jams with one click.
- **Host & Guest Permissions** — Hosts control playback while guests can request music for the queue.
- **Last.fm Scrobbling** — Now playing and scrobbles, following Last.fm's rules.
- **Start with Windows** — Optional, from Settings.
- **Minimalist Interface** — A black design with the Baloo Thambi typefaces, smooth hover animations, and scrolling titles for long names.
- **Native Desktop Experience** — Built in Rust with `egui` / `eframe`, with its own title bar: drag the top bar to move the window, and use its minimize and close buttons.

## 🖥️ Screenshots

<!-- Add screenshots or GIFs of the application here. -->

<p align="center">
  <i>Screenshots coming soon.</i>
</p>

## 🚀 Getting Started

### Download

Download `MusiKPL.exe` from the latest [**GitHub Release**](https://github.com/liathy/MusiKPL/releases) and run it. No installer or extra files are needed.

### Requirements

- **Windows** — The release target.
- **Internet connection** — Required for searching and streaming online music.

`mpv` and `yt-dlp` are bundled inside `MusiKPL.exe`. On first launch they are unpacked once to `%LOCALAPPDATA%\MusiKPL\tools` and reused afterwards; a newer MusiKPL release replaces them. The unpacked `yt-dlp` updates itself weekly, because YouTube changes often. To use your own versions instead, place `mpv.exe` and/or `yt-dlp.exe` next to `MusiKPL.exe`.

### Settings

Settings are saved in `%APPDATA%\MusiKPL\settings.json`.

To scrobble to Last.fm:

1. Create an API account at https://www.last.fm/api/account/create.
2. In MusiKPL, open the menu, then **Settings → Last.fm**, and enter the API key and shared secret.
3. Press **Connect** and approve MusiKPL in your browser. Your username fills in automatically; your password is never entered in MusiKPL.

## 🪶 Lightweight by Design

- The window has a fixed size: at startup it takes the design's 1440 × 1024 proportions, as large as fits your screen, and every size is in rem units, so the whole layout scales with the screen.
- The app draws only when something changes: about 0 % CPU when idle and around 5 % of one core while a long title scrolls, with frames capped at 60 per second.
- `yt-dlp` runs only for a few seconds per search or song lookup, and `mpv` only while music plays.
- Nothing is drawn while the window is minimized; playback, Jams, and scrobbling keep running.

## 🛠️ Built With

| Technology | Purpose |
| --- | --- |
| [Rust](https://www.rust-lang.org/) | Application logic and core functionality |
| [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui) | Desktop user interface |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Media search and audio stream resolution |
| [mpv](https://mpv.io/) | Audio playback |
| [Last.fm API](https://www.last.fm/api) | Scrobbling and album art |

## 🧑‍💻 Development

### Prerequisites

Install the following before building from source:

- [Rust toolchain](https://rustup.rs/)
- [Git](https://git-scm.com/)
- `mpv.exe` and `yt-dlp.exe` in the project root (the setup script downloads them)

### Clone the repository

```bash
git clone https://github.com/liathy/MusiKPL.git
cd MusiKPL
```

### Set up and build

```powershell
Set-ExecutionPolicy -Scope Process Bypass
.\scripts\install-dependencies.ps1 -Build
```

The script installs Rust if needed, downloads the latest official `yt-dlp` and a current 64-bit Windows `mpv` build into the project root, builds the release, and puts the single `MusiKPL.exe` in `dist`. Omit `-Build` to only install the dependencies.

### Run in development

```bash
cargo run
```

Debug builds use the `mpv.exe` and `yt-dlp.exe` in the project root.

### Build a release

```bash
cargo build --release
```

Release builds bundle the project-root `mpv.exe` and `yt-dlp.exe` into the executable, located at:

```text
target/release/MusiKPL.exe
```

### Check the code

```bash
cargo fmt -- --check
cargo clippy --all-targets
cargo test
```

## 🎧 LAN Jam

MusiKPL includes a local-network listening feature designed to make listening together simple:

1. Open the menu and press **Start** to host a Jam, or press **Join** on a nearby Jam.
2. Guests hear the host's music in sync; the host controls playback and track changes.
3. Guests press **+** on a song to request it, and the host accepts or declines.

Discovery depends on your network: networks that block broadcast or isolate devices may not show nearby Jams.

## 🗺️ Roadmap

Potential areas for future development include:

- [ ] Further improve LAN session discovery and connection reliability.
- [ ] Refine playback synchronization across devices.
- [ ] Expand music discovery and automatic mix recommendations.
- [ ] Add screenshots to this README.

## 🤝 Contributing

Contributions, bug reports, and feature suggestions are welcome.

1. Fork the repository.
2. Create a feature branch.
3. Make your changes and test them.
4. Open a pull request describing your changes.

For bugs or feature requests, open an [issue](https://github.com/liathy/MusiKPL/issues).

## ⚠️ Disclaimer

MusiKPL is an independent, community-developed project and is not affiliated with, endorsed by, or officially associated with YouTube, Google, or Last.fm.

Users are responsible for complying with applicable laws, platform terms, and the rights of content owners when accessing or playing media.

## 📄 License

See the repository's [LICENSE](LICENSE) file for licensing information. Baloo Thambi and Baloo Thambi 2 are by Ek Type, under the SIL Open Font License (`src/assets/fonts/LICENSE.txt`).

---

<p align="center">
  Made with ❤️ and Rust.
</p>
