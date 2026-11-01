# MusiKPL 🎵

**A lightweight desktop music player built with Rust, powered by YouTube audio streaming.**

MusiKPL brings music discovery, queue management, automatic mixes, synchronized listening with friends, and Last.fm scrobbling into a clean, minimalist desktop application that ships as a single `MusiKPL.exe`.

<p align="center">
  <strong>Rust · egui · mpv · yt-dlp</strong>
</p>

---

## ✨ Features

- **Home** — MusiKPL opens on Home: rows of songs picked from what you listen to most ("Because you like …"), your Liked songs, a Discover row of artists you haven't heard yet, and Recently played. On first run you pick a few favorite genres (from pop and rock to hardstyle, hardcore, frenchcore, hardtekk, phonk, Russian rap and more). Home picks new songs every time you open MusiKPL or press Refresh, and it scrolls forever: rows load more songs as you reach their end, and more rows appear as you reach the bottom.
- **Your Taste, Kept Locally** — MusiKPL learns from how long you listen, what you finish, what you skip, and what you ♥ like, and looks up each song's genres on Last.fm. It all stays on your PC, in `%APPDATA%\MusiKPL`. Clear it any time in Settings.
- **YouTube Music Search** — Search for songs, artists, or albums, or paste a YouTube link. Songs appear as a grid of up to 20 cover cards, with matching **artists** (and a **person**, when you type an exact Last.fm username) above them. If YouTube's search answers nothing, MusiKPL tries again and then asks YouTube Music; if that still finds nothing, it lists the songs **Last.fm** knows, and each one is found on YouTube when you play it. The search box remembers your recent searches.
- **Artist Pages** — Open any artist for their photo, listeners, genres, bio, top songs (play them, or use **+** to like them, add them to the mix or to a playlist) and similar artists. Click the artist's name in the player bar to open their page: MusiKPL picks the most listened Last.fm artist matching the YouTube name, since Last.fm has many with similar names. **Bookmark** artists to keep them in the menu: they get their own Home rows ("From …", "Similar to …") and Auto Mix favours them.
- **Audio Streaming** — Best-quality audio resolved with `yt-dlp` and played with `mpv`. When YouTube won't play a video (age-restricted, blocked in your country, removed, or with no playable stream), MusiKPL plays another upload of the same song instead, and otherwise tells you why in plain words.
- **Playback Controls** — Play, pause, skip tracks, seek through songs, and adjust volume.
- **Cover Art** — Each song's YouTube cover, cropped square; it loads straight away and never changes.
- **Playlists** — Create playlists from the menu, add songs with **+** (or right-click) on any song, the queue, or the player bar, and drag songs to reorder them. Give a playlist your own cover photo, ♥ it to show it on Home, and get **Recommended songs** based on what's already in it. Deleting needs a 3-second hold, so it never happens by accident.
- **Like Songs** — Choose ♥ Like from a song's **+** menu, or press ♥ in the player bar or the queue.
- **Smooth Song Changes** — The next song in the queue is prepared in the background while the current one plays, so it starts right away.
- **Every Language** — Titles in Japanese, Chinese, Korean, Russian, Arabic, Thai, Hindi and more show properly, using the system's own fonts, loaded only when a title needs them.
- **Windows and Linux** — A Windows installer and portable exe, and a Linux binary for Ubuntu and other desktops.
- **Docked Menu and Queue** — The menu (left) and queue (right) open beside the music instead of over it, so you can keep browsing and playing with them open.
- **Picks Up Where You Left Off** — Your volume, Auto Mix choice, queue, and last song are remembered. The last song waits, paused, where you stopped; nothing loads until you press play.
- **Auto Mix** — On by default; switch it in **Settings → General**. Keeps the queue filled with songs like the one playing, leaning toward your taste: songs you keep skipping are left out and your favorite artists move up. It refills in the background before the queue runs out, falls back to the artist's songs when YouTube has no Mix, retries when a search fails, and skips songs you've already heard. Songs you add with **+** play first and are kept when you start a new mix.
- **LAN Jam** — Listen together over your local network with synchronized playback; join nearby Jams with one click. Works on PCs with VPN or virtual network adapters too.
- **Internet Jam** — Invite friends anywhere with a copy-paste invite: no account, no port forwarding. Connections are end-to-end encrypted, and only people with the invite can join.
- **Shared Jam Queue** — Everyone in a Jam sees the host's queue, and guests add songs to it with **+**. The host controls playback and is the only one who can remove songs.
- **Last.fm** — Connect in one click (or create an account) from **Settings → Last.fm**. MusiKPL scrobbles what you play, ♥ loves songs on your Last.fm profile too, and shows your **profile** from the menu: scrobbles, top artists and songs for any period, and recent plays.
- **Friends** — Add friends from their Last.fm profile, swap friend codes once, and see who has MusiKPL open. Friends show their Last.fm profile picture. Invite online friends to the Jam you're hosting in one click; their invite shows as a pop-up (or quietly in the menu, your choice in Settings). Peer-to-peer and encrypted, with no server; only people on your friends list can reach you.
- **Start with Windows** and **Close to tray** — Optional, from Settings. With close to tray, ✕ hides MusiKPL next to the clock and the music keeps playing.
- **Updates Built In** — Open the menu and press **Check for updates** at the bottom. When a new release is out, **Install update** downloads it, restarts MusiKPL on the new version, and keeps all your data. GitHub's address is looked up through Cloudflare's 1.1.1.1 (over HTTPS), so an internet provider blocking GitHub by DNS doesn't stop updates.
- **Back and Forward** — Move between pages like in a browser: the ‹ › buttons beside Home, your mouse's side buttons (back and forward), or Alt+← / Alt+→.
- **Minimalist Interface** — A black design with the Baloo Thambi typefaces, smooth hover animations, and long names that scroll instead of being cut off, everywhere (cards, queue, menu, lists). All scrolling text moves on one shared clock, so the app draws nothing while it pauses.
- **Native Desktop Experience** — Built in Rust with `egui` / `eframe`, with its own title bar: drag the top bar to move the window, double-click it (or use its button) to maximize, and drag the edges to resize.

## 🖥️ Screenshots

<!-- Add screenshots or GIFs of the application here. -->

<p align="center">
  <i>Screenshots coming soon.</i>
</p>

## 🚀 Getting Started

### Download

From the latest [**GitHub Release**](https://github.com/liathy/MusiKPL/releases), download either:

- **`MusiKPL-Setup.exe`** (recommended) — installs MusiKPL for your Windows account only, with no administrator rights, into `%LOCALAPPDATA%\Programs\MusiKPL`. You choose whether to add Start menu and Desktop shortcuts. It appears in **Settings → Apps → Installed apps**, where you can uninstall it; the uninstaller asks whether to delete your data too (it keeps it unless you tick the box).
- **`MusiKPL.exe`** — the portable app: run it from anywhere, no install.

Both update themselves from the menu's **Check for updates**. Your data is never touched by installing, updating, or reinstalling. If MusiKPL is open when you run the setup, the setup closes it first (it saves everything as it closes).

### Linux (Ubuntu and others)

Download **`MusiKPL-linux-x86_64`** from the same release. It runs on Ubuntu 20.04 and newer (and other 64-bit Linux desktops, X11 or Wayland). It needs the system's `mpv`; MusiKPL downloads its own up-to-date `yt-dlp` on first start and keeps it updated (Ubuntu's own yt-dlp package is too old for YouTube):

```bash
sudo apt install mpv
chmod +x MusiKPL-linux-x86_64
./MusiKPL-linux-x86_64
```

Optional extras: `sudo apt install zenity` to choose playlist cover photos, and `fonts-noto-cjk` for Japanese, Chinese and Korean titles. **Check for updates** updates the Linux binary in place, **Settings → Start on login** adds MusiKPL to your desktop's autostart, and your data lives in `~/.local/share/MusiKPL`.

### Requirements

- **Windows 10/11** or **64-bit Linux**.
- **Internet connection** — Required for searching and streaming online music.

`mpv` and `yt-dlp` are bundled inside `MusiKPL.exe`. On first launch they are unpacked once to `%LOCALAPPDATA%\MusiKPL\tools` and reused afterwards; a newer MusiKPL release replaces them. The unpacked `yt-dlp` updates itself weekly, because YouTube changes often. To use your own versions instead, place `mpv.exe` and/or `yt-dlp.exe` next to `MusiKPL.exe`.

### Settings

Everything MusiKPL remembers is in `%APPDATA%\MusiKPL` (on Linux, `~/.local/share/MusiKPL`), as small JSON files:

| File | What it holds |
| --- | --- |
| `settings.json` | Settings, volume, Auto Mix, recent searches, favorite genres, Last.fm connection |
| `taste.json` | Listening history and likes (up to 1,500 songs; liked songs are always kept) |
| `session.json` | The last song, where you stopped, and the queue |
| `home.json` | The latest Home, so it opens instantly |
| `playlists.json` | Your playlists |
| `covers\` | Cover photos you chose for playlists (small square copies) |
| `artists.json` | Bookmarked artists |
| `artist_images.json` | Which YouTube picture is each artist's photo |
| `friends.json` | Your friends |
| `friend-key.txt` | This PC's private key behind your friend code (keep it private) |

Genres, artist pages and profiles come from Last.fm with MusiKPL's built-in key, so they work without an account.

To connect your Last.fm account (scrobbling, loves, your profile):

1. In MusiKPL, open **Settings → Last.fm** and press **Connect with Last.fm** (or **Create a Last.fm account** first).
2. Approve MusiKPL in your browser. Your username fills in automatically; your password is never entered in MusiKPL.

Prefer your own API key? Choose **Use my own API key instead** on the same page.

## 🪶 Lightweight by Design

- The window opens at the design's 1440 × 1024 proportions, as large as fits your screen, and every size is in rem units, so the layout scales with the window (down to 960 × 680).
- The app draws only when something changes: about 0 % CPU when idle and around 5 % of one core while a long title scrolls, with frames capped at 60 per second.
- `yt-dlp` runs only for a few seconds per search or song lookup, and `mpv` only while music plays.
- Nothing is drawn while the window is minimized; playback, Jams, and scrobbling keep running.
- Internet Jam networking starts only while an internet Jam is active and stops when it ends. Friends use one background thread, started only once you have a friend with a code, and check who's online only every minute while the menu is open (every 5 minutes otherwise).
- Home is built by one `yt-dlp` search at a time and opens instantly from its saved copy. Covers load only for cards scrolled into view, and YouTube Mixes are read only as far as needed.
- Extra fonts for other languages load only when a title needs them.
- Your data is saved in the background, at most every 10 seconds and when the app closes.

## 🛠️ Built With

| Technology | Purpose |
| --- | --- |
| [Rust](https://www.rust-lang.org/) | Application logic and core functionality |
| [egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui) | Desktop user interface |
| [yt-dlp](https://github.com/yt-dlp/yt-dlp) | Media search and audio stream resolution |
| [mpv](https://mpv.io/) | Audio playback |
| [Last.fm API](https://www.last.fm/api) | Scrobbling and genres |
| [iroh](https://www.iroh.computer/) | Encrypted peer-to-peer connections for internet Jams |

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

To build both release files (`MusiKPL.exe` and `MusiKPL-Setup.exe`) into `dist`:

```powershell
.\scripts\build-release.ps1
```

Put MusiKPL's Last.fm API key in `lastfm-api-key.txt` in the project root first (ignored by git), or set `MUSIKPL_LASTFM_KEY`. It is built into the app for genres. Builds without it still work; users then need their own key for those.

For trying changes, `.\scripts\build-release.ps1 -Quick` builds only `dist\MusiKPL.exe` in seconds instead of minutes (no whole-program optimization; releases don't use it).

To build the Linux binary (`dist\MusiKPL-linux-x86_64`) from Windows, start Docker Desktop and run:

```powershell
.\scripts\build-linux.ps1
```

On Linux itself, `cargo build --release -p MusiKPL` works directly.

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
cargo fmt --all -- --check
cargo clippy --all-targets --workspace
cargo test --workspace
```

The setup lives in `installer/` (a workspace member). It embeds `target/release/MusiKPL.exe`, so build MusiKPL first.

### Project layout

| Folder | What's in it |
|---|---|
| `core/` | `musikpl-core`: the app without drawing. Its `Engine` holds what plays, the queue and Auto Mix, search, Home, playlists, Last.fm, Jams and friends; a shell ticks it, draws its state and calls its methods. Shared with the Android app. Platforms plug in through `AudioOutput` (mpv, ExoPlayer) and `Extractor` (yt-dlp, NewPipe). |
| `src/` | The desktop shell: the egui interface around the engine, mpv and yt-dlp, the tray, updates, Windows and Linux integration. |
| `installer/` | The Windows setup. |

## 🎧 Jam

Listen together in sync, on your home network or over the internet. Everyone streams the audio themselves; the Jam only keeps playback in sync.

### On your network (LAN Jam)

1. Open the menu and press **Start** to host a Jam, or press **Join** on a nearby Jam.
2. Guests hear the host's music in sync; the host controls playback and track changes.
3. Everyone sees the same queue. Guests press **+** on a song to add it; only the host can remove songs.

### Over the internet (Internet Jam)

1. Everyone uses MusiKPL 3.0.0 or newer (Jams between 3.x and older versions don't connect).
2. The host starts a Jam, then under **Invite over the internet** presses **Create invite** and **Copy invite**, and sends the `musikpl:…` text to friends.
3. Friends paste it under **Join over the internet** in the menu and press **Join**.

Internet Jams use [iroh](https://www.iroh.computer/): devices connect directly when their routers allow it, and through iroh's public relay servers otherwise. Every connection is end-to-end encrypted (relays only forward encrypted data), and only MusiKPL's Jam protocol is accepted. Each Jam gets a new secret inside its invite, so only people with the invite can join, and the invite stops working when the Jam ends. Anyone who has the invite can join while the Jam runs, so share it only with people you want to listen with.

### If a Jam doesn't show up

- Windows Firewall blocks Jams for a copy of MusiKPL it has no rule for, or allows it only on Private networks. Press **Allow through firewall** in the menu's Jam section (shown when no Jams are found, or while you host a Jam nobody has joined) and accept the Windows prompt. Do this on **both** PCs.
- Some routers, often on guest Wi-Fi, isolate devices from each other, which blocks LAN Jams. Use an internet Jam instead.

## 🗺️ Roadmap

Potential areas for future development include:

- [ ] Further improve LAN session discovery and connection reliability.
- [ ] Refine playback synchronization across devices.
- [ ] Optionally run a dedicated relay server for internet Jams.
- [x] Home with recommendations from your listening, and taste-aware Auto Mix.
- [x] Installer and built-in updates.
- [ ] Expand music discovery further (more Home rows, mood and time-of-day mixes).
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

MusiKPL is free software, released under the [GNU General Public License v3.0 or later](LICENSE): you may use, study, share and change it, and anything built from it must stay under the same license. Baloo Thambi and Baloo Thambi 2 are by Ek Type, under the SIL Open Font License (`src/assets/fonts/LICENSE.txt`).

---

<p align="center">
  Made with ❤️ and Rust.
</p>
