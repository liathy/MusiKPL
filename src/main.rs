#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

#[cfg(not(windows))]
use std::{fs::OpenOptions, io::Write};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

static NEXT_PIPE_ID: AtomicU64 = AtomicU64::new(1);

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
}

#[derive(Clone)]
struct ProcessRegistry {
    pids: Arc<Mutex<Vec<u32>>>,
    closing: Arc<AtomicBool>,
}

impl Default for ProcessRegistry {
    fn default() -> Self {
        Self {
            pids: Arc::new(Mutex::new(Vec::new())),
            closing: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl ProcessRegistry {
    fn add(&self, pid: u32) {
        if let Ok(mut pids) = self.pids.lock() {
            pids.push(pid);
        }
    }

    fn remove(&self, pid: u32) {
        if let Ok(mut pids) = self.pids.lock() {
            pids.retain(|registered| *registered != pid);
        }
    }

    fn terminate_all(&self) {
        self.closing.store(true, Ordering::SeqCst);
        let pids = self
            .pids
            .lock()
            .map(|pids| pids.clone())
            .unwrap_or_default();
        for pid in pids {
            let mut command = Command::new("taskkill");
            hide_console(&mut command);
            let _ = command
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .status();
        }
        if let Ok(mut registered) = self.pids.lock() {
            registered.clear();
        }
    }
}

fn run_registered(mut command: Command, registry: &ProcessRegistry) -> Result<Output, String> {
    if registry.closing.load(Ordering::SeqCst) {
        return Err("Application is closing".into());
    }
    hide_console(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start yt-dlp: {e}"))?;
    let pid = child.id();
    registry.add(pid);
    if registry.closing.load(Ordering::SeqCst) {
        let _ = child.kill();
        let _ = child.wait();
        registry.remove(pid);
        return Err("Application is closing".into());
    }
    let result = child
        .wait_with_output()
        .map_err(|e| format!("Could not read yt-dlp output: {e}"));
    registry.remove(pid);
    result
}

#[derive(Clone, Debug)]
struct Track {
    title: String,
    id: String,
    duration: Option<f64>,
    channel: String,
}

impl Track {
    fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }

    fn duration_text(&self) -> String {
        self.duration
            .map(format_duration)
            .unwrap_or_else(|| "--:--".into())
    }
}

#[derive(Debug, Deserialize)]
struct YtEntry {
    id: Option<String>,
    title: Option<String>,
    duration: Option<f64>,
    channel: Option<String>,
    uploader: Option<String>,
    webpage_url: Option<String>,
}

struct Player {
    child: Option<Child>,
    ipc_path: Option<String>,
    started: Option<Instant>,
    paused: bool,
    volume: u8,
    registry: ProcessRegistry,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            child: None,
            ipc_path: None,
            started: None,
            paused: false,
            volume: 100,
            registry: ProcessRegistry::default(),
        }
    }
}

impl Player {
    fn send(&mut self, command: &str) {
        if let Some(path) = &self.ipc_path {
            let payload = format!("{{\"command\":{command}}}\n");
            #[cfg(windows)]
            write_windows_pipe(path, payload.as_bytes());
            #[cfg(not(windows))]
            if let Ok(mut pipe) = OpenOptions::new().write(true).open(path) {
                let _ = pipe.write_all(payload.as_bytes());
                let _ = pipe.flush();
            }
        }
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            let _ = child.kill();
            let _ = child.wait();
            self.registry.remove(pid);
        }
        self.ipc_path = None;
        self.started = None;
        self.paused = false;
    }

    fn play_stream(
        &mut self,
        _track: &Track,
        stream_url: &str,
        tools: &Tools,
    ) -> Result<(), String> {
        self.stop();
        let pipe_name = format!(
            "\\\\.\\pipe\\ytmusic-{}-{}",
            std::process::id(),
            NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed)
        );
        let mut command = Command::new(&tools.mpv);
        hide_console(&mut command);
        command
            .arg("--no-video")
            .arg("--force-window=no")
            .arg("--audio-display=no")
            .arg("--idle=no")
            .arg(format!("--input-ipc-server={pipe_name}"))
            .arg(format!("--volume={}", self.volume))
            .arg(stream_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        let child = command
            .spawn()
            .map_err(|e| format!("Could not start mpv at {}: {e}", tools.mpv.display()))?;
        self.registry.add(child.id());
        self.child = Some(child);
        self.ipc_path = Some(pipe_name);
        self.started = Some(Instant::now());
        Ok(())
    }

    fn is_finished(&mut self) -> bool {
        self.child
            .as_mut()
            .map(|c| c.try_wait().ok().flatten().is_some())
            .unwrap_or(false)
    }
}

#[cfg(windows)]
fn write_windows_pipe(path: &str, payload: &[u8]) {
    use std::ptr::null_mut;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{
            CreateFileW, WriteFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_WRITE, FILE_SHARE_READ,
            FILE_SHARE_WRITE, OPEN_EXISTING,
        },
    };

    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let handle = CreateFileW(
            wide.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            null_mut(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            0,
        );
        if handle == INVALID_HANDLE_VALUE {
            return;
        }
        let mut written = 0u32;
        let _ = WriteFile(
            handle,
            payload.as_ptr(),
            payload.len() as u32,
            &mut written,
            null_mut(),
        );
        CloseHandle(handle);
    }
}

#[derive(Clone)]
struct Tools {
    mpv: PathBuf,
    yt_dlp: PathBuf,
}

impl Tools {
    fn discover() -> Self {
        let base = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            mpv: base.join("mpv.exe"),
            yt_dlp: base.join("yt-dlp.exe"),
        }
    }

    fn ready(&self) -> Result<(), String> {
        if !self.mpv.exists() || !self.yt_dlp.exists() {
            return Err(format!(
                "Place mpv.exe and yt-dlp.exe next to the YTMusic executable. Missing: {}{}",
                if self.mpv.exists() { "" } else { "mpv.exe " },
                if self.yt_dlp.exists() {
                    ""
                } else {
                    "yt-dlp.exe"
                }
            ));
        }
        Ok(())
    }
}

fn query_ytdlp(
    tools: &Tools,
    query: &str,
    single: bool,
    cookie_browser: CookieBrowser,
    registry: &ProcessRegistry,
) -> Result<Vec<Track>, String> {
    tools.ready()?;
    let target = if single {
        query.to_string()
    } else {
        format!("ytsearch8:{query}")
    };
    let mut command = Command::new(&tools.yt_dlp);
    hide_console(&mut command);
    add_cookie_args(&mut command, cookie_browser)?;
    command.args([
        "--dump-json",
        "--skip-download",
        "--flat-playlist",
        "--no-warnings",
        &target,
    ]);
    let output = run_registered(command, registry)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let entries = parse_ytdlp_entries(&output.stdout, &output.stderr)?;
    Ok(entries
        .into_iter()
        .filter_map(|e| {
            let id = e.id.or_else(|| {
                e.webpage_url
                    .and_then(|u| u.split("v=").nth(1).map(str::to_owned))
            })?;
            Some(Track {
                title: e.title.unwrap_or_else(|| "Untitled".into()),
                id,
                duration: e.duration,
                channel: e.channel.or(e.uploader).unwrap_or_default(),
            })
        })
        .collect())
}

fn parse_ytdlp_entries(stdout: &[u8], stderr: &[u8]) -> Result<Vec<YtEntry>, String> {
    if stdout.iter().all(u8::is_ascii_whitespace) {
        let message = String::from_utf8_lossy(stderr).trim().to_owned();
        return Err(if message.is_empty() {
            "yt-dlp returned no search data".into()
        } else {
            message
        });
    }

    let entries: Vec<YtEntry> = String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if entries.is_empty() {
        return Err("yt-dlp returned no usable search results".into());
    }
    Ok(entries)
}

fn resolve_audio_url(
    tools: &Tools,
    track: &Track,
    cookie_browser: CookieBrowser,
    registry: &ProcessRegistry,
) -> Result<String, String> {
    tools.ready()?;
    let mut command = Command::new(&tools.yt_dlp);
    hide_console(&mut command);
    add_cookie_args(&mut command, cookie_browser)?;
    command.args([
        "--get-url",
        "--format",
        "bestaudio/best",
        "--no-playlist",
        "--no-warnings",
        &track.url(),
    ]);
    if cookie_browser == CookieBrowser::None {
        command.args(["--extractor-args", "youtube:player_client=android"]);
    }
    let output = run_registered(command, registry)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("http"))
        .map(str::to_owned)
        .ok_or_else(|| "yt-dlp did not return an audio stream".into())
}

fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

fn youtube_mix_url(track: &Track) -> String {
    format!(
        "https://www.youtube.com/watch?v={}&list=RD{}",
        track.id, track.id
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CookieBrowser {
    None,
    Chrome,
    Edge,
    Firefox,
    Helium,
}

impl Default for CookieBrowser {
    fn default() -> Self {
        Self::None
    }
}

impl CookieBrowser {
    fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
            Self::Helium => "Helium",
        }
    }

    fn yt_dlp_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Chrome => Some("chrome"),
            Self::Edge => Some("edge"),
            Self::Firefox => Some("firefox"),
            Self::Helium => None,
        }
    }
}

fn helium_cookie_profile() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        let candidates = [
            local_app_data.join("net.imput.helium").join("User Data"),
            local_app_data
                .join("imput")
                .join("Helium")
                .join("User Data"),
            local_app_data.join("Helium").join("User Data"),
        ];
        candidates.into_iter().find(|path| path.exists())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn add_cookie_args(command: &mut Command, browser: CookieBrowser) -> Result<(), String> {
    if browser == CookieBrowser::Helium {
        let profile = helium_cookie_profile().ok_or_else(|| -> String {
            "Helium profile was not found. Sign in to Helium first, or verify its User Data folder."
                .into()
        })?;
        command.args([
            "--cookies-from-browser",
            &format!("chromium:{}", profile.display()),
        ]);
    } else if let Some(name) = browser.yt_dlp_name() {
        command.args(["--cookies-from-browser", name]);
    }
    Ok(())
}

struct YtMusicApp {
    tools: Tools,
    registry: ProcessRegistry,
    player: Player,
    search: String,
    results: Vec<Track>,
    queue: VecDeque<Track>,
    history: Vec<Track>,
    current: Option<Track>,
    status: String,
    error: Option<String>,
    auto_mix: bool,
    cookie_browser: CookieBrowser,
    searching: bool,
    loading: bool,
    mixing: bool,
    finished_handled: bool,
    search_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    mix_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    play_rx: Option<Receiver<Result<(Track, String, bool), String>>>,
}

impl Default for YtMusicApp {
    fn default() -> Self {
        let registry = ProcessRegistry::default();
        let mut player = Player::default();
        player.registry = registry.clone();
        Self {
            tools: Tools::discover(),
            registry,
            player,
            search: String::new(),
            results: vec![],
            queue: VecDeque::new(),
            history: vec![],
            current: None,
            status: "Ready".into(),
            error: None,
            auto_mix: true,
            cookie_browser: CookieBrowser::default(),
            searching: false,
            loading: false,
            mixing: false,
            finished_handled: false,
            search_rx: None,
            mix_rx: None,
            play_rx: None,
        }
    }
}

impl YtMusicApp {
    fn search(&mut self) {
        let query = self.search.trim().to_owned();
        if query.is_empty() {
            return;
        }
        if self.searching {
            return;
        }
        self.searching = true;
        self.error = None;
        let tools = self.tools.clone();
        let single = query.starts_with("http://") || query.starts_with("https://");
        let cookie_browser = self.cookie_browser;
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        self.search_rx = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(query_ytdlp(
                &tools,
                &query,
                single,
                cookie_browser,
                &registry,
            ));
        });
    }

    fn play_track(&mut self, track: Track) {
        self.start_track(track, true);
    }

    fn start_track(&mut self, track: Track, remember_current: bool) {
        self.player.stop();
        self.loading = true;
        self.error = None;
        self.status = "Loading audio...".into();
        self.finished_handled = false;
        let tools = self.tools.clone();
        let cookie_browser = self.cookie_browser;
        let (sender, receiver) = mpsc::channel();
        self.play_rx = Some(receiver);
        let registry = self.registry.clone();
        std::thread::spawn(move || {
            let result = resolve_audio_url(&tools, &track, cookie_browser, &registry)
                .map(|url| (track, url, remember_current));
            let _ = sender.send(result);
        });
    }

    fn next(&mut self) {
        if let Some(track) = self.queue.pop_front() {
            self.play_track(track);
        } else {
            self.player.stop();
            self.current = None;
            self.status = "Queue finished".into();
            self.play_rx = None;
            self.loading = false;
        }
    }

    fn previous(&mut self) {
        if let Some(track) = self.history.pop() {
            if let Some(current) = self.current.take() {
                self.queue.push_front(current);
            }
            self.start_track(track, false);
        }
    }

    fn add_mix(&mut self, track: &Track) {
        if self.mixing {
            return;
        }
        let query = youtube_mix_url(track);
        let tools = self.tools.clone();
        let cookie_browser = self.cookie_browser;
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        self.mix_rx = Some(receiver);
        self.mixing = true;
        self.status = "Building My Mix...".into();
        std::thread::spawn(move || {
            let _ = sender.send(query_ytdlp(&tools, &query, true, cookie_browser, &registry));
        });
    }

    fn add_mix_items(&mut self, items: Vec<Track>) {
        let current_id = self.current.as_ref().map(|track| track.id.as_str());
        for item in items {
            if current_id == Some(item.id.as_str())
                || self.queue.iter().any(|queued| queued.id == item.id)
                || self.history.iter().any(|played| played.id == item.id)
            {
                continue;
            }
            self.queue.push_back(item);
            if self.queue.len() >= 8 {
                break;
            }
        }
    }

    fn poll_background_work(&mut self) {
        let search_result = self.search_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = search_result {
            self.search_rx = None;
            self.searching = false;
            match result {
                Ok(items) => {
                    self.results = items;
                    self.status = format!("{} result(s)", self.results.len());
                }
                Err(e) => {
                    self.status = "Search failed".into();
                    self.error = Some(e);
                }
            }
        }

        let mix_result = self.mix_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = mix_result {
            self.mix_rx = None;
            self.mixing = false;
            match result {
                Ok(items) => {
                    self.add_mix_items(items);
                    self.status = if self.queue.is_empty() {
                        "My Mix returned no new songs".into()
                    } else {
                        format!("My Mix ready  ·  {} queued", self.queue.len())
                    };
                    if self.finished_handled && self.player.is_finished() {
                        self.next();
                    }
                }
                Err(e) => {
                    self.status = "My Mix failed".into();
                    self.error = Some(format!("Could not create mix: {e}"));
                }
            }
        }

        let play_result = self.play_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = play_result {
            self.play_rx = None;
            self.loading = false;
            match result {
                Ok((track, url, remember_current)) => {
                    if remember_current {
                        if let Some(old) = self.current.replace(track.clone()) {
                            self.history.push(old);
                        }
                    } else {
                        self.current = Some(track.clone());
                    }
                    match self.player.play_stream(&track, &url, &self.tools) {
                        Ok(()) => {
                            self.status = "Playing".into();
                            self.error = None;
                            self.finished_handled = false;
                            if self.auto_mix {
                                self.add_mix(&track);
                            }
                        }
                        Err(e) => {
                            self.status = "Playback failed".into();
                            self.error = Some(e);
                        }
                    }
                }
                Err(e) => {
                    self.status = "Playback failed".into();
                    self.error = Some(e);
                }
            }
        }
    }
}

impl Drop for YtMusicApp {
    fn drop(&mut self) {
        self.player.stop();
        self.registry.terminate_all();
    }
}

impl eframe::App for YtMusicApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(egui::Color32::from_rgb(235, 234, 242));
        visuals.panel_fill = egui::Color32::from_rgb(4, 4, 6);
        visuals.window_fill = egui::Color32::from_rgb(12, 12, 16);
        visuals.faint_bg_color = egui::Color32::from_rgb(10, 10, 13);
        visuals.extreme_bg_color = egui::Color32::from_rgb(0, 0, 0);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(14, 14, 18);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(20, 20, 26);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(38, 36, 50);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(71, 63, 105);
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(10.0, 8.0);
            style.spacing.button_padding = egui::vec2(14.0, 9.0);
        });

        self.poll_background_work();
        if self.player.is_finished() && !self.finished_handled && !self.loading {
            self.finished_handled = true;
            if self.auto_mix && self.queue.len() < 2 {
                if let Some(current) = self.current.clone() {
                    self.add_mix(&current);
                }
            }
            if !self.queue.is_empty() || !self.auto_mix {
                self.next();
            }
        }
        ctx.request_repaint_after(Duration::from_millis(250));

        egui::TopBottomPanel::top("top")
            .exact_height(76.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("YT")
                            .size(24.0)
                            .strong()
                            .color(egui::Color32::from_rgb(190, 177, 255)),
                    );
                    ui.vertical(|ui| {
                        ui.label(egui::RichText::new("YTMusic").size(18.0).strong());
                        ui.small("Audio only  /  native player");
                    });
                    ui.add_space(24.0);
                    let response = ui.add_sized(
                        [440.0, 36.0],
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text("Search songs, artists, or paste a YouTube URL"),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        self.search();
                    }
                    if ui
                        .add_enabled(
                            !self.searching,
                            egui::Button::new(if self.searching {
                                "Searching"
                            } else {
                                "Search"
                            }),
                        )
                        .clicked()
                    {
                        self.search();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new("OLED MODE")
                                .size(11.0)
                                .color(egui::Color32::from_rgb(167, 157, 214)),
                        );
                    });
                });
            });

        egui::SidePanel::left("queue")
            .resizable(false)
            .default_width(188.0)
            .show(ctx, |ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("LIBRARY")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(157, 148, 184)),
                );
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Home").size(15.0).strong());
                ui.label(
                    egui::RichText::new(format!("Queue  {}", self.queue.len()))
                        .color(egui::Color32::from_rgb(190, 177, 255)),
                );
                ui.separator();
                ui.checkbox(&mut self.auto_mix, "Auto mix");
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("YOUTUBE ACCESS")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(157, 148, 184)),
                );
                egui::ComboBox::from_id_salt("cookie_browser")
                    .selected_text(self.cookie_browser.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.cookie_browser, CookieBrowser::None, "Off");
                        ui.selectable_value(
                            &mut self.cookie_browser,
                            CookieBrowser::Chrome,
                            "Chrome cookies",
                        );
                        ui.selectable_value(
                            &mut self.cookie_browser,
                            CookieBrowser::Edge,
                            "Edge cookies",
                        );
                        ui.selectable_value(
                            &mut self.cookie_browser,
                            CookieBrowser::Firefox,
                            "Firefox cookies",
                        );
                        ui.selectable_value(
                            &mut self.cookie_browser,
                            CookieBrowser::Helium,
                            "Helium cookies",
                        );
                    });
                ui.small("Used only by yt-dlp when selected");
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new("PLAYBACK")
                        .size(11.0)
                        .color(egui::Color32::from_rgb(157, 148, 184)),
                );
                ui.small("Best available audio");
                ui.small("No video / no browser");
                ui.add_space(18.0);
                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(255, 135, 155), err);
                }
            });

        egui::SidePanel::right("up_next")
            .resizable(false)
            .default_width(300.0)
            .show(ctx, |ui| {
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("UP NEXT").size(12.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(if self.auto_mix { "MIX ON" } else { "MANUAL" })
                                .size(10.0)
                                .color(egui::Color32::from_rgb(190, 177, 255)),
                        );
                    });
                });
                ui.separator();
                let queue_snapshot: Vec<Track> = self.queue.iter().cloned().collect();
                if queue_snapshot.is_empty() {
                    ui.small("Your mix will appear here as songs are added.");
                }
                let mut remove = None;
                for (i, track) in queue_snapshot.iter().enumerate() {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(12, 12, 16))
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(format!("{:02}", i + 1))
                                        .color(egui::Color32::from_rgb(147, 137, 175)),
                                );
                                if ui
                                    .selectable_label(
                                        false,
                                        egui::RichText::new(&track.title).size(13.0),
                                    )
                                    .clicked()
                                {
                                    if let Some(pos) =
                                        self.queue.iter().position(|item| item.id == track.id)
                                    {
                                        self.queue.remove(pos);
                                    }
                                    self.play_track(track.clone());
                                }
                                if ui.small_button("Remove").clicked() {
                                    remove = Some(i);
                                }
                            });
                            ui.small(
                                egui::RichText::new(format!(
                                    "{}  ·  {}",
                                    track.channel,
                                    track.duration_text()
                                ))
                                .color(egui::Color32::from_rgb(147, 145, 157)),
                            );
                        });
                }
                if let Some(i) = remove {
                    self.queue.remove(i);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(18.0);
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(13, 12, 18))
                .rounding(18.0)
                .inner_margin(22.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("NOW PLAYING")
                            .size(11.0)
                            .color(egui::Color32::from_rgb(167, 157, 214)),
                    );
                    ui.add_space(9.0);
                    ui.horizontal(|ui| {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(48, 39, 70))
                            .rounding(14.0)
                            .inner_margin(25.0)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("♫")
                                        .size(46.0)
                                        .color(egui::Color32::from_rgb(221, 214, 255)),
                                );
                            });
                        ui.vertical(|ui| {
                            if let Some(track) = &self.current {
                                ui.label(egui::RichText::new(&track.title).size(21.0).strong());
                                ui.label(
                                    egui::RichText::new(&track.channel)
                                        .color(egui::Color32::from_rgb(180, 174, 190)),
                                );
                                ui.add_space(10.0);
                                let elapsed = self
                                    .player
                                    .started
                                    .map(|start| start.elapsed().as_secs_f64())
                                    .unwrap_or(0.0);
                                let duration = track.duration.unwrap_or(0.0);
                                let progress = if duration > 0.0 {
                                    (elapsed / duration).min(1.0)
                                } else {
                                    0.0
                                };
                                ui.add(
                                    egui::ProgressBar::new(progress as f32)
                                        .desired_width(360.0)
                                        .text(format!(
                                            "{}  /  {}",
                                            format_duration(elapsed),
                                            track.duration_text()
                                        )),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("Choose something to play")
                                        .size(21.0)
                                        .strong(),
                                );
                                ui.label("Search YouTube to start your mix");
                            }
                        });
                    });
                });
            ui.add_space(22.0);
            ui.label(egui::RichText::new("SEARCH RESULTS").size(12.0).strong());
            if self.results.is_empty() {
                ui.small("Search for a song, artist, album, or YouTube URL to begin.");
            }
            for track in self.results.clone() {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(9, 9, 12))
                    .rounding(12.0)
                    .inner_margin(10.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if ui.button("Play").clicked() {
                                self.play_track(track.clone());
                            }
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&track.title).strong());
                                ui.small(format!(
                                    "{}  ·  {}",
                                    track.channel,
                                    track.duration_text()
                                ));
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.button("Add to mix").clicked() {
                                        self.queue.push_back(track.clone());
                                    }
                                },
                            );
                        });
                    });
            }
        });

        egui::TopBottomPanel::bottom("controls")
            .exact_height(82.0)
            .show(ctx, |ui| {
                ui.add_space(9.0);
                ui.horizontal(|ui| {
                    if ui.button("Previous").clicked() {
                        self.previous();
                    }
                    if ui
                        .add_sized(
                            [90.0, 34.0],
                            egui::Button::new(if self.player.paused { "Play" } else { "Pause" }),
                        )
                        .clicked()
                    {
                        self.player.paused = !self.player.paused;
                        self.player.send(if self.player.paused {
                            "[\"set_property\",\"pause\",true]"
                        } else {
                            "[\"set_property\",\"pause\",false]"
                        });
                    }
                    if ui.button("Next").clicked() {
                        self.next();
                    }
                    if ui.button("Stop").clicked() {
                        self.player.stop();
                        self.current = None;
                        self.status = "Stopped".into();
                    }
                    ui.separator();
                    ui.label("Volume");
                    let volume_changed = ui
                        .add_sized(
                            [180.0, 24.0],
                            egui::Slider::new(&mut self.player.volume, 0..=100).show_value(true),
                        )
                        .changed();
                    if volume_changed {
                        self.player.send(&format!(
                            "[\"set_property\",\"volume\",{}]",
                            self.player.volume
                        ));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.small(
                            self.current
                                .as_ref()
                                .map(|t| format!("{}  ·  {}", t.title, self.status))
                                .unwrap_or_else(|| self.status.clone()),
                        );
                    });
                });
            });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("YTMusic")
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([700.0, 450.0]),
        ..Default::default()
    };
    eframe::run_native(
        "YTMusic",
        options,
        Box::new(|_cc| Ok(Box::new(YtMusicApp::default()))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, title: &str) -> Track {
        Track {
            title: title.into(),
            id: id.into(),
            duration: Some(125.0),
            channel: "Test artist".into(),
        }
    }

    #[test]
    fn mix_url_uses_youtube_rd_playlist_for_the_seed_track() {
        assert_eq!(
            youtube_mix_url(&track("abc123", "Song")),
            "https://www.youtube.com/watch?v=abc123&list=RDabc123"
        );
    }

    #[test]
    fn duration_format_is_stable_for_progress_and_queue_rows() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(125.9), "2:05");
        assert_eq!(format_duration(-1.0), "0:00");
    }

    #[test]
    fn mix_queue_excludes_current_history_and_duplicates() {
        let mut app = YtMusicApp::default();
        app.current = Some(track("current", "Current"));
        app.history.push(track("played", "Played"));
        app.queue.push_back(track("queued", "Already queued"));

        app.add_mix_items(vec![
            track("current", "Current"),
            track("played", "Played"),
            track("queued", "Already queued"),
            track("new", "New song"),
        ]);

        assert_eq!(app.queue.len(), 2);
        assert_eq!(app.queue.back().map(|song| song.id.as_str()), Some("new"));
    }

    #[test]
    fn registry_releases_process_ids_and_blocks_new_work_on_shutdown() {
        let registry = ProcessRegistry::default();
        registry.add(1001);
        registry.remove(1001);
        assert!(registry.pids.lock().expect("registry lock").is_empty());

        registry.terminate_all();
        assert!(registry.closing.load(Ordering::SeqCst));
    }

    #[test]
    fn empty_ytdlp_output_reports_a_useful_search_error() {
        let error = parse_ytdlp_entries(b"", b"").expect_err("empty output must fail");
        assert_eq!(error, "yt-dlp returned no search data");

        let error = parse_ytdlp_entries(b"\n", b"YouTube blocked the request")
            .expect_err("empty output with stderr must fail");
        assert_eq!(error, "YouTube blocked the request");
    }

    #[test]
    fn line_delimited_ytdlp_results_are_all_retained() {
        let json = br#"{"id":"one","title":"One"}
{"id":"two","title":"Two"}"#;
        let entries = parse_ytdlp_entries(json, b"").expect("results should parse");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id.as_deref(), Some("one"));
        assert_eq!(entries[1].id.as_deref(), Some("two"));
    }

    #[cfg(windows)]
    #[test]
    fn registered_command_is_removed_after_completion() {
        let registry = ProcessRegistry::default();
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "echo", "captured"]);
        let output = run_registered(command, &registry).expect("command should run");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "captured");
        assert!(registry.pids.lock().expect("registry lock").is_empty());
    }
}
