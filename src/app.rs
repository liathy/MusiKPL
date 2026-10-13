use crate::{
    jam::{self, JamController, JamEvent, PlaybackSnapshot, Role},
    models::{format_duration, format_position, youtube_mix_url, CookieBrowser, Track},
    player::Player,
    process::ProcessRegistry,
    theme::Theme,
    youtube::{query_ytdlp, resolve_audio_url, Tools},
};
use eframe::egui;
use std::{
    collections::VecDeque,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

struct YtMusicApp {
    icons: Option<IconTextures>,
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
    play_rx: Option<Receiver<Result<(Track, String, bool, bool), String>>>,
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
            jam: JamController::default(),
            jam_join_address: "192.168.1.1:38721".into(),
            jam_join_token: String::new(),
            jam_port: "38721".into(),
            jam_pending: None,
        }
    }
}

impl YtMusicApp {
    fn render_full_width_footer(&mut self, ctx: &egui::Context, icons: &IconTextures) {
        let theme = Theme::default();
        let available = ctx.available_rect().size();
        let unit = (available.x / 120.0).clamp(12.0, 16.0);
        let footer_height = responsive_layout(available.x, available.y, unit).footer_height;

        egui::TopBottomPanel::bottom("app_footer")
            .frame(
                egui::Frame::none()
                    .fill(egui::Color32::BLACK)
                    .inner_margin(0.0),
            )
            .exact_height(footer_height)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::BLACK)
                    .show(ui, |ui| {
                        let footer_rect = ui.available_rect_before_wrap();
                        ui.set_min_size(footer_rect.size());
                        let rect = footer_rect;
                        ui.painter().line_segment(
                            [rect.left_top(), rect.right_top()],
                            egui::Stroke::new(1.0_f32, theme.border),
                        );
                        let geometry = footer_geometry(footer_rect);
                        let padding = footer_padding(footer_rect.width());
                        ui.allocate_new_ui(
                            egui::UiBuilder::new()
                                .max_rect(footer_rect)
                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                            |ui| {
                                if geometry.left.width() > 0.0 {
                                    let left_content_rect = geometry.left.shrink2(
                                        egui::vec2(padding, 0.0),
                                    );
                                ui.allocate_new_ui(
                                egui::UiBuilder::new()
                                    .max_rect(left_content_rect)
                                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Image::from_texture((
                                                icons.music.id(),
                                                egui::vec2(28.0, 28.0),
                                            ))
                                            .fit_to_exact_size(egui::vec2(28.0, 28.0))
                                            .tint(theme.accent),
                                        );
                                        ui.vertical(|ui| {
                                            if let Some(track) = &self.current {
                                                ui.label(
                                                    egui::RichText::new(&track.title)
                                                        .size(14.0)
                                                        .color(theme.text),
                                                );
                                                ui.small(
                                                    egui::RichText::new(&track.channel)
                                                        .color(theme.secondary_text),
                                                );
                                            } else {
                                                ui.label(
                                                    egui::RichText::new("Nothing playing")
                                                        .size(14.0)
                                                        .color(theme.text),
                                                );
                                                ui.small("Search for music to begin");
                                            }
                                        });
                                    });
                                },
                                );

                                }

                                ui.allocate_new_ui(
                                    egui::UiBuilder::new()
                                        .max_rect(geometry.playback)
                                        .layout(egui::Layout::top_down(egui::Align::Center)),
                                    |ui| {
                                        let rows = footer_playback_rows(geometry.playback);

                                        let duration = self
                                            .current
                                            .as_ref()
                                            .and_then(|track| track.duration)
                                            .unwrap_or(0.0);

                                        let mut position = if duration > 0.0 {
                                            self.player.position_seconds().min(duration) as f32
                                        } else {
                                            0.0
                                        };
                                        // A zero-length slider range is rendered by egui with
                                        // its handle at the midpoint. Use a harmless disabled
                                        // visual range while no track is loaded so it stays at 00:00.
                                        let slider_end = duration.max(1.0) as f32;
                                        let guest = self.jam.info.role == Some(Role::Guest);

                                        // Row 1: playback buttons, centered in their own rectangle.
                                        ui.allocate_new_ui(
                                            egui::UiBuilder::new()
                                                .max_rect(rows.transport)
                                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                            |ui| {
                                                let button_width =
                                                    28.0 + 36.0 + 28.0 + ui.spacing().item_spacing.x * 2.0;
                                                let button_rect = egui::Rect::from_center_size(
                                                    rows.transport.center(),
                                                    egui::vec2(button_width, 30.0),
                                                );
                                                ui.allocate_new_ui(
                                                    egui::UiBuilder::new()
                                                        .max_rect(button_rect)
                                                        .layout(egui::Layout::left_to_right(
                                                            egui::Align::Center,
                                                        )),
                                                    |ui| {
                                                        ui.horizontal(|ui| {
                                                    if !guest && icon_button(
                                                        ui,
                                                        &icons.previous,
                                                        20.0,
                                                        egui::vec2(28.0, 28.0),
                                                        "Previous",
                                                    )
                                                    .clicked()
                                                    {
                                                        self.previous();
                                                    }

                                                    let play_icon = if self.player.paused {
                                                        &icons.play
                                                    } else {
                                                        &icons.pause
                                                    };
                                                    if ui
                                                        .add_enabled(
                                                            !guest,
                                                            egui::Button::image(design_icon(play_icon, 22.0))
                                                                .min_size(egui::vec2(36.0, 30.0)),
                                                        )
                                                        .clicked()
                                                    {
                                                        self.player.set_paused(!self.player.paused);
                                                    }

                                                    if !guest && icon_button(
                                                        ui,
                                                        &icons.next,
                                                        20.0,
                                                        egui::vec2(28.0, 28.0),
                                                        "Next",
                                                    )
                                                    .clicked()
                                                    {
                                                        self.next();
                                                    }
                                                        });
                                                    },
                                                );
                                            },
                                        );

                                        // Row 2: elapsed time, seek slider, and duration.
                                        // Both rows are centered on the same fixed footer midpoint.
                                        let elapsed = format_position(position as f64);
                                        let total = format_duration(duration);

                                        // Measure each label independently so the complete visible
                                        // row can be centered using its actual content bounds.
                                        let (elapsed_width, total_width) = ui.fonts(|fonts| {
                                            let font = egui::FontId::proportional(12.0);
                                            let elapsed_width = fonts
                                                .layout_no_wrap(elapsed.clone(), font.clone(), theme.secondary_text)
                                                .size().x;
                                            let total_width = fonts
                                                .layout_no_wrap(total.clone(), font, theme.secondary_text)
                                                .size().x;
                                            (elapsed_width, total_width)
                                        });
                                        let gap = ui.spacing().item_spacing.x;
                                        let slider_width = (rows.progress.width()
                                            - elapsed_width
                                            - total_width
                                            - gap * 2.0)
                                            .max(40.0)
                                            .min(rows.progress.width());

                                        let content_width = elapsed_width
                                            + gap
                                            + slider_width
                                            + gap
                                            + total_width;
                                        let content_left =
                                            rows.progress.center().x - content_width / 2.0;
                                        let content_top = rows.progress.center().y - 10.0;
                                        let elapsed_rect = egui::Rect::from_min_size(
                                            egui::pos2(content_left, content_top),
                                            egui::vec2(elapsed_width, 20.0),
                                        );
                                        let slider_rect = egui::Rect::from_min_size(
                                            egui::pos2(elapsed_rect.right() + gap, content_top),
                                            egui::vec2(slider_width, 20.0),
                                        );
                                        let total_rect = egui::Rect::from_min_size(
                                            egui::pos2(slider_rect.right() + gap, content_top),
                                            egui::vec2(total_width, 20.0),
                                        );

                                        ui.allocate_new_ui(
                                            egui::UiBuilder::new()
                                                .max_rect(elapsed_rect)
                                                .layout(egui::Layout::right_to_left(egui::Align::Center)),
                                            |time_ui| {
                                                time_ui.label(
                                                    egui::RichText::new(elapsed)
                                                        .size(12.0)
                                                        .color(theme.secondary_text),
                                                );
                                            },
                                        );

                                        let slider_response = ui.allocate_new_ui(
                                            egui::UiBuilder::new()
                                                .max_rect(slider_rect)
                                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                            |slider_ui| {
                                                let visuals = slider_ui.visuals_mut();
                                                visuals.widgets.inactive.bg_fill = theme.border;
                                                visuals.widgets.inactive.weak_bg_fill = theme.border;
                                                visuals.widgets.hovered.bg_fill = theme.secondary_text;
                                                visuals.widgets.active.bg_fill = theme.accent;
                                                visuals.slider_trailing_fill = true;
                                                let style = slider_ui.style_mut();
                                                style.spacing.slider_width = slider_width;
                                                style.spacing.slider_rail_height = 6.0;

                                                slider_ui.add_sized(
                                                    egui::vec2(slider_width, 20.0),
                                                            egui::Slider::new(
                                                                &mut position,
                                                                0.0..=slider_end,
                                                            )
                                                    .show_value(false),
                                                )
                                            },
                                        );

                                        debug_assert!(
                                            (slider_response.inner.rect.width() - slider_width)
                                                .abs()
                                                < 0.5
                                        );

                                        ui.allocate_new_ui(
                                            egui::UiBuilder::new()
                                                .max_rect(total_rect)
                                                .layout(egui::Layout::left_to_right(egui::Align::Center)),
                                            |time_ui| {
                                                time_ui.label(
                                                    egui::RichText::new(total)
                                                        .size(12.0)
                                                        .color(theme.secondary_text),
                                                );
                                            },
                                        );

                                        if slider_response.inner.changed() && !guest {
                                            self.player.seek(position as f64);
                                        }
                                    },
                                );

                                if geometry.right.width() > 0.0 {
                                    ui.allocate_new_ui(
                                        egui::UiBuilder::new().max_rect(geometry.right).layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                        ),
                                        |ui| {
                                            ui.add_space(padding);
                                            ui.label(
                                                egui::RichText::new("Volume")
                                                    .size(12.0)
                                                    .color(theme.text),
                                            );
                                            ui.add_space(padding * 0.5);
                                            let slider_width = (ui.available_width()
                                                - padding * 2.0)
                                                .clamp(40.0, 140.0);
                                            let volume_changed = ui
                                                .add_sized(
                                                    egui::vec2(slider_width, 22.0),
                                                    egui::Slider::new(
                                                        &mut self.player.volume,
                                                        0..=100,
                                                    )
                                                    .show_value(true),
                                                )
                                                .changed();
                                            if volume_changed {
                                                self.player.send(&format!(
                                                    "[\"set_property\",\"volume\",{}]",
                                                    self.player.volume
                                                ));
                                            }
                                        },
                                    );
                                }
                            },
                        );
                    });
            });
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
        if let Some(track) = self.queue.pop_front() {
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
        self.current.as_ref().map(|track| {
            track.id.bytes().map(u64::from).sum::<u64>()
        }).unwrap_or(0)
    }

    fn poll_jam(&mut self) {
        for event in self.jam.poll() {
            if let JamEvent::Snapshot(snapshot) = event {
                self.jam_pending = Some(snapshot.clone());
                if self.jam.info.role == Some(Role::Guest) {
                    self.status = format!("Jam sync · revision {}", snapshot.revision);
                    if let Some(track) = snapshot.track {
                        if self.current.as_ref().map(|current| current.id.as_str()) != Some(track.video_id.as_str())
                            && !self.loading
                        {
                            self.start_track(Track {
                                title: track.title,
                                id: track.video_id,
                                duration: track.duration_secs,
                                channel: track.artist,
                            }, false, false);
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
        let Some(snapshot) = self.jam_pending.clone() else { return; };
        let Some(track) = &snapshot.track else { return; };
        if self.current.as_ref().map(|current| current.id.as_str()) != Some(track.video_id.as_str()) {
            return;
        }
        let expected = jam::expected_position(&snapshot, self.jam.host_now_ms());
        if let jam::DriftAction::Seek(position) = jam::drift_action(self.player.position_seconds(), expected) {
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

const HOME_ICON: &[u8] =
    include_bytes!("assets/icons/home_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const MUSIC_ICON: &[u8] =
    include_bytes!("assets/icons/music_note_2_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const PLAY_ICON: &[u8] =
    include_bytes!("assets/icons/play_arrow_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const PAUSE_ICON: &[u8] =
    include_bytes!("assets/icons/pause_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const PREVIOUS_ICON: &[u8] =
    include_bytes!("assets/icons/skip_previous_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const NEXT_ICON: &[u8] =
    include_bytes!("assets/icons/skip_next_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");

#[derive(Clone)]
struct IconTextures {
    home: egui::TextureHandle,
    music: egui::TextureHandle,
    play: egui::TextureHandle,
    pause: egui::TextureHandle,
    previous: egui::TextureHandle,
    next: egui::TextureHandle,
}

impl IconTextures {
    fn load(ctx: &egui::Context) -> Result<Self, String> {
        let load = |name: &str, bytes: &[u8]| {
            egui_extras::image::load_svg_bytes(bytes)
                .map(|image| ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
        };
        Ok(Self {
            home: load("musikpl-home", HOME_ICON)?,
            music: load("musikpl-music", MUSIC_ICON)?,
            play: load("musikpl-play", PLAY_ICON)?,
            pause: load("musikpl-pause", PAUSE_ICON)?,
            previous: load("musikpl-previous", PREVIOUS_ICON)?,
            next: load("musikpl-next", NEXT_ICON)?,
        })
    }
}

fn design_icon(texture: &egui::TextureHandle, size: f32) -> egui::Image<'static> {
    egui::Image::from_texture((texture.id(), egui::vec2(size, size)))
        .fit_to_exact_size(egui::vec2(size, size))
        .tint(Theme::default().accent)
}

fn icon_button(
    ui: &mut egui::Ui,
    texture: &egui::TextureHandle,
    size: f32,
    button_size: egui::Vec2,
    tooltip: &str,
) -> egui::Response {
    ui.add_sized(button_size, egui::Button::image(design_icon(texture, size)))
        .on_hover_text(tooltip)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ResponsiveLayout {
    header_height: f32,
    footer_height: f32,
    body_height: f32,
    sidebar_width: f32,
    queue_width: f32,
    main_width: f32,
    show_sidebar: bool,
    show_queue: bool,
}

fn responsive_layout(width: f32, height: f32, unit: f32) -> ResponsiveLayout {
    // Secondary panels collapse from the current content width so resizing and
    // DPI scaling cannot squeeze the main content into a negative rectangle.
    let header_height = (4.625 * unit).clamp(56.0, 76.0);
    let footer_height = (4.9375 * unit).clamp(76.0, 104.0);
    let show_sidebar = width >= 820.0;
    let show_queue = width >= 1120.0;
    let sidebar_width = if show_sidebar {
        (12.875 * unit).clamp(144.0, 220.0)
    } else {
        0.0
    };
    let queue_width = if show_queue {
        (26.75 * unit).clamp(240.0, 360.0)
    } else {
        0.0
    };
    let body_height = (height - header_height - footer_height).max(1.0);
    let main_width = (width - sidebar_width - queue_width).max(1.0);

    ResponsiveLayout {
        header_height,
        footer_height,
        body_height,
        sidebar_width,
        queue_width,
        main_width,
        show_sidebar,
        show_queue,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FooterColumns {
    left: f32,
    center: f32,
    right: f32,
}

fn footer_columns(width: f32) -> FooterColumns {
    // Compact layout: prioritize playback when the window is too narrow for
    // track details and volume to fit comfortably beside it.
    if width < 720.0 {
        return FooterColumns {
            left: 0.0,
            center: width,
            right: 0.0,
        };
    }

    // Keep the outer columns equal. This is important: equal left/right widths
    // mean the playback column's center stays at the center of the whole window,
    // just like a centered grid column in a CSS layout.
    let side = if width < 900.0 {
        (width * 0.22).clamp(120.0, 180.0)
    } else {
        (width * 0.25).clamp(180.0, 480.0)
    };

    FooterColumns {
        left: side,                 // Track artwork + title/channel
        center: (width - side * 2.0).max(0.0), // Playback + progress
        right: side,                // Volume (implementation unchanged)
    }
}

fn footer_padding(width: f32) -> f32 {
    (width * 0.02).clamp(8.0, 24.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FooterGeometry {
    left: egui::Rect,
    playback: egui::Rect,
    right: egui::Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct FooterPlaybackRows {
    transport: egui::Rect,
    progress: egui::Rect,
}

fn footer_playback_rows(playback_rect: egui::Rect) -> FooterPlaybackRows {
    const TRANSPORT_HEIGHT: f32 = 30.0;
    const PROGRESS_HEIGHT: f32 = 20.0;
    const ROW_GAP: f32 = 10.0;

    // The playback rectangle is a full-width positioning reference. The
    // widgets themselves remain in compact rectangles so they cannot cover
    // the track details or volume controls.
    let transport_width = 120.0_f32.min(playback_rect.width());
    // Use the actual middle column width so the progress row fills the space
    // between the side sections without changing its centered position.
    let footer_width = playback_rect.width().max(0.0);
    let progress_width = footer_columns(footer_width).center.min(footer_width);

    // Treat the buttons and seek bar as one centered vertical control group.
    // Both rows use the footer midpoint as their horizontal midpoint.
    let content_height = TRANSPORT_HEIGHT + ROW_GAP + PROGRESS_HEIGHT;
    let content_top = playback_rect.center().y - content_height / 2.0;
    let content_top = content_top.max(playback_rect.top());
    let transport_top = content_top;
    let progress_top = transport_top + TRANSPORT_HEIGHT + ROW_GAP;
    let progress_top = progress_top.min(playback_rect.bottom() - PROGRESS_HEIGHT);

    let transport_bottom = transport_top + TRANSPORT_HEIGHT;
    debug_assert!((progress_top - transport_bottom - ROW_GAP).abs() < 0.1);

    FooterPlaybackRows {
        transport: egui::Rect::from_center_size(
            egui::pos2(
                playback_rect.center().x,
                transport_top + TRANSPORT_HEIGHT / 2.0,
            ),
            egui::vec2(transport_width, TRANSPORT_HEIGHT),
        ),
        progress: egui::Rect::from_center_size(
            egui::pos2(
                playback_rect.center().x,
                progress_top + PROGRESS_HEIGHT / 2.0,
            ),
            egui::vec2(progress_width, PROGRESS_HEIGHT),
        ),
    }
}

fn footer_geometry(footer_rect: egui::Rect) -> FooterGeometry {
    let columns = footer_columns(footer_rect.width());
    FooterGeometry {
        left: egui::Rect::from_min_size(
            footer_rect.min,
            egui::vec2(columns.left, footer_rect.height()),
        ),
        playback: egui::Rect::from_center_size(
            footer_rect.center(),
            egui::vec2(footer_rect.width(), footer_rect.height()),
        ),
        right: egui::Rect::from_min_size(
            egui::pos2(footer_rect.right() - columns.right, footer_rect.top()),
            egui::vec2(columns.right, footer_rect.height()),
        ),
    }
}

impl YtMusicApp {
    #[cfg(any())]
    fn render_tunetrek_layout(&mut self, ctx: &egui::Context) {
        let unit = (ctx.screen_rect().width() / 90.0).clamp(12.0, 16.0);
        let rem = |value: f32| value * unit;
        let canvas = design_color("#000000");
        let white = design_color("#121016");
        let orange = design_color("#BB86FC");
        let pale_orange = design_color("#241936");
        let ink = design_color("#F7F2FF");
        let secondary = design_color("#A99DB3");
        if self.icons.is_none() {
            self.icons = IconTextures::load(ctx).ok();
        }
        let icons = self.icons.clone();
        let available = ctx.screen_rect().size();
        let sidebar_width = rem(19.7).min(available.x * 0.24).max(rem(15.5));
        let player_width = rem(18.2).min(available.x * 0.25).max(rem(15.5));

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(canvas))
            .show(ctx, |ui| {
                ui.set_min_size(ui.available_size());
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(sidebar_width, ui.available_height()),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.add_space(rem(1.3));
                            ui.horizontal(|ui| {
                                ui.add_space(rem(1.8));
                                ui.label(egui::RichText::new("◉").size(rem(1.8)).color(orange));
                                ui.label(egui::RichText::new("TuneTrek").size(rem(1.5)).strong().color(ink));
                            });
                            ui.add_space(rem(2.2));
                            ui.add_space(rem(0.6));
                            ui.label(egui::RichText::new("MENU").size(rem(0.8)).color(secondary));
                            ui.add_space(rem(0.35));
                            let nav_items = [("⌂", "Home"), ("⌕", "Search"), ("♡", "Likes"), ("♬", "Playlists"), ("♫", "Albums"), ("♙", "Following")];
                            for (symbol, label) in nav_items {
                                let selected = label == "Home";
                                let fill = if selected { pale_orange } else { white };
                                egui::Frame::none().fill(fill).rounding(rem(0.45)).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        ui.add_space(rem(1.8));
                                        if label == "Home" {
                                            if let Some(icons) = &icons {
                                                ui.add(egui::Image::from_texture((icons.home.id(), egui::vec2(rem(1.25), rem(1.25)))).fit_to_exact_size(egui::vec2(rem(1.25), rem(1.25))).tint(if selected { orange } else { secondary }));
                                            }
                                        } else if label == "Playlists" {
                                            if let Some(icons) = &icons {
                                                ui.add(egui::Image::from_texture((icons.music.id(), egui::vec2(rem(1.25), rem(1.25)))).fit_to_exact_size(egui::vec2(rem(1.25), rem(1.25))).tint(secondary));
                                            }
                                        } else {
                                            ui.label(egui::RichText::new(symbol).size(rem(1.25)).color(if selected { orange } else { secondary }));
                                        }
                                        if ui.selectable_label(selected, egui::RichText::new(label).size(rem(0.9)).color(if selected { orange } else { secondary })).clicked() {
                                            self.status = label.to_string();
                                        }
                                    });
                                });
                                ui.add_space(rem(0.15));
                            }
                            ui.add_space(rem(1.2));
                            ui.label(egui::RichText::new("GENERAL").size(rem(0.8)).color(secondary));
                            ui.add_space(rem(0.35));
                            for label in ["⚙  Settings", "♛  Subscription", "↪  Log out"] {
                                ui.horizontal(|ui| {
                                    ui.add_space(rem(1.8));
                                    ui.label(egui::RichText::new(label).size(rem(0.9)).color(secondary));
                                });
                                ui.add_space(rem(0.55));
                            }
                            ui.add_space(rem(2.0));
                            ui.label(egui::RichText::new("GO MOBILE").size(rem(0.8)).color(secondary));
                            ui.horizontal(|ui| {
                                ui.add_space(rem(1.8));
                                ui.add_sized(egui::vec2(rem(7.2), rem(2.0)), egui::Button::new("●  App Store").fill(ink));
                                ui.add_sized(egui::vec2(rem(7.2), rem(2.0)), egui::Button::new("▶  Google Play").fill(ink));
                            });
                            ui.add_space(rem(0.8));
                            ui.label(egui::RichText::new("Legal · Privacy · Cookie Policy").size(rem(0.62)).color(secondary));
                            ui.label(egui::RichText::new("Blog · Manage · Impressum · Chart").size(rem(0.62)).color(secondary));
                            ui.label(egui::RichText::new("Language: English (US)").size(rem(0.62)).color(orange));
                        },
                    );

                    ui.allocate_ui_with_layout(
                        egui::vec2((ui.available_width() - player_width).max(rem(28.0)), ui.available_height()),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), rem(5.7)), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add_space(rem(2.0));
                                ui.label(egui::RichText::new("Discover").size(rem(1.35)).strong().color(ink));
                                ui.label(egui::RichText::new("›").size(rem(1.5)).color(ink));
                                ui.label(egui::RichText::new("Recently Played").size(rem(1.1)).color(secondary));
                                let search_width = ui.available_width().min(rem(18.0));
                                let response = ui.add_sized(egui::vec2(search_width, rem(2.2)), egui::TextEdit::singleline(&mut self.search).hint_text("Search music"));
                                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) { self.search(); }
                                if ui.add_sized(egui::vec2(rem(4.0), rem(2.2)), egui::Button::new("Search").fill(orange)).clicked() { self.search(); }
                                ui.add_space(rem(1.5));
                                ui.label(egui::RichText::new("Wade Warren").size(rem(0.85)).color(ink));
                                ui.small(egui::RichText::new("Premium").size(rem(0.65)).color(secondary));
                            });
                            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                                ui.add_space(rem(0.8));
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Charts: Top 50").size(rem(1.1)).color(ink));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(egui::RichText::new("‹  ›").size(rem(1.2)).color(secondary)); });
                                });
                                ui.add_space(rem(0.7));
                                ui.horizontal(|ui| {
                                    for (title, color) in [("AI Music Genre", "#241936"), ("Pop", "#512DA8"), ("Hip Hop", "#4527A0"), ("Rock", "#7E57C2"), ("R&B Soul", "#6A1B9A"), ("Country", "#9575CD")] {
                                        egui::Frame::none().fill(white).show(ui, |ui| {
                                            ui.set_min_width(rem(8.0));
                                            egui::Frame::none().fill(design_color(color)).rounding(rem(0.7)).show(ui, |ui| { ui.set_min_size(egui::vec2(rem(8.0), rem(7.5))); ui.label(egui::RichText::new("♪").size(rem(3.0)).color(white)); });
                                            ui.label(egui::RichText::new(title).size(rem(0.85)).color(ink));
                                            ui.small(egui::RichText::new("Top 50").size(rem(0.7)).color(secondary));
                                        });
                                        ui.add_space(rem(0.7));
                                    }
                                });
                                ui.add_space(rem(2.0));
                                egui::Frame::none().fill(white).rounding(rem(1.2)).inner_margin(rem(1.4)).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| { ui.label(egui::RichText::new("Listening History").size(rem(1.2)).color(ink)); ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(egui::RichText::new("See All").size(rem(0.8)).color(secondary)); }); });
                                    ui.add_space(rem(0.9));
                                    let from_search = !self.results.is_empty();
                                    let tracks = if from_search { self.results.clone() } else { self.queue.iter().cloned().collect::<Vec<_>>() };
                                    if tracks.is_empty() { ui.label(egui::RichText::new("Search for music to build your listening history").size(rem(0.9)).color(secondary)); }
                                    for track in tracks.iter().take(5) {
                                        ui.horizontal(|ui| {
                                            egui::Frame::none().fill(pale_orange).rounding(rem(0.6)).show(ui, |ui| { ui.set_min_size(egui::vec2(rem(4.2), rem(4.2))); ui.label(egui::RichText::new("♪").size(rem(1.8)).color(orange)); });
                                            ui.vertical(|ui| { ui.label(egui::RichText::new(&track.title).size(rem(0.9)).color(ink)); ui.small(egui::RichText::new(&track.channel).size(rem(0.75)).color(secondary)); });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.add_sized(egui::vec2(rem(4.2), rem(1.8)), egui::Button::new("Play").fill(pale_orange)).clicked() { if from_search { self.play_search_track(track.clone()); } else { self.play_queue_track(track.clone()); } } });
                                        });
                                        ui.add_space(rem(0.45));
                                    }
                                });
                            });
                        },
                    );

                    ui.allocate_ui_with_layout(egui::vec2(player_width, ui.available_height()), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        ui.add_space(rem(4.8));
                        egui::Frame::none().fill(white).rounding(rem(1.2)).inner_margin(rem(1.0)).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.vertical_centered(|ui| {
                                egui::Frame::none().fill(pale_orange).rounding(rem(0.7)).show(ui, |ui| { ui.set_min_size(egui::vec2(ui.available_width(), rem(13.2))); ui.label(egui::RichText::new("♪").size(rem(4.0)).color(orange)); });
                                if let Some(track) = &self.current { ui.label(egui::RichText::new(&track.title).size(rem(1.1)).strong().color(ink)); ui.label(egui::RichText::new(&track.channel).size(rem(0.8)).color(secondary)); } else { ui.label(egui::RichText::new("Nothing playing").size(rem(1.1)).strong().color(ink)); ui.label(egui::RichText::new("Choose a song to begin").size(rem(0.8)).color(secondary)); }
                                let duration = self.current.as_ref().and_then(|track| track.duration).unwrap_or(0.0);
                                let mut position = self.player.position_seconds().min(duration.max(0.0)) as f32;
                                ui.horizontal(|ui| { ui.small(format_position(position as f64)); let changed = ui.add_enabled(duration > 0.0, egui::Slider::new(&mut position, 0.0..=(duration.max(0.0) as f32)).show_value(false)).changed(); ui.small(format_duration(duration)); if changed { self.player.seek(position as f64); } });
                                ui.horizontal(|ui| {
                                    if let Some(icons) = &icons { if icon_button(ui, &icons.previous, rem(1.2), egui::vec2(rem(1.6), rem(1.6)), "Previous").clicked() { self.previous(); } }
                                    let pulse = ctx.animate_value_with_time(egui::Id::new("tunetrek_play"), if self.player.paused { 0.0 } else { 1.0 }, 0.25);
                                    let fill = egui::Color32::from_rgb(187, (134.0 + pulse * 15.0) as u8, 252);
                                    if let Some(icons) = &icons { let icon = if self.player.paused { &icons.play } else { &icons.pause }; if ui.add_sized(egui::vec2(rem(3.0), rem(3.0)), egui::Button::image(design_icon(icon, rem(1.5))).fill(fill)).clicked() { self.player.set_paused(!self.player.paused); } if icon_button(ui, &icons.next, rem(1.2), egui::vec2(rem(1.6), rem(1.6)), "Next").clicked() { self.next(); } }
                                });
                                ui.label(egui::RichText::new("Volume").size(rem(0.75)).color(secondary));
                                let changed = ui.add_sized(egui::vec2(ui.available_width(), rem(1.5)), egui::Slider::new(&mut self.player.volume, 0..=100).show_value(true)).changed();
                                if changed { self.player.send(&format!("[\"set_property\",\"volume\",{}]", self.player.volume)); }
                            });
                        });
                    });
                });
            });
    }

    #[cfg(any())]
    fn render_legacy_responsive_layout(&mut self, ctx: &egui::Context) {
        let theme = Theme::default();
        let background = theme.background;
        let header = theme.header;
        let surface = theme.surface;
        let raised = theme.raised;
        let border = theme.border;
        let text = theme.text;
        let muted = theme.secondary_text;
        let accent = theme.accent;
        if self.icons.is_none() {
            self.icons = IconTextures::load(ctx).ok();
        }
        let icons = self.icons.clone();

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(background))
            .show(ctx, |ui| {
                let available = ui.available_size();
                let unit = (available.x / 120.0).clamp(12.0, 16.0);
                let rem = |value: f32| value * unit;
                let layout = responsive_layout(available.x, available.y, unit);
                let header_height = layout.header_height;
                let footer_height = layout.footer_height;
                let body_height = layout.body_height;

                ui.allocate_ui_with_layout(
                    egui::vec2(available.x, header_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        egui::Frame::none().fill(header).show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            ui.horizontal_centered(|ui| {
                                ui.add_space(rem(0.5));
                                if ui.add_sized(egui::vec2(rem(3.94), rem(1.75)), egui::Button::new("Search").fill(raised)).clicked() {
                                    self.search();
                                }
                                let search_width = (ui.available_width() - rem(1.0)).max(rem(10.0));
                                let response = ui.add_sized(egui::vec2(search_width, rem(1.75)), egui::TextEdit::singleline(&mut self.search).hint_text("Search YouTube"));
                                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                                    self.search();
                                }
                            });
                        });
                    },
                );

                ui.allocate_ui_with_layout(
                    egui::vec2(available.x, body_height),
                    egui::Layout::left_to_right(egui::Align::TOP),
                    |ui| {
                        let sidebar_width = layout.sidebar_width;
                        let queue_width = layout.queue_width;
                        let center_width = layout.main_width.min(ui.available_width());

                        if layout.show_sidebar { ui.allocate_ui_with_layout(egui::vec2(sidebar_width, ui.available_height()), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.add_space(rem(1.06));
                            ui.label(egui::RichText::new("LIBRARY").size(rem(0.56)).color(muted));
                            ui.add_space(rem(0.31));
                            egui::Frame::none().fill(raised).rounding(rem(0.44)).show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.add_space(rem(0.65));
                                    if let Some(icons) = &icons {
                                        if icon_button(ui, &icons.home, rem(1.0), egui::vec2(rem(1.35), rem(1.75)), "Home").clicked() {
                                            self.status = "Home".into();
                                        }
                                    }
                                    if ui.selectable_label(false, "Home").clicked() { self.status = "Home".into(); }
                                });
                            });
                            if let Some(icons) = &icons {
                                let _ = ui.add_sized(egui::vec2(ui.available_width(), rem(1.75)), egui::Button::image_and_text(design_icon(&icons.music, rem(1.0)), "Search"));
                            }
                            ui.add_space(rem(0.15));
                            ui.label(egui::RichText::new(format!("Queue  {}", self.queue.len())).size(rem(0.75)).color(accent));
                            ui.separator();
                            ui.checkbox(&mut self.auto_mix, "Auto mix");
                            ui.add_space(rem(0.4));
                            ui.label(egui::RichText::new("YOUTUBE ACCESS").size(rem(0.56)).color(muted));
                            egui::ComboBox::from_id_salt("responsive_cookie_browser").selected_text(self.cookie_browser.label()).width(rem(5.3)).show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.cookie_browser, CookieBrowser::None, "Off");
                                ui.selectable_value(&mut self.cookie_browser, CookieBrowser::Chrome, "Chrome cookies");
                                ui.selectable_value(&mut self.cookie_browser, CookieBrowser::Edge, "Edge cookies");
                                ui.selectable_value(&mut self.cookie_browser, CookieBrowser::Firefox, "Firefox cookies");
                                ui.selectable_value(&mut self.cookie_browser, CookieBrowser::Helium, "Helium cookies");
                            });
                            ui.small(egui::RichText::new("Used only by yt-dlp when selected").size(rem(0.56)).color(muted));
                            ui.add_space(rem(0.5));
                            if let Some(error) = &self.error { ui.colored_label(egui::Color32::from_rgb(255, 135, 155), error); }
                        }); }

                        ui.allocate_ui_with_layout(egui::vec2(center_width, ui.available_height()), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.add_space(rem(1.56));
                            egui::Frame::none().fill(surface).rounding(rem(0.875)).inner_margin(egui::Margin::symmetric(rem(1.2), rem(1.56))).show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.label(egui::RichText::new("NOW PLAYING").size(rem(0.94)).color(accent));
                                ui.add_space(rem(0.94));
                                ui.horizontal(|ui| {
                                    egui::Frame::none().fill(raised).rounding(rem(0.5)).inner_margin(egui::Margin::symmetric(rem(1.44), rem(1.25))).show(ui, |ui| {
                                        if let Some(icons) = &icons { ui.add(egui::Image::from_texture((icons.music.id(), egui::vec2(rem(2.0), rem(3.0)))).fit_to_exact_size(egui::vec2(rem(2.0), rem(3.0))).tint(accent)); }
                                    });
                                    ui.vertical(|ui| {
                                        if let Some(track) = &self.current {
                                            ui.label(egui::RichText::new(&track.title).size(rem(1.125)).color(text));
                                            ui.label(egui::RichText::new(&track.channel).size(rem(0.75)).color(muted));
                                            ui.add_space(rem(0.5));
                                            ui.small(egui::RichText::new("Audio only playback").size(rem(0.5)).color(muted));
                                        } else {
                                            ui.label(egui::RichText::new("Choose something to play").size(rem(1.125)).color(text));
                                            ui.label(egui::RichText::new("Search YouTube to start your mix").size(rem(0.75)).color(muted));
                                        }
                                    });
                                });
                            });
                            ui.add_space(rem(1.125));
                            ui.label(egui::RichText::new("SEARCH RESULTS").size(rem(0.56)).color(text));
                            if self.results.is_empty() { ui.small(egui::RichText::new("Search for a song, artist, album, or YouTube URL to begin.").size(rem(0.56)).color(muted)); }
                            for track in self.results.clone() {
                                egui::Frame::none().fill(surface).rounding(rem(0.5)).inner_margin(rem(0.5)).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        if let Some(icons) = &icons { if icon_button(ui, &icons.play, rem(1.0), egui::vec2(rem(2.94), rem(1.75)), "Play").clicked() { self.play_search_track(track.clone()); } }
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(&track.title).size(rem(0.69)).color(text));
                                            ui.small(egui::RichText::new(format!("{} · {}", track.channel, track.duration_text())).size(rem(0.5)).color(muted));
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { if ui.add_sized(egui::vec2(rem(4.5), rem(1.75)), egui::Button::new("Add to mix").fill(accent)).clicked() { self.queue.push_back(track.clone()); } });
                                    });
                                });
                            }
                        });

                        if layout.show_queue { ui.allocate_ui_with_layout(egui::vec2(queue_width, ui.available_height()), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.add_space(rem(1.06));
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("UP NEXT").size(rem(0.75)).color(text));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| { ui.label(egui::RichText::new(if self.auto_mix { "MIX ON" } else { "MANUAL" }).size(rem(0.56)).color(accent)); });
                            });
                            ui.separator();
                            let snapshot: Vec<Track> = self.queue.iter().cloned().collect();
                            if snapshot.is_empty() { ui.small(egui::RichText::new("Your mix will appear here as songs are added.").size(rem(0.56)).color(muted)); }
                            let mut remove = None;
                            for (index, track) in snapshot.iter().enumerate() {
                                egui::Frame::none().fill(surface).rounding(rem(0.5)).inner_margin(rem(0.5)).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(format!("{:02}", index + 1)).size(rem(0.69)).color(accent));
                                        if ui.selectable_label(false, egui::RichText::new(&track.title).size(rem(0.69))).clicked() {
                                            if let Some(position) = self.queue.iter().position(|item| item.id == track.id) { self.queue.remove(position); }
                                            self.play_queue_track(track.clone());
                                        }
                                        if ui.add_sized(egui::vec2(rem(3.2), rem(1.06)), egui::Button::new("Remove")).clicked() { remove = Some(index); }
                                    });
                                    ui.small(egui::RichText::new(format!("{} · {}", track.channel, track.duration_text())).size(rem(0.5)).color(muted));
                                });
                            }
                            if let Some(index) = remove { self.queue.remove(index); }
                        }); }
                    },
                );

                ui.allocate_ui_with_layout(
                    egui::vec2(available.x, footer_height),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        egui::Frame::none().fill(background).stroke(egui::Stroke::new(rem(0.06), border)).show(ui, |ui| {
                            ui.set_min_size(ui.available_size());
                            let width = ui.available_width();
                            let info_width = (width * 0.3).max(rem(16.0));
                            let control_width = (width * 0.45).max(rem(22.0));
                            ui.allocate_ui_with_layout(egui::vec2(info_width, footer_height), egui::Layout::top_down(egui::Align::LEFT), |ui| {
                                ui.add_space(rem(0.56));
                                if let Some(track) = &self.current { ui.label(egui::RichText::new(&track.title).size(rem(1.0)).color(text)); ui.small(egui::RichText::new(&track.channel).size(rem(0.875)).color(muted)); ui.small(egui::RichText::new(&self.status).size(rem(0.75)).color(muted)); } else { ui.label(egui::RichText::new("Nothing playing").size(rem(1.0)).color(text)); ui.small(egui::RichText::new("Search for music to begin").size(rem(0.75)).color(muted)); }
                            });
                            ui.allocate_ui_with_layout(egui::vec2(control_width, footer_height), egui::Layout::top_down(egui::Align::Center), |ui| {
                                let duration = self.current.as_ref().and_then(|track| track.duration).unwrap_or(0.0);
                                let mut position = self.player.position_seconds().min(duration.max(0.0)) as f32;
                                ui.horizontal(|ui| { ui.small(format_position(position as f64)); let changed = ui.add_enabled(duration > 0.0, egui::Slider::new(&mut position, 0.0..=(duration.max(0.0) as f32)).show_value(false)).changed(); ui.small(format_duration(duration)); if changed { self.player.seek(position as f64); } });
                                ui.horizontal(|ui| {
                                    if let Some(icons) = &icons { if icon_button(ui, &icons.previous, rem(1.25), egui::vec2(rem(1.5), rem(1.5)), "Previous").clicked() { self.previous(); } }
                                    let pulse = ctx.animate_value_with_time(egui::Id::new("responsive_play_button"), if self.player.paused { 0.0 } else { 1.0 }, 0.25);
                                    let play_color = egui::Color32::from_rgb((208.0 + pulse * 15.0) as u8, (188.0 + pulse * 15.0) as u8, 255);
                                    if let Some(icons) = &icons { let icon = if self.player.paused { &icons.play } else { &icons.pause }; if ui.add_sized(egui::vec2(rem(2.0), rem(1.75)), egui::Button::image(design_icon(icon, rem(1.35))).fill(play_color)).clicked() { self.player.set_paused(!self.player.paused); } if icon_button(ui, &icons.next, rem(1.25), egui::vec2(rem(1.5), rem(1.5)), "Next").clicked() { self.next(); } }
                                    if ui.add_sized(egui::vec2(rem(1.5), rem(1.5)), egui::Button::new("■")).on_hover_text("Stop").clicked() { self.player.stop(); self.current = None; self.status = "Stopped".into(); }
                                });
                            });
                            ui.allocate_ui_with_layout(egui::vec2((width - info_width - control_width).max(rem(11.0)), footer_height), egui::Layout::top_down(egui::Align::LEFT), |ui| { ui.add_space(rem(1.3)); ui.horizontal(|ui| { ui.label(egui::RichText::new("Volume").size(rem(0.75)).color(text)); let changed = ui.add_sized(egui::vec2(rem(5.4), rem(1.5)), egui::Slider::new(&mut self.player.volume, 0..=100).show_value(true)).changed(); if changed { self.player.send(&format!("[\"set_property\",\"volume\",{}]", self.player.volume)); } }); });
                        });
                    },
                );
            });
    }

    fn render_responsive_layout(&mut self, ctx: &egui::Context) {
        if self.icons.is_none() {
            self.icons = IconTextures::load(ctx).ok();
        }
        let Some(icons) = self.icons.clone() else {
            self.error = Some("Could not load the bundled SVG icons".into());
            return;
        };
        let theme = Theme::default();
        let viewport = ctx.available_rect().size();
        let unit = (viewport.x / 120.0).clamp(12.0, 16.0);
        let rem = unit;
        let layout = responsive_layout(viewport.x, viewport.y, unit);
        let header = theme.header;
        let surface = theme.surface;
        let raised = theme.raised;
        let text = theme.text;
        let muted = theme.secondary_text;
        let accent = theme.accent;
        let header_height = layout.header_height;
        let show_sidebar = layout.show_sidebar;
        let show_queue = layout.show_queue;
        let sidebar_width = layout.sidebar_width;
        let queue_width = layout.queue_width;

        egui::TopBottomPanel::top("app_header")
            .exact_height(header_height)
            .show(ctx, |ui| {
                egui::Frame::none().fill(header).show(ui, |ui| {

ui.allocate_ui_with_layout(
    ui.available_size(),
    egui::Layout::top_down(egui::Align::Center),
    |ui| {
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                let button_width = 63.0;
                let gap = ui.spacing().item_spacing.x;
                let search_width = ui.available_width().clamp(180.0, 520.0);
                let total_width = search_width + gap + button_width;

                // Center the group horizontally.
                ui.add_space(
                    ((ui.available_width() - total_width) / 2.0).max(0.0),
                );

                let response = ui.add_sized(
                    [search_width, 28.0],
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search YouTube")
                        .horizontal_align(egui::Align::Center),
                );

                let search_clicked = ui
                    .add_sized(
                        [button_width, 28.0],
                        egui::Button::new("Search").fill(raised),
                    )
                    .clicked();

                if search_clicked
                    || (response.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    self.search();
                }
            },
        );
    },
);

                });
            });

        self.render_full_width_footer(ctx, &icons);

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
                                self.error = Some(error);
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label("Port");
                            ui.add_sized([70.0, 22.0], egui::TextEdit::singleline(&mut self.jam_port));
                        });
                        ui.add_sized([sidebar_width - 16.0, 22.0], egui::TextEdit::singleline(&mut self.jam_join_address).hint_text("Host address:port"));
                        ui.add_sized([sidebar_width - 16.0, 22.0], egui::TextEdit::singleline(&mut self.jam_join_token).hint_text("Join token"));
                        if ui.button("Join Jam").clicked() {
                            if let Err(error) = self.jam.join(&self.jam_join_address, &self.jam_join_token) {
                                self.error = Some(error);
                            }
                        }
                    } else {
                        ui.label(egui::RichText::new(&self.jam.info.status).size(10.0).color(muted));
                        if self.jam.info.role == Some(Role::Host) {
                            ui.small(format!("Share: {}", self.jam.info.address));
                            ui.small(format!("Token: {}", self.jam.info.token));
                            ui.small(format!("Guests: {}", self.jam.info.peers));
                        }
                        if ui.button(if self.jam.info.role == Some(Role::Host) { "End Jam" } else { "Leave Jam" }).clicked() {
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
                        ui.colored_label(egui::Color32::from_rgb(255, 135, 155), error);
                    }
                });
        }

        if show_queue {
            egui::SidePanel::right("app_queue")
                .resizable(false)
                .exact_width(queue_width)
                .show(ctx, |ui| {
                    ui.add_space(17.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("UP NEXT").size(12.0).color(text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(if self.auto_mix {
                                    "MIX ON"
                                } else {
                                    "MANUAL"
                                })
                                .size(9.0)
                                .color(accent),
                            );
                        });
                    });
                    ui.separator();
                    let queue_snapshot: Vec<Track> = self.queue.iter().cloned().collect();
                    if queue_snapshot.is_empty() {
                        ui.small(
                            egui::RichText::new("Your mix will appear here as songs are added.")
                                .size(9.0)
                                .color(muted),
                        );
                    }
                    let mut remove = None;
                    for (index, track) in queue_snapshot.iter().enumerate() {
                        egui::Frame::none()
                            .fill(surface)
                            .rounding(8.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("{:02}", index + 1))
                                            .size(11.0)
                                            .color(accent),
                                    );
                                    if ui
                                        .selectable_label(
                                            false,
                                            egui::RichText::new(&track.title).size(11.0),
                                        )
                                        .clicked()
                                    {
                                        if let Some(position) =
                                            self.queue.iter().position(|item| item.id == track.id)
                                        {
                                            self.queue.remove(position);
                                        }
                                        self.play_queue_track(track.clone());
                                    }
                                    if ui
                                        .add_sized([51.0, 17.0], egui::Button::new("Remove"))
                                        .clicked()
                                    {
                                        remove = Some(index);
                                    }
                                });
                                ui.small(
                                    egui::RichText::new(format!(
                                        "{} · {}",
                                        track.channel,
                                        track.duration_text()
                                    ))
                                    .size(8.0)
                                    .color(muted),
                                );
                            });
                    }
                    if let Some(index) = remove {
                        self.queue.remove(index);
                    }
                });
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Frame::none()
                        .fill(surface)
                        .rounding(14.0)
                        .inner_margin(egui::Margin::symmetric(19.0, 25.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("NOW PLAYING").size(15.0).color(accent));
                            ui.add_space(15.0);
                            ui.horizontal(|ui| {
                                egui::Frame::none()
                                    .fill(raised)
                                    .rounding(8.0)
                                    .inner_margin(egui::Margin::symmetric(23.0, 20.0))
                                    .show(ui, |ui| {
                                        ui.add(
                                            egui::Image::from_texture((
                                                icons.music.id(),
                                                egui::vec2(32.0, 48.0),
                                            ))
                                            .fit_to_exact_size(egui::vec2(32.0, 48.0))
                                            .tint(accent),
                                        );
                                    });
                                ui.vertical(|ui| {
                                    if let Some(track) = &self.current {
                                        ui.label(
                                            egui::RichText::new(&track.title)
                                                .size(18.0)
                                                .color(text),
                                        );
                                        ui.label(
                                            egui::RichText::new(&track.channel)
                                                .size(12.0)
                                                .color(muted),
                                        );
                                        ui.add_space(8.0);
                                        ui.small(
                                            egui::RichText::new("Audio only playback")
                                                .size(8.0)
                                                .color(muted),
                                        );
                                    } else {
                                        ui.label(
                                            egui::RichText::new("Choose something to play")
                                                .size(18.0)
                                                .color(text),
                                        );
                                        ui.label(
                                            egui::RichText::new("Search YouTube to start your mix")
                                                .size(12.0)
                                                .color(muted),
                                        );
                                    }
                                });
                            });
                        });
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("SEARCH RESULTS").size(9.0).color(text));
                    if self.results.is_empty() {
                        ui.small(
                            egui::RichText::new(
                                "Search for a song, artist, album, or YouTube URL to begin.",
                            )
                            .size(9.0)
                            .color(muted),
                        );
                    }
                    for track in self.results.clone() {
                        egui::Frame::none()
                            .fill(surface)
                            .rounding(8.0)
                            .inner_margin(8.0)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    if icon_button(
                                        ui,
                                        &icons.play,
                                        16.0,
                                        egui::vec2(47.0, 28.0),
                                        "Play",
                                    )
                                    .clicked()
                                    {
                                        self.play_search_track(track.clone());
                                    }
                                    ui.vertical(|ui| {
                                        ui.label(
                                            egui::RichText::new(&track.title)
                                                .size(11.0)
                                                .color(text),
                                        );
                                        ui.small(
                                            egui::RichText::new(format!(
                                                "{} · {}",
                                                track.channel,
                                                track.duration_text()
                                            ))
                                            .size(8.0)
                                            .color(muted),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add_sized(
                                                    [72.0, 28.0],
                                                    egui::Button::new("Add to mix").fill(accent),
                                                )
                                                .clicked()
                                            {
                                                self.queue.push_back(track.clone());
                                            }
                                        },
                                    );
                                });
                            });
                    }
                });
        });

        #[cfg(any())]
        {
            egui::TopBottomPanel::bottom("legacy_app_footer")
                .exact_height(footer_height)
                .show(ctx, |ui| {
                    egui::Frame::none()
                        .fill(background)
                        .stroke(egui::Stroke::new(1.0_f32, border))
                        .show(ui, |ui| {
                            ui.set_min_height(footer_height);
                            ui.horizontal(|ui| {
                                let available = ui.available_width();
                                let left_width =
                                    (available * 0.32).clamp(16.0 * rem, 30.0625 * rem);
                                let center_width =
                                    (available * 0.36).clamp(20.0 * rem, 26.625 * rem);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(left_width, 3.75 * rem),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        ui.add_space(9.0);
                                        if let Some(track) = &self.current {
                                            ui.label(
                                                egui::RichText::new(&track.title)
                                                    .size(16.0)
                                                    .color(text),
                                            );
                                            ui.small(
                                                egui::RichText::new(&track.channel)
                                                    .size(14.0)
                                                    .color(muted),
                                            );
                                            ui.small(
                                                egui::RichText::new(&self.status)
                                                    .size(12.0)
                                                    .color(muted),
                                            );
                                        } else {
                                            ui.label(
                                                egui::RichText::new("Nothing playing")
                                                    .size(16.0)
                                                    .color(text),
                                            );
                                            ui.small(
                                                egui::RichText::new("Search for music to begin")
                                                    .size(12.0)
                                                    .color(muted),
                                            );
                                        }
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2(center_width, 3.75 * rem),
                                    egui::Layout::top_down(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(5.0);
                                        let duration = self
                                            .current
                                            .as_ref()
                                            .and_then(|track| track.duration)
                                            .unwrap_or(0.0);
                                        let mut position =
                                            self.player.position_seconds().min(duration.max(0.0))
                                                as f32;
                                        ui.horizontal(|ui| {
                                            ui.small(format_position(position as f64));
                                            let changed = ui
                                                .add_enabled(
                                                    duration > 0.0,
                                                    egui::Slider::new(
                                                        &mut position,
                                                        0.0..=(duration.max(0.0) as f32),
                                                    )
                                                    .show_value(false),
                                                )
                                                .changed();
                                            ui.small(format_duration(duration));
                                            if changed {
                                                self.player.seek(position as f64);
                                            }
                                        });
                                        ui.horizontal(|ui| {
                                            if icon_button(
                                                ui,
                                                &icons.previous,
                                                20.0,
                                                egui::vec2(24.0, 24.0),
                                                "Previous",
                                            )
                                            .clicked()
                                            {
                                                self.previous();
                                            }
                                            let pulse = ctx.animate_value_with_time(
                                                egui::Id::new("design_play_button"),
                                                if self.player.paused { 0.0 } else { 1.0 },
                                                0.25,
                                            );
                                            let play_color = egui::Color32::from_rgb(
                                                (208.0 + pulse * 15.0) as u8,
                                                (188.0 + pulse * 15.0) as u8,
                                                (255.0) as u8,
                                            );
                                            let icon = if self.player.paused {
                                                &icons.play
                                            } else {
                                                &icons.pause
                                            };
                                            if ui
                                                .add_sized(
                                                    [32.0, 28.0],
                                                    egui::Button::image(design_icon(icon, 22.0))
                                                        .fill(play_color),
                                                )
                                                .clicked()
                                            {
                                                self.player.set_paused(!self.player.paused);
                                            }
                                            if icon_button(
                                                ui,
                                                &icons.next,
                                                20.0,
                                                egui::vec2(24.0, 24.0),
                                                "Next",
                                            )
                                            .clicked()
                                            {
                                                self.next();
                                            }
                                            if ui
                                                .add_sized([24.0, 24.0], egui::Button::new("■"))
                                                .on_hover_text("Stop")
                                                .clicked()
                                            {
                                                self.player.stop();
                                                self.current = None;
                                                self.status = "Stopped".into();
                                            }
                                        });
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2(
                                        (available - left_width - center_width).max(11.25 * rem),
                                        3.75 * rem,
                                    ),
                                    egui::Layout::top_down(egui::Align::LEFT),
                                    |ui| {
                                        ui.add_space(21.0);
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                egui::RichText::new("Volume")
                                                    .size(12.0)
                                                    .color(text),
                                            );
                                            let volume_changed = ui
                                                .add_sized(
                                                    [86.0, 24.0],
                                                    egui::Slider::new(
                                                        &mut self.player.volume,
                                                        0..=100,
                                                    )
                                                    .show_value(true),
                                                )
                                                .changed();
                                            if volume_changed {
                                                self.player.send(&format!(
                                                    "[\"set_property\",\"volume\",{}]",
                                                    self.player.volume
                                                ));
                                            }
                                        });
                                    },
                                );
                            });
                        });
                });
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
        return;

        #[cfg(any())]
        {
            egui::TopBottomPanel::top("top")
                .exact_height(64.0)
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let _ = ui.small_button("‹");
                        let _ = ui.small_button("›");
                        ui.add_space(12.0);
                        let response = ui.add_sized(
                            [420.0, 34.0],
                            egui::TextEdit::singleline(&mut self.search)
                                .hint_text("What do you want to play?"),
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
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(40, 40, 40))
                                .rounding(18.0)
                                .inner_margin(egui::Margin::symmetric(14.0, 7.0))
                                .show(ui, |ui| {
                                    ui.label(egui::RichText::new("MusiKPL").strong());
                                });
                        });
                    });
                });

            egui::SidePanel::left("queue")
                .resizable(false)
                .default_width(241.0)
                .show(ctx, |ui| {
                    ui.add_space(16.0);
                    ui.label(
                        egui::RichText::new(format!("Queue  {}", self.queue.len()))
                            .color(egui::Color32::from_rgb(30, 215, 96)),
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
                                egui::RichText::new(if self.auto_mix {
                                    "MIX ON"
                                } else {
                                    "MANUAL"
                                })
                                .size(10.0)
                                .color(egui::Color32::from_rgb(30, 215, 96)),
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
                            .fill(egui::Color32::from_rgb(24, 24, 24))
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
                                        self.play_queue_track(track.clone());
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
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(24, 24, 24))
                    .rounding(8.0)
                    .inner_margin(22.0)
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("NOW PLAYING")
                                .size(11.0)
                                .color(egui::Color32::from_rgb(30, 215, 96)),
                        );
                        ui.add_space(9.0);
                        ui.horizontal(|ui| {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(40, 40, 40))
                                .rounding(6.0)
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
                                    ui.add_space(8.0);
                                    ui.small(
                                        egui::RichText::new("Audio only playback")
                                            .color(egui::Color32::from_rgb(147, 145, 157)),
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
                        .fill(egui::Color32::from_rgb(24, 24, 24))
                        .rounding(6.0)
                        .inner_margin(10.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Play").clicked() {
                                    self.play_search_track(track.clone());
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
                .exact_height(112.0)
                .show(ctx, |ui| {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            if let Some(track) = &self.current {
                                ui.label(egui::RichText::new(&track.title).strong().size(14.0));
                                ui.small(&track.channel);
                                ui.small(&self.status);
                            } else {
                                ui.label(egui::RichText::new("Nothing playing").strong());
                                ui.small("Search for music to begin");
                            }
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            let duration = self
                                .current
                                .as_ref()
                                .and_then(|t| t.duration)
                                .unwrap_or(0.0);
                            let mut position =
                                self.player.position_seconds().min(duration.max(0.0)) as f32;
                            ui.horizontal(|ui| {
                                ui.small(format_position(position as f64));
                                let changed = ui
                                    .add_enabled(
                                        duration > 0.0,
                                        egui::Slider::new(
                                            &mut position,
                                            0.0..=(duration.max(0.0) as f32),
                                        )
                                        .show_value(false),
                                    )
                                    .changed();
                                ui.small(format_duration(duration));
                                if changed {
                                    self.player.seek(position as f64);
                                }
                            });
                            ui.horizontal(|ui| {
                                ui.add_space(65.0);
                                if ui.add_sized([32.0, 28.0], egui::Button::new("⏮")).clicked() {
                                    self.previous();
                                }
                                let pulse = ctx.animate_value_with_time(
                                    egui::Id::new("bottom_play_button"),
                                    if self.player.paused { 0.0 } else { 1.0 },
                                    0.25,
                                );
                                let play_color = egui::Color32::from_rgb(
                                    (30.0 + pulse * 12.0) as u8,
                                    (215.0 + pulse * 18.0) as u8,
                                    (96.0 + pulse * 16.0) as u8,
                                );
                                if ui
                                    .add_sized(
                                        [42.0, 34.0],
                                        egui::Button::new(if self.player.paused {
                                            "▶"
                                        } else {
                                            "Ⅱ"
                                        })
                                        .fill(play_color),
                                    )
                                    .clicked()
                                {
                                    self.player.set_paused(!self.player.paused);
                                }
                                if ui.add_sized([32.0, 28.0], egui::Button::new("⏭")).clicked() {
                                    self.next();
                                }
                                if ui.add_sized([32.0, 28.0], egui::Button::new("■")).clicked() {
                                    self.player.stop();
                                    self.current = None;
                                    self.status = "Stopped".into();
                                }
                            });
                        });
                        ui.separator();
                        ui.label("Volume");
                        let volume_changed = ui
                            .add_sized(
                                [180.0, 24.0],
                                egui::Slider::new(&mut self.player.volume, 0..=100)
                                    .show_value(true),
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
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(YtMusicApp::default()))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::youtube::parse_ytdlp_entries;

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
        assert_eq!(format_position(0.0), "00:00");
        assert_eq!(format_position(125.9), "02:05");
    }

    #[test]
    fn responsive_layout_collapses_secondary_panels_at_narrow_widths() {
        let compact = responsive_layout(800.0, 600.0, 12.0);
        assert!(!compact.show_sidebar);
        assert!(!compact.show_queue);
        assert_eq!(compact.main_width, 800.0);

        let desktop = responsive_layout(1280.0, 720.0, 12.0);
        assert!(desktop.show_sidebar);
        assert!(desktop.show_queue);
        assert!(desktop.main_width > 0.0);
        assert!(desktop.body_height > 0.0);
    }

    #[test]
    fn responsive_layout_fills_remaining_height_without_forcing_overflow() {
        for (width, height) in [
            (800.0, 600.0),
            (1280.0, 720.0),
            (1920.0, 1080.0),
            (1600.0, 600.0),
            (700.0, 1000.0),
        ] {
            let layout = responsive_layout(width, height, 12.0);
            assert!(layout.body_height > 0.0);
            assert!(layout.main_width > 0.0);
            assert!(layout.header_height + layout.footer_height <= height);
        }
    }

    #[test]
    fn footer_columns_keep_playback_centered_in_the_full_window() {
        for width in [800.0, 1280.0, 1920.0] {
            let columns = footer_columns(width);
            assert!((columns.left + columns.center + columns.right - width).abs() < f32::EPSILON);
            assert!((columns.left - columns.right).abs() < 0.001 || width < 900.0);
        }

        let compact = footer_columns(600.0);
        assert_eq!(compact.left, 0.0);
        assert_eq!(compact.right, 0.0);
        assert_eq!(compact.center, 600.0);

        let medium = footer_columns(800.0);
        assert!(medium.left > 0.0);
        assert!(medium.right > 0.0);
        assert!(medium.center > 0.0);
    }

    #[test]
    fn footer_geometry_centers_playback_and_keeps_zones_disjoint() {
        for width in [600.0, 800.0, 1280.0, 1920.0] {
            let footer =
                egui::Rect::from_min_size(egui::pos2(37.0, 700.0), egui::vec2(width, 88.0));
            let geometry = footer_geometry(footer);
            let tolerance = 0.001;
            assert!((geometry.playback.center().x - footer.center().x).abs() < tolerance);
            assert!(footer.contains(geometry.left.min));
            assert!(footer.contains(geometry.playback.min));
            assert!(footer.contains(geometry.right.min));
            assert!(geometry.left.right() <= geometry.playback.left() + tolerance);
            assert!(geometry.playback.right() <= geometry.right.left() + tolerance);
            assert!((geometry.right.right() - footer.right()).abs() < tolerance);
        }
    }

    #[test]
    fn footer_playback_rows_fit_and_stay_centered() {
        for size in [(800.0, 76.0), (1280.0, 88.0), (1920.0, 104.0)] {
            let footer =
                egui::Rect::from_min_size(egui::pos2(37.0, 500.0), egui::vec2(size.0, size.1));
            let rows = footer_playback_rows(footer);
            let tolerance = 0.001;

            assert!((rows.transport.center().x - footer.center().x).abs() < tolerance);
            assert!((rows.progress.center().x - footer.center().x).abs() < tolerance);
            assert!(footer.contains(rows.transport.min));
            assert!(footer.contains(rows.transport.max));
            assert!(footer.contains(rows.progress.min));
            assert!(footer.contains(rows.progress.max));
            assert!(rows.transport.bottom() <= rows.progress.top() + tolerance);
            assert!((rows.transport.center().x - rows.progress.center().x).abs() < tolerance);
            assert!((rows.transport.width() - rows.progress.width()).abs() < tolerance);
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
        assert_eq!(app.queue.back().map(|song| song.id.as_str()), Some("new"));
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
        use crate::process::run_registered;
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
