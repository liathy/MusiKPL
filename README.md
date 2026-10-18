# MusiKPL 🎵

**A lightweight desktop music player built with Rust, powered by YouTube audio streaming.**

MusiKPL brings music discovery, queue management, automatic mixes, and synchronized listening with friends into a clean, minimalist desktop application.

<p align="center">
  <strong>Rust · egui · mpv · yt-dlp</strong>
</p>

---

## ✨ Features

- **YouTube Music Search** — Search for music and discover tracks through YouTube.
- **Audio Streaming** — Resolve and play audio using `yt-dlp` and `mpv`.
- **Playback Controls** — Play, pause, skip tracks, seek through songs, and adjust volume.
- **Queue Management** — Build and manage your upcoming listening session.
- **Automatic Mix** — Generate music mixes informed by your listening history.
- **LAN Jam** — Listen together over your local network with synchronized playback.
- **Host & Guest Permissions** — Hosts control playback while guests can contribute music to the queue.
- **Minimalist Interface** — A black theme with purple accents and a responsive layout.
- **Native Desktop Experience** — Built in Rust with `egui` / `eframe`.

## 🖥️ Screenshots

<!-- Add screenshots or GIFs of the application here. -->

<p align="center">
  <i>Screenshots coming soon.</i>
</p>

## 🚀 Getting Started

### Download

Download the latest version from the [**GitHub Releases**](https://github.com/liathy/MusiKPL/releases) page.

Check the release notes for version-specific requirements and installation instructions.

### Requirements

MusiKPL relies on the following tools:

- **Windows** — The primary release target.
- **mpv** — Audio playback engine.
- **yt-dlp** — YouTube media extraction.
- **Internet connection** — Required for searching and streaming online music.

Make sure `mpv` and `yt-dlp` are installed and available to the application if they are not bundled with your release.

## 🛠️ Built With

| Technology | Purpose |
| --- | --- |
| [Rust](https://www.rust-lang.org/) | Application logic and core functionality |
| [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui) | Desktop user interface |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Media search and audio stream resolution |
| [mpv](https://mpv.io/) | Audio playback |

## 🧑‍💻 Development

### Prerequisites

Install the following before building from source:

- [Rust toolchain](https://rustup.rs/)
- [Git](https://git-scm.com/)
- `mpv`
- `yt-dlp`

### Clone the repository

```bash
git clone https://github.com/liathy/MusiKPL.git
cd MusiKPL
```

### Run in development

```bash
cargo run
```

### Build a release

```bash
cargo build --release
```

The compiled executable will be located at:

```text
target/release/MusiKPL.exe
```

### Run tests

```bash
cargo test
```

### Format the code

```bash
cargo fmt
```

## 🎧 LAN Jam

MusiKPL includes a local-network listening feature designed to make listening together simple.

The intended experience is:

1. Discover an available listening session on your local network.
2. Join a session hosted by another user.
3. Listen to synchronized playback alongside the host.
4. Let the host control playback and track changes.
5. Add music to the host's queue as a guest.

Playback synchronization and discovery depend on the implementation and network environment of the version you're running.

## 🗺️ Roadmap

Potential areas for future development include:

- [ ] Further improve LAN session discovery and connection reliability.
- [ ] Refine playback synchronization across devices.
- [ ] Continue improving responsive layouts and visual polish.
- [ ] Expand music discovery and automatic mix recommendations.
- [ ] Improve installation and dependency management.

## 🤝 Contributing

Contributions, bug reports, and feature suggestions are welcome.

1. Fork the repository.
2. Create a feature branch.
3. Make your changes and test them.
4. Open a pull request describing your changes.

For bugs or feature requests, open an [issue](https://github.com/liathy/MusiKPL/issues).

## ⚠️ Disclaimer

MusiKPL is an independent, community-developed project and is not affiliated with, endorsed by, or officially associated with YouTube or Google.

Users are responsible for complying with applicable laws, platform terms, and the rights of content owners when accessing or playing media.

## 📄 License

See the repository's [LICENSE](LICENSE) file for licensing information.

---

<p align="center">
  Made with ❤️ and Rust.
</p>
