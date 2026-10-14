mod actions;
mod errors;
mod jam_sync;

use crate::{
    domain::{youtube_mix_url, CookieBrowser, Track},
    infrastructure::process::ProcessRegistry,
    jam::{JamController, Role},
    media::{query_ytdlp, resolve_audio_url, Tools},
    playback::{Player, Queue},
    recommendations::{select_mix_items, History},
    theme::Theme,
    ui::footer::{show as render_footer, FooterState},
    ui::screens::{show_header, show_home, show_queue_panel, HomeAction, QueueAction},
    ui::{
        apply_text_sizes, icons::IconTextures, layout::responsive_layout, responsive::ResponsiveUi,
    },
};
use eframe::egui;
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use self::{errors::AppError, jam_sync::GuestSync};

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
    jam_show_code_join: bool,
    jam_sync: GuestSync,
    jam_notice: Option<(String, Instant)>,
    /// The track currently being resolved, so a Jam host can announce it early.
    pending_track: Option<Track>,
    /// Host: when a new track started waiting, paused, for guests to load it.
    jam_hold: Option<Instant>,
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
            jam_join_address: String::new(),
            jam_join_token: String::new(),
            jam_show_code_join: false,
            jam_sync: GuestSync::default(),
            jam_notice: None,
            pending_track: None,
            jam_hold: None,
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
        self.pending_track = Some(track.clone());
        self.jam_hold = None;
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
            self.pending_track = None;
            if self.is_guest() {
                // A guest only ever loads the host's track, on the host's clock.
                match result {
                    Ok((track, url, _, _)) => self.finish_guest_load(track, &url),
                    Err(error) => self.fail_guest_load(None, error),
                }
                return;
            }
            match result {
                Ok((track, url, remember_current, refresh_mix)) => {
                    if remember_current {
                        if let Some(old) = self.current.replace(track.clone()) {
                            self.history.push(old);
                        }
                    } else {
                        self.current = Some(track.clone());
                    }
                    // A Jam host holds a new track at 0:00 until its guests
                    // have loaded it, so everyone hears the start together.
                    let hold = self.should_hold_for_guests();
                    match self
                        .player
                        .play_stream_at(&track, &url, &self.tools.mpv, 0.0, hold)
                    {
                        Ok(()) => {
                            self.jam_hold = hold.then(Instant::now);
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
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            apply_text_sizes(ui, &responsive);
                            ui.add_space(responsive.rem(0.375));
                            ui.label(
                                egui::RichText::new(format!("Queue  {}", self.queue.len()))
                                    .size(responsive.font_body)
                                    .color(accent),
                            );
                            ui.separator();
                            ui.checkbox(&mut self.auto_mix, "Auto mix");
                            ui.separator();
                            self.show_jam_panel(ui, &responsive);
                            ui.add_space(responsive.rem(0.375));
                            ui.label(
                                egui::RichText::new("YOUTUBE ACCESS")
                                    .size(responsive.font_caption)
                                    .color(muted),
                            );
                            egui::ComboBox::from_id_salt("cookie_browser")
                                .selected_text(self.cookie_browser.label())
                                .width(responsive.clamp_rem(85.0, 5.3125, 140.0))
                                .show_ui(ui, |ui| {
                                    apply_text_sizes(ui, &responsive);
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
                                ui.add_space(responsive.space_sm);
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 135, 155),
                                    error.to_string(),
                                );
                            }
                        });
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
            self.is_guest(),
            &responsive,
        );
        for action in home_actions {
            match action {
                // A guest's picks become requests; only the host's queue plays.
                HomeAction::PlaySearch(track) | HomeAction::AddToQueue(track)
                    if self.is_guest() =>
                {
                    self.request_from_host(&track);
                }
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

        self.player.refresh_position();
        self.poll_background_work();
        self.poll_jam();
        let guest = self.is_guest();
        if !guest && self.player.is_finished() && !self.finished_handled && !self.loading {
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
        // A guest releases preloaded audio and corrects drift on frame
        // boundaries, and a host's changes are noticed on them, so a session
        // repaints faster than the idle interface needs to.
        ctx.request_repaint_after(Duration::from_millis(match self.jam.info.role {
            Some(Role::Guest) => 20,
            Some(Role::Host) => 100,
            None => 250,
        }));

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
            let mut app = YtMusicApp::default();
            app.jam.start_discovery();
            Ok(Box::new(app))
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
