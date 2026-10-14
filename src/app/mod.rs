mod actions;
mod errors;

use crate::{
    domain::{youtube_mix_url, CookieBrowser, Track},
    infrastructure::process::ProcessRegistry,
    jam::{self, JamController, JamEvent, PlaybackSnapshot, Role},
    media::{query_ytdlp, resolve_audio_url, Tools},
    playback::{Player, Queue},
    recommendations::{select_mix_items, History},
    theme::Theme,
    ui::footer::{show as render_footer, FooterState},
    ui::screens::{show_header, show_home, show_queue_panel, HomeAction, QueueAction},
    ui::{icons::IconTextures, layout::responsive_layout, responsive::ResponsiveUi},
};
use eframe::egui;
use std::{
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use self::errors::AppError;

type PlayResult = Result<(Track, String, bool, bool), String>;

struct YtMusicApp {
    icons: Option<IconTextures>,
    tools: Tools,
    registry: ProcessRegistry,
    player: Player,
    search: String,
    results: Vec<Track>,
    queue: Queue,
    history: History,
    current: Option<Track>,
    status: String,
    error: Option<AppError>,
    auto_mix: bool,
    cookie_browser: CookieBrowser,
    searching: bool,
    loading: bool,
    mixing: bool,
    finished_handled: bool,
    search_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    mix_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    play_rx: Option<Receiver<PlayResult>>,
    jam: JamController,
    jam_join_address: String,
    jam_join_token: String,
    jam_port: String,
    jam_pending: Option<PlaybackSnapshot>,
}

impl Default for YtMusicApp {
    fn default() -> Self {
        let registry = ProcessRegistry::default();
        let mut player = Player::default();
        player.set_registry(registry.clone());
        Self {
            icons: None,
            tools: Tools::discover(),
            registry,
            player,
            search: String::new(),
            results: vec![],
            queue: Queue::default(),
            history: History::default(),
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
            jam: JamController::default(),
            jam_join_address: "192.168.1.1:38721".into(),
            jam_join_token: String::new(),
            jam_port: "38721".into(),
            jam_pending: None,
        }
    }
}

impl YtMusicApp {
    fn render_full_width_footer(
        &mut self,
        ctx: &egui::Context,
        icons: &IconTextures,
        responsive: &ResponsiveUi,
    ) {
        let duration = self
            .current
            .as_ref()
            .and_then(|track| track.duration)
            .unwrap_or(0.0);
        let position = if duration > 0.0 {
            self.player.position_seconds().min(duration)
        } else {
            0.0
        };
        let guest = self.jam.info.role == Some(Role::Guest);
        let actions = render_footer(
            ctx,
            icons,
            FooterState {
                current: self.current.as_ref(),
                paused: self.player.paused,
                position,
                duration,
                volume: self.player.volume,
                guest,
            },
            responsive,
        );

        for action in actions {
            actions::apply_footer_action(self, action);
        }
    }

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

    fn play_search_track(&mut self, track: Track) {
        if self.jam.info.role == Some(Role::Guest) {
            return;
        }
        self.reset_mix_for_search();
        self.start_track(track, true, true);
    }

    fn reset_mix_for_search(&mut self) {
        self.queue.clear();
        self.mix_rx = None;
        self.mixing = false;
    }

    fn play_queue_track(&mut self, track: Track) {
        if self.jam.info.role == Some(Role::Guest) {
            return;
        }
        self.start_track(track, true, false);
    }

    fn start_track(&mut self, track: Track, remember_current: bool, refresh_mix: bool) {
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
                .map(|url| (track, url, remember_current, refresh_mix));
            let _ = sender.send(result);
        });
    }

    fn next(&mut self) {
        if self.jam.info.role == Some(Role::Guest) {
            return;
        }
        if let Some(track) = self.queue.pop_next() {
            self.play_queue_track(track);
        } else {
            self.player.stop();
            self.current = None;
            self.status = "Queue finished".into();
            self.play_rx = None;
            self.loading = false;
        }
    }

    fn previous(&mut self) {
        if self.jam.info.role == Some(Role::Guest) {
            return;
        }
        if let Some(track) = self.history.pop() {
            if let Some(current) = self.current.take() {
                self.queue.push_front(current);
            }
            self.start_track(track, false, false);
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
        let queued_ids: Vec<&str> = self.queue.iter().map(|track| track.id.as_str()).collect();
        let history_ids: Vec<&str> = self.history.iter().map(|track| track.id.as_str()).collect();
        for item in select_mix_items(items, current_id, &queued_ids, &history_ids, 8) {
            self.queue.enqueue_unique(item);
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
                    self.error = Some(AppError::Search(e));
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
                    self.error = Some(AppError::Mix(format!("Could not create mix: {e}")));
                }
            }
        }

        let play_result = self.play_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = play_result {
            self.play_rx = None;
            self.loading = false;
            match result {
                Ok((track, url, remember_current, refresh_mix)) => {
                    if remember_current {
                        if let Some(old) = self.current.replace(track.clone()) {
                            self.history.push(old);
                        }
                    } else {
                        self.current = Some(track.clone());
                    }
                    match self.player.play_stream(&track, &url, &self.tools.mpv) {
                        Ok(()) => {
                            self.status = "Playing".into();
                            self.error = None;
                            self.finished_handled = false;
                            if refresh_mix && self.auto_mix {
                                self.add_mix(&track);
                            }
                        }
                        Err(e) => {
                            self.status = "Playback failed".into();
                            self.error = Some(AppError::Playback(e));
                        }
                    }
                }
                Err(e) => {
                    self.status = "Playback failed".into();
                    self.error = Some(AppError::Playback(e));
                }
            }
        }
    }

    fn jam_snapshot(&self) -> Option<PlaybackSnapshot> {
        let track = self.current.as_ref().and_then(|track| {
            jam::track_ref(&track.id, &track.title, &track.channel, track.duration)
        });
        Some(PlaybackSnapshot {
            revision: self.jam_revision(),
            track,
            position_secs: self.player.position_seconds(),
            playing: self.current.is_some() && !self.player.paused && !self.loading,
            host_clock_ms: self.jam.clock_ms(),
            scheduled_start_host_ms: None,
        })
    }

    fn jam_revision(&self) -> u64 {
        self.current
            .as_ref()
            .map(|track| track.id.bytes().map(u64::from).sum::<u64>())
            .unwrap_or(0)
    }

    fn poll_jam(&mut self) {
        for event in self.jam.poll() {
            if let JamEvent::Snapshot(snapshot) = event {
                self.jam_pending = Some(snapshot.clone());
                if self.jam.info.role == Some(Role::Guest) {
                    self.status = format!("Jam sync · revision {}", snapshot.revision);
                    if let Some(track) = snapshot.track {
                        if self.current.as_ref().map(|current| current.id.as_str())
                            != Some(track.video_id.as_str())
                            && !self.loading
                        {
                            self.start_track(
                                Track {
                                    title: track.title,
                                    id: track.video_id,
                                    duration: track.duration_secs,
                                    channel: track.artist,
                                },
                                false,
                                false,
                            );
                        }
                    } else if self.current.is_some() {
                        self.player.stop();
                        self.current = None;
                    }
                }
            }
        }
        if self.jam.info.role == Some(Role::Host) {
            if let Some(snapshot) = self.jam_snapshot() {
                self.jam.broadcast(snapshot);
            }
        }
    }

    fn reconcile_guest_playback(&mut self) {
        if self.jam.info.role != Some(Role::Guest) || self.loading {
            return;
        }
        let Some(snapshot) = self.jam_pending.clone() else {
            return;
        };
        let Some(track) = &snapshot.track else {
            return;
        };
        if self.current.as_ref().map(|current| current.id.as_str()) != Some(track.video_id.as_str())
        {
            return;
        }
        let expected = jam::expected_position(&snapshot, self.jam.host_now_ms());
        if let jam::DriftAction::Seek(position) =
            jam::drift_action(self.player.position_seconds(), expected)
        {
            self.player.seek(position);
        }
        let should_pause = !snapshot.playing;
        if self.player.paused != should_pause {
            self.player.set_paused(should_pause);
        }
    }
}

impl Drop for YtMusicApp {
    fn drop(&mut self) {
        self.player.stop();
        self.registry.terminate_all();
    }
}

impl YtMusicApp {
    fn render_responsive_layout(&mut self, ctx: &egui::Context) {
        if self.icons.is_none() {
            self.icons = IconTextures::load(ctx).ok();
        }
        let Some(icons) = self.icons.clone() else {
            self.error = Some(AppError::Ui("Could not load the bundled SVG icons".into()));
            return;
        };
        let theme = Theme::default();
        let viewport = ctx.available_rect().size();
        let responsive = ResponsiveUi::from_size(viewport);
        let layout = responsive_layout(viewport, &responsive);
        let muted = theme.secondary_text;
        let accent = theme.accent;
        let header_height = layout.header_height;
        let show_sidebar = layout.show_sidebar;
        let show_queue = layout.show_queue;
        let sidebar_width = layout.sidebar_width;
        let queue_width = layout.queue_width;

        if show_header(ctx, header_height, &mut self.search, &responsive) {
            self.search();
        }
        self.render_full_width_footer(ctx, &icons, &responsive);

        if show_sidebar {
            egui::SidePanel::left("app_sidebar")
                .resizable(false)
                .exact_width(sidebar_width)
                .show(ctx, |ui| {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(format!("Queue  {}", self.queue.len()))
                            .size(12.0)
                            .color(accent),
                    );
                    ui.separator();
                    ui.checkbox(&mut self.auto_mix, "Auto mix");
                    ui.separator();
                    ui.label(egui::RichText::new("LAN JAM").size(9.0).color(muted));
                    if self.jam.info.role.is_none() {
                        if ui.button("Host Jam").clicked() {
                            let port = self.jam_port.parse::<u16>().unwrap_or(38_721);
                            if let Err(error) = self.jam.host(port) {
                                self.error = Some(AppError::Jam(error));
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Port");
                            ui.add_sized(
                                [70.0, 22.0],
                                egui::TextEdit::singleline(&mut self.jam_port),
                            );
                        });
                        ui.add_sized(
                            [sidebar_width - 16.0, 22.0],
                            egui::TextEdit::singleline(&mut self.jam_join_address)
                                .hint_text("Host address:port"),
                        );
                        ui.add_sized(
                            [sidebar_width - 16.0, 22.0],
                            egui::TextEdit::singleline(&mut self.jam_join_token)
                                .hint_text("Join token"),
                        );
                        if ui.button("Join Jam").clicked() {
                            if let Err(error) =
                                self.jam.join(&self.jam_join_address, &self.jam_join_token)
                            {
                                self.error = Some(AppError::Jam(error));
                            }
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(&self.jam.info.status)
                                .size(10.0)
                                .color(muted),
                        );
                        if self.jam.info.role == Some(Role::Host) {
                            ui.small(format!("Share: {}", self.jam.info.address));
                            ui.small(format!("Token: {}", self.jam.info.token));
                            ui.small(format!("Guests: {}", self.jam.info.peers));
                        }
                        if ui
                            .button(if self.jam.info.role == Some(Role::Host) {
                                "End Jam"
                            } else {
                                "Leave Jam"
                            })
                            .clicked()
                        {
                            self.jam.leave();
                        }
                    }
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("YOUTUBE ACCESS").size(9.0).color(muted));
                    egui::ComboBox::from_id_salt("cookie_browser")
                        .selected_text(self.cookie_browser.label())
                        .width(85.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.cookie_browser,
                                CookieBrowser::None,
                                "Off",
                            );
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
                    if let Some(error) = &self.error {
                        ui.add_space(8.0);
                        ui.colored_label(egui::Color32::from_rgb(255, 135, 155), error.to_string());
                    }
                });
        }

        if show_queue {
            let queue_snapshot: Vec<Track> = self.queue.iter().cloned().collect();
            for action in show_queue_panel(
                ctx,
                queue_width,
                self.auto_mix,
                &queue_snapshot,
                &responsive,
            ) {
                match action {
                    QueueAction::Play(track) => {
                        self.queue.remove_id(&track.id);
                        self.play_queue_track(track);
                    }
                    QueueAction::Remove(index) => {
                        self.queue.remove(index);
                    }
                }
            }
        }
        let home_actions = show_home(
            ctx,
            &icons,
            self.current.as_ref(),
            &self.results,
            &responsive,
        );
        for action in home_actions {
            match action {
                HomeAction::PlaySearch(track) => self.play_search_track(track),
                HomeAction::AddToQueue(track) => self.queue.push_back(track),
            }
        }
    }
}

impl eframe::App for YtMusicApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::default();
        let background = theme.background;
        let surface = theme.surface;
        let raised = theme.raised;
        let button = theme.surface;
        let text = theme.text;
        let accent = theme.accent;

        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(text);
        visuals.panel_fill = background;
        visuals.window_fill = surface;
        visuals.faint_bg_color = surface;
        visuals.extreme_bg_color = background;
        visuals.widgets.noninteractive.bg_fill = background;
        visuals.widgets.noninteractive.fg_stroke.color = theme.secondary_text;
        visuals.widgets.inactive.bg_fill = button;
        visuals.widgets.inactive.weak_bg_fill = button;
        visuals.widgets.hovered.bg_fill = raised;
        visuals.widgets.hovered.weak_bg_fill = theme.hover;
        visuals.widgets.active.bg_fill = accent;
        visuals.widgets.active.weak_bg_fill = accent;
        visuals.widgets.open.fg_stroke.color = theme.muted_text;
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(theme.radius);
        visuals.widgets.inactive.rounding = egui::Rounding::same(theme.radius);
        visuals.widgets.hovered.rounding = egui::Rounding::same(theme.radius);
        visuals.widgets.active.rounding = egui::Rounding::same(theme.radius);
        visuals.selection.bg_fill = accent;
        visuals.selection.stroke.color = accent;
        ctx.set_visuals(visuals);
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(theme.spacing, theme.spacing * 0.75);
            style.spacing.button_padding = egui::vec2(theme.spacing + 1.0, theme.spacing * 0.75);
        });

        self.poll_background_work();
        self.poll_jam();
        self.reconcile_guest_playback();
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

        self.render_responsive_layout(ctx);
    }
}

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MusiKPL")
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([700.0, 450.0]),
        ..Default::default()
    };
    eframe::run_native(
        "MusiKPL",
        options,
        Box::new(|cc| {
            apply_windows_title_bar_colors(cc);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(YtMusicApp::default()))
        }),
    )
}

#[cfg(windows)]
fn apply_windows_title_bar_colors(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{ffi::c_void, mem::size_of};
    use windows_sys::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR,
        },
    };

    let Ok(window_handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(window) = window_handle.as_raw() else {
        return;
    };

    let hwnd = window.hwnd.get() as HWND;
    let black: u32 = 0x000000;
    let white: u32 = 0xFFFFFF;

    unsafe {
        for (attribute, color) in [
            (DWMWA_CAPTION_COLOR, black),
            (DWMWA_TEXT_COLOR, white),
            (DWMWA_BORDER_COLOR, black),
        ] {
            let _ = DwmSetWindowAttribute(
                hwnd,
                attribute as u32,
                &color as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            );
        }
    }
}

#[cfg(not(windows))]
fn apply_windows_title_bar_colors(_: &eframe::CreationContext<'_>) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::parse_ytdlp_entries;

    fn track(id: &str, title: &str) -> Track {
        Track {
            title: title.into(),
            id: id.into(),
            duration: Some(125.0),
            channel: "Test artist".into(),
        }
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
        assert_eq!(
            app.queue.iter().last().map(|song| song.id.as_str()),
            Some("new")
        );
    }

    #[test]
    fn starting_a_new_search_resets_the_previous_mix_queue() {
        let mut app = YtMusicApp::default();
        app.queue.push_back(track("old-1", "Old mix song"));
        app.queue.push_back(track("old-2", "Another old mix song"));
        app.mixing = true;

        app.reset_mix_for_search();

        assert!(app.queue.is_empty());
        assert!(!app.mixing);
        assert!(app.mix_rx.is_none());
    }

    #[test]
    fn registry_releases_process_ids_and_blocks_new_work_on_shutdown() {
        let registry = ProcessRegistry::default();
        registry.add(1001);
        registry.remove(1001);
        assert!(registry.is_empty());

        registry.terminate_all();
        assert!(registry.is_closing());
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
        use crate::infrastructure::process::run_registered;
        use std::process::Command;

        let registry = ProcessRegistry::default();
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "echo", "captured"]);
        let output = run_registered(command, &registry).expect("command should run");

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "captured");
        assert!(registry.is_empty());
    }
}
