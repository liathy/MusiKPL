mod actions;
mod automix;
mod errors;
mod jam_sync;
mod library;
mod settings;
mod updates;

use crate::{
    domain::{song_info, CoverArt, Track},
    infrastructure::{process::ProcessRegistry, settings::Settings},
    jam::{JamController, Role},
    lastfm::{lookup_key, LastFm, ScrobbleTracker},
    media::{query_ytdlp, resolve_audio_url, Tools},
    playback::{Player, Queue},
    recommendations::History,
    theme::Theme,
    ui::{
        footer::{self, FooterState},
        home::{self, HomeAction, HomeRowView, HomeView},
        icons::IconTextures,
        layout::window_layout,
        overlays::{self, MenuAction, MenuView, QueueAction, QueueView, SettingsPage},
        responsive::ResponsiveUi,
        screens::{self, CardContext, ResultAction, ResultsView},
        topbar::{self, TopBarAction, TopBarState},
    },
};
use eframe::egui;
use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use self::{
    automix::AutoMix, errors::AppError, jam_sync::GuestSync, library::Library,
    settings::LastFmConnect, updates::Updates,
};

/// A resolved stream: the track, its URL, and whether the track it
/// replaces belongs in the history.
type PlayResult = Result<(Track, String, bool), String>;

/// Songs that may fail to load in a row before the player gives up instead
/// of skipping to the next one.
const MAX_FAILED_SKIPS: u32 = 3;

/// What the main area shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum View {
    #[default]
    Home,
    Search,
}

struct YtMusicApp {
    icons: Option<IconTextures>,
    tools: Tools,
    registry: ProcessRegistry,
    player: Player,
    search: String,
    /// The query the current results belong to.
    results_query: String,
    results: Vec<Track>,
    queue: Queue,
    history: History,
    current: Option<Track>,
    error: Option<AppError>,
    auto_mix: bool,
    automix: AutoMix,
    searching: bool,
    loading: bool,
    finished_handled: bool,
    /// Songs in a row that failed to load.
    failed_plays: u32,
    search_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    play_rx: Option<Receiver<PlayResult>>,
    jam: JamController,
    jam_sync: GuestSync,
    jam_notice: Option<(String, Instant)>,
    /// The track currently being resolved, so a Jam host can announce it early.
    pending_track: Option<Track>,
    /// Host: when a new track started waiting, paused, for guests to load it.
    jam_hold: Option<Instant>,
    /// Adding the Windows Firewall rule for Jams, waiting on the user's
    /// approval in the Windows prompt.
    firewall_rx: Option<Receiver<Result<(), String>>>,
    /// The invite being pasted to join a Jam over the internet.
    jam_invite_input: String,
    view: View,
    /// The docked menu and queue; both may be open, and they stay open.
    menu_open: bool,
    queue_open: bool,
    settings_open: bool,
    /// The user's taste, likes, session, and Home.
    library: Library,
    updates: Updates,
    /// The last session's song is shown paused at this position but not
    /// loaded yet; pressing play loads it from here.
    resume_at: Option<f64>,
    /// Where the song being loaded starts.
    start_at: f64,
    /// The first-run (or Settings) genre picker is showing, with its picks.
    genre_picks: Option<Vec<String>>,
    settings_page: SettingsPage,
    settings: Settings,
    /// Problem to show on the settings screen.
    settings_error: Option<String>,
    lastfm: LastFm,
    lastfm_connect: LastFmConnect,
    /// Settings form fields, saved when the user connects.
    lastfm_key_draft: String,
    lastfm_secret_draft: String,
    scrobbler: ScrobbleTracker,
    cover_art: CoverArt,
    /// Cover images no longer on screen, to drop from egui's image cache.
    stale_images: Vec<String>,
    /// When the previous frame started, for the frame-rate cap.
    last_frame: Option<Instant>,
}

/// Shortest time between frames: at most 60 per second, whatever asks for
/// repaints (animations, scrolling text, mouse movement). Without a cap egui
/// draws as fast as the GPU allows when vsync doesn't apply.
const MIN_FRAME_TIME: Duration = Duration::from_micros(16_667);

/// How often to wake up with nothing to animate, by situation.
const IDLE_TICK: Duration = Duration::from_secs(1);
const PLAYING_TICK: Duration = Duration::from_millis(250);
const JAM_HOST_TICK: Duration = Duration::from_millis(100);
/// A guest corrects drift on frame boundaries, so it needs frequent ticks.
const JAM_GUEST_TICK: Duration = Duration::from_millis(20);

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
            results_query: String::new(),
            results: vec![],
            queue: Queue::default(),
            history: History::default(),
            current: None,
            error: None,
            auto_mix: true,
            automix: AutoMix::default(),
            searching: false,
            loading: false,
            finished_handled: false,
            failed_plays: 0,
            search_rx: None,
            play_rx: None,
            jam: JamController::default(),
            jam_sync: GuestSync::default(),
            jam_notice: None,
            pending_track: None,
            jam_hold: None,
            firewall_rx: None,
            jam_invite_input: String::new(),
            view: View::Home,
            menu_open: false,
            queue_open: false,
            settings_open: false,
            library: Library::default(),
            updates: Updates::default(),
            resume_at: None,
            start_at: 0.0,
            genre_picks: None,
            settings_page: SettingsPage::General,
            settings: Settings::default(),
            settings_error: None,
            lastfm: LastFm::default(),
            lastfm_connect: LastFmConnect::Idle,
            lastfm_key_draft: String::new(),
            lastfm_secret_draft: String::new(),
            scrobbler: ScrobbleTracker::default(),
            cover_art: CoverArt::default(),
            stale_images: Vec::new(),
            last_frame: None,
        }
    }
}

impl YtMusicApp {
    fn search(&mut self) {
        let query = self.search.trim().to_owned();
        if query.is_empty() || self.searching {
            return;
        }
        self.searching = true;
        self.error = None;
        self.view = View::Search;
        self.settings.remember_search(&query);
        self.library.settings_changed = true;
        self.results_query = query.clone();
        let tools = self.tools.clone();
        let single = query.starts_with("http://") || query.starts_with("https://");
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        self.search_rx = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(query_ytdlp(&tools, &query, single, &registry));
        });
    }

    /// Play a search result: a fresh start, so Auto Mix builds a new mix
    /// around it (songs the user queued stay).
    fn play_search_track(&mut self, track: Track) {
        if self.is_guest() {
            return;
        }
        self.reset_mix();
        self.failed_plays = 0;
        self.start_track(track, true);
    }

    fn play_queue_track(&mut self, track: Track) {
        if self.is_guest() {
            return;
        }
        self.start_track(track, true);
    }

    fn start_track(&mut self, track: Track, remember_current: bool) {
        self.start_track_at(track, remember_current, 0.0);
    }

    fn start_track_at(&mut self, track: Track, remember_current: bool, start_at: f64) {
        self.player.stop();
        self.resume_at = None;
        self.start_at = start_at;
        self.loading = true;
        self.automix.waiting = false;
        self.pending_track = Some(track.clone());
        self.jam_hold = None;
        self.error = None;
        self.finished_handled = false;
        let tools = self.tools.clone();
        let (sender, receiver) = mpsc::channel();
        self.play_rx = Some(receiver);
        let registry = self.registry.clone();
        std::thread::spawn(move || {
            let result = resolve_audio_url(&tools, &track, &registry)
                .map(|url| (track, url, remember_current));
            let _ = sender.send(result);
        });
    }

    /// Play the next queued song. With nothing queued, Auto Mix keeps the
    /// session going: the player waits for its next pick instead of ending.
    fn next(&mut self) {
        if self.is_guest() {
            return;
        }
        if let Some(track) = self.queue.pop_next() {
            self.play_queue_track(track);
            return;
        }
        self.player.stop();
        self.resume_at = None;
        self.play_rx = None;
        self.loading = false;
        if let Some(pending) = self.pending_track.take() {
            // A skipped song that never started still seeds the mix.
            self.current.get_or_insert(pending);
        }
        if self.auto_mix && self.current.is_some() {
            self.automix.waiting = true;
            // The user is waiting now, so don't hold off a retry.
            self.automix.retry_at = None;
        } else {
            self.current = None;
        }
    }

    /// A song could not be loaded: skip it, up to a few times in a row.
    fn skip_failed_track(&mut self, error: String) {
        self.error = Some(AppError::Playback(error));
        self.failed_plays += 1;
        if self.failed_plays < MAX_FAILED_SKIPS {
            self.next();
        }
    }

    fn previous(&mut self) {
        if self.is_guest() {
            return;
        }
        if let Some(track) = self.history.pop() {
            if let Some(current) = self.current.take() {
                self.queue.push_front(current);
            }
            self.start_track(track, false);
        }
    }

    fn poll_background_work(&mut self) {
        let search_result = self.search_rx.as_ref().and_then(|rx| rx.try_recv().ok());
        if let Some(result) = search_result {
            self.search_rx = None;
            self.searching = false;
            match result {
                Ok(items) => {
                    let old = std::mem::replace(&mut self.results, items);
                    self.retire_covers(old);
                }
                Err(e) => self.error = Some(AppError::Search(e)),
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
                    Ok((track, url, _)) => self.finish_guest_load(track, &url),
                    Err(error) => self.fail_guest_load(None, error),
                }
                return;
            }
            match result {
                Ok((track, url, remember_current)) => {
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
                    let start_at = std::mem::take(&mut self.start_at);
                    match self
                        .player
                        .play_stream_at(&track, &url, &self.tools.mpv, start_at, hold)
                    {
                        Ok(()) => {
                            self.jam_hold = hold.then(Instant::now);
                            self.error = None;
                            self.finished_handled = false;
                            self.failed_plays = 0;
                        }
                        Err(e) => self.skip_failed_track(e),
                    }
                }
                Err(e) => self.skip_failed_track(e),
            }
        }
    }

    /// Advance to the next song when one ends. Auto Mix has normally queued
    /// it already; if not, `next` waits for it.
    fn handle_finished_track(&mut self) {
        if self.is_guest() || !self.player.is_finished() || self.finished_handled || self.loading {
            return;
        }
        self.finished_handled = true;
        self.next();
    }

    /// Mark covers of replaced search results for removal from the image
    /// cache, unless the song is still playing or queued. Every cover is a
    /// decoded image plus a GPU texture, so a long session would otherwise
    /// keep every search it ever showed.
    fn retire_covers(&mut self, old: Vec<Track>) {
        for track in old {
            let still_shown = self.results.iter().any(|t| t.id == track.id)
                || self.library.home_tracks().any(|t| t.id == track.id)
                || self.queue.contains_id(&track.id)
                || self.current.as_ref().is_some_and(|t| t.id == track.id)
                || self
                    .pending_track
                    .as_ref()
                    .is_some_and(|t| t.id == track.id);
            if !still_shown {
                if let Some(cover) = self.cover_art.cover_for(&track) {
                    self.stale_images.push(cover.url);
                }
            }
        }
    }

    /// How long until the next frame is needed when nothing animates.
    fn idle_tick(&self) -> Duration {
        match self.jam.info.role {
            Some(Role::Guest) => JAM_GUEST_TICK,
            Some(Role::Host) => JAM_HOST_TICK,
            // The clock and seek bar move, and the end of a song must be
            // noticed promptly to start the next one.
            None if self.current.is_some() && !self.player.paused => PLAYING_TICK,
            // Background work reports through channels polled each frame.
            None if self.searching
                || self.loading
                || self.automix.busy()
                || self.automix.waiting
                || self.updates.busy()
                || self.library.building_home() =>
            {
                PLAYING_TICK
            }
            None => IDLE_TICK,
        }
    }

    /// The song shown in the player bar: the one loading, or the one playing.
    fn shown_track(&self) -> Option<&Track> {
        if self.loading {
            self.pending_track.as_ref().or(self.current.as_ref())
        } else {
            self.current.as_ref()
        }
    }

    /// Ask Last.fm for album art of every song on screen, once each.
    fn request_covers(&mut self) {
        let Some(api_key) = lookup_key(&self.settings.lastfm.api_key) else {
            return;
        };
        let home = (self.view == View::Home).then(|| self.library.home_tracks());
        let results = (self.view == View::Search).then_some(self.results.iter());
        let tracks = self
            .current
            .iter()
            .chain(self.pending_track.iter())
            .chain(results.into_iter().flatten())
            .chain(home.into_iter().flatten())
            .chain(self.queue.iter().take(20));
        for track in tracks {
            if self.cover_art.has_looked_up(&track.id) {
                continue;
            }
            self.cover_art.mark_pending(&track.id);
            self.lastfm.submit(crate::lastfm::LastFmJob::AlbumArt {
                api_key: api_key.clone(),
                video_id: track.id.clone(),
                song: song_info(track),
            });
        }
    }

    /// Play or pause. The last session's song is loaded on the first play,
    /// from where it was left.
    fn toggle_pause(&mut self) {
        if let Some(position) = self.resume_at {
            if let Some(track) = self.current.clone() {
                self.failed_plays = 0;
                self.start_track_at(track, false, position);
                return;
            }
            self.resume_at = None;
        }
        // Pressing play while waiting for guests starts right away.
        self.jam_hold = None;
        self.player.set_paused(!self.player.paused);
    }

    fn seek(&mut self, position: f64) {
        if self.resume_at.is_some() {
            self.resume_at = Some(position);
            self.player.show_paused_at(position);
        } else {
            self.player.seek(position);
        }
    }

    fn set_volume(&mut self, volume: u8) {
        self.player.volume = volume;
        self.player
            .send(&format!("[\"set_property\",\"volume\",{volume}]"));
        self.settings.volume = volume;
        self.library.settings_changed = true;
    }

    fn apply_card_action(&mut self, action: ResultAction) {
        let guest = self.is_guest();
        match action {
            ResultAction::ToggleLike(track) => self.toggle_like(&track),
            // A guest's picks become requests; only the host's queue plays.
            ResultAction::Play(track) | ResultAction::AddToMix(track) if guest => {
                self.request_from_host(&track);
            }
            ResultAction::Play(track) => self.play_search_track(track),
            ResultAction::AddToMix(track) => self.add_to_queue(track),
        }
    }

    fn apply_home_action(&mut self, action: HomeAction) {
        match action {
            HomeAction::Card(action) => self.apply_card_action(action),
            HomeAction::Refresh => self.refresh_home(true),
            HomeAction::ToggleGenre(genre) => {
                if let Some(picks) = &mut self.genre_picks {
                    if let Some(index) = picks.iter().position(|pick| *pick == genre) {
                        picks.remove(index);
                    } else {
                        picks.push(genre);
                    }
                }
            }
            HomeAction::FinishGenres => {
                let picks = self.genre_picks.take().unwrap_or_default();
                let changed = picks != self.settings.favorite_genres;
                self.settings.favorite_genres = picks;
                self.finish_onboarding(changed);
            }
            HomeAction::SkipGenres => {
                self.genre_picks = None;
                self.finish_onboarding(false);
            }
        }
    }

    fn finish_onboarding(&mut self, genres_changed: bool) {
        self.settings.onboarded = true;
        self.library.settings_changed = true;
        self.refresh_home(genres_changed);
    }

    /// Open the genre picker on Home, starting from the saved picks.
    fn pick_genres(&mut self) {
        self.genre_picks = Some(self.settings.favorite_genres.clone());
        self.view = View::Home;
        self.settings_open = false;
    }
}

impl Drop for YtMusicApp {
    fn drop(&mut self) {
        self.save_library(true);
        self.player.stop();
        self.registry.terminate_all();
    }
}

impl YtMusicApp {
    fn render(&mut self, ctx: &egui::Context) {
        if self.icons.is_none() {
            match IconTextures::load(ctx) {
                Ok(icons) => self.icons = Some(icons),
                Err(error) => {
                    self.error = Some(AppError::Ui(format!(
                        "Could not load the bundled icons: {error}"
                    )));
                    return;
                }
            }
        }
        let Some(icons) = self.icons.clone() else {
            return;
        };
        let window = ctx.screen_rect();
        let r = ResponsiveUi::from_size(window.size());
        let layout = window_layout(window, &r);
        let theme = Theme::default();
        let guest = self.is_guest();

        let mut top_actions = Vec::new();
        let mut card_actions = Vec::new();
        let mut home_actions = Vec::new();
        let mut footer_actions = Vec::new();
        let mut menu_actions = Vec::new();
        let mut menu_data = None;
        let mut queue_actions = Vec::new();
        let menu_open = overlays::open_amount(ctx, "menu", self.menu_open);
        let queue_open = overlays::open_amount(ctx, "queue", self.queue_open);
        // The docked panels take their width from the main area, which
        // shrinks to make room instead of being covered.
        let content = egui::Rect::from_min_max(
            egui::pos2(
                layout.main.left() + overlays::menu_width(&r) * menu_open,
                layout.main.top(),
            ),
            egui::pos2(
                layout.main.right() - overlays::queue_width(&r) * queue_open,
                layout.main.bottom(),
            ),
        );
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(theme.background))
            .show(ctx, |ui| {
                top_actions = topbar::show(
                    ui,
                    layout.top_bar,
                    &mut self.search,
                    &TopBarState {
                        menu_open: self.menu_open,
                        home_open: self.view == View::Home,
                        recent: &self.settings.recent_searches,
                    },
                    &icons,
                    &r,
                );
                ui.painter().line_segment(
                    [layout.main.left_top(), layout.main.right_top()],
                    egui::Stroke::new(1.0_f32, theme.border),
                );

                let queue = &self.queue;
                let in_mix = |id: &str| queue.contains_id(id);
                let taste = &self.library.taste;
                let liked = |id: &str| taste.is_liked(id);
                let cards = CardContext {
                    guest,
                    covers: &self.cover_art,
                    in_mix: &in_mix,
                    liked: &liked,
                };
                match self.view {
                    View::Home => {
                        home_actions = home::show(
                            ui,
                            content,
                            &HomeView {
                                rows: self.home_rows(),
                                building: self.library.building_home(),
                                picking_genres: self.genre_picks.as_deref(),
                                cards,
                            },
                            &icons,
                            &r,
                        );
                    }
                    View::Search => {
                        card_actions = screens::show_results(
                            ui,
                            content,
                            &ResultsView {
                                results: &self.results,
                                query: &self.results_query,
                                searching: self.searching,
                                guest,
                                covers: &self.cover_art,
                                in_mix: &in_mix,
                                liked: &liked,
                            },
                            &icons,
                            &r,
                        );
                    }
                }

                if menu_open > 0.0 {
                    let data = self.jam_panel_data();
                    // The view borrows the app, so the invite field is
                    // edited on the side and put back afterwards.
                    let mut invite = std::mem::take(&mut self.jam_invite_input);
                    let actions = overlays::show_menu(
                        ui,
                        layout.main,
                        menu_open,
                        &icons,
                        &MenuView {
                            auto_mix: self.auto_mix,
                            jam: self.jam_view(&data),
                            update: self.updates.view(),
                        },
                        &mut invite,
                        &r,
                    );
                    self.jam_invite_input = invite;
                    menu_actions = actions;
                    menu_data = Some(data);
                }
                if queue_open > 0.0 {
                    let now_playing = self.shown_track().cloned();
                    let snapshot: Vec<Track> = self.queue.iter().cloned().collect();
                    queue_actions = overlays::show_queue(
                        ui,
                        layout.main,
                        queue_open,
                        &icons,
                        &QueueView {
                            now_playing: now_playing.as_ref(),
                            queue: &snapshot,
                            mixing: self.automix.busy(),
                            guest,
                            covers: &self.cover_art,
                        },
                        &r,
                    );
                }

                let shown = self.shown_track();
                let duration = self
                    .current
                    .as_ref()
                    .and_then(|track| track.duration)
                    .unwrap_or(0.0);
                let position = if duration > 0.0 && !self.loading {
                    self.player.position_seconds().min(duration)
                } else {
                    0.0
                };
                footer_actions = footer::show(
                    ui,
                    layout.footer,
                    &icons,
                    &FooterState {
                        track: shown,
                        // Waiting for Auto Mix's next song looks like loading.
                        loading: self.loading || self.automix.waiting,
                        cover: shown.and_then(|track| self.cover_art.cover_for(track)),
                        paused: self.player.paused,
                        position,
                        duration: if self.loading { 0.0 } else { duration },
                        volume: self.player.volume,
                        guest,
                        queue_open: self.queue_open,
                        liked: shown.is_some_and(|track| self.is_liked(&track.id)),
                    },
                    &r,
                );
            });

        for action in top_actions {
            match action {
                TopBarAction::ToggleMenu => self.menu_open = !self.menu_open,
                TopBarAction::Home => {
                    self.view = View::Home;
                    self.refresh_home(false);
                }
                TopBarAction::Search => self.search(),
                TopBarAction::SearchFor(query) => {
                    self.search = query;
                    self.search();
                }
                TopBarAction::StartDrag => ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag),
                TopBarAction::Minimize => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true))
                }
                TopBarAction::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        for action in card_actions {
            self.apply_card_action(action);
        }
        for action in home_actions {
            self.apply_home_action(action);
        }
        for action in menu_actions {
            match action {
                MenuAction::Close => self.menu_open = false,
                MenuAction::OpenSettings => {
                    self.settings_page = SettingsPage::General;
                    self.settings_error = None;
                    self.settings_open = true;
                }
                MenuAction::ToggleAutoMix => self.set_auto_mix(!self.auto_mix),
                MenuAction::Jam(action) => {
                    if let Some(data) = &menu_data {
                        self.apply_jam_action(action, data);
                    }
                }
                MenuAction::CheckForUpdates => self.check_for_updates(),
                MenuAction::InstallUpdate => self.install_update(),
            }
        }
        for action in queue_actions {
            match action {
                QueueAction::Close => self.queue_open = false,
                QueueAction::Play(track) => {
                    self.queue.remove_id(&track.id);
                    self.play_queue_track(track);
                }
                QueueAction::Remove(index) => {
                    self.queue.remove(index);
                }
            }
        }
        for action in footer_actions {
            actions::apply_footer_action(self, action);
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.settings_open = false;
        }
        let settings = overlays::open_amount(ctx, "settings", self.settings_open);
        if settings > 0.0 {
            self.render_settings(ctx, window, settings, &icons, &r);
        }

        // The settings window covers the main area and shows its own errors.
        let error = self.error.as_ref().filter(|_| !self.settings_open);
        if let Some(error) = error {
            if screens::error_toast(ctx, content, &error.to_string(), &icons, &r) {
                self.error = None;
            }
        }

        // A hairline around the borderless window.
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("window_border"),
        ))
        .rect_stroke(
            window.shrink(0.5),
            0.0,
            egui::Stroke::new(1.0_f32, theme.border),
        );
    }

    /// Home's rows: the first two built from the user's favorites, then
    /// Liked songs, the rest (Discover last), and Recently played.
    fn home_rows(&self) -> Vec<HomeRowView<'_>> {
        let built: Vec<HomeRowView<'_>> = self
            .library
            .home
            .rows
            .iter()
            .map(|row| HomeRowView {
                title: &row.title,
                tracks: &row.tracks,
            })
            .collect();
        let split = built.len().min(2);
        let mut rows = Vec::with_capacity(built.len() + 2);
        let mut built = built.into_iter();
        rows.extend(built.by_ref().take(split));
        if !self.library.liked.is_empty() {
            rows.push(HomeRowView {
                title: "Liked songs",
                tracks: &self.library.liked,
            });
        }
        rows.extend(built);
        if !self.library.recent.is_empty() {
            rows.push(HomeRowView {
                title: "Recently played",
                tracks: &self.library.recent,
            });
        }
        rows
    }
}

impl eframe::App for YtMusicApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Cap the frame rate: if this frame comes too soon after the last,
        // wait out the difference instead of drawing extra frames.
        if let Some(last) = self.last_frame {
            if let Some(wait) = MIN_FRAME_TIME.checked_sub(last.elapsed()) {
                std::thread::sleep(wait);
            }
        }
        self.last_frame = Some(Instant::now());

        self.player.refresh_position();
        self.poll_background_work();
        self.poll_jam();
        self.poll_lastfm();
        self.update_scrobbling();
        self.tick_library();
        self.poll_updates(ctx);
        self.request_covers();
        self.handle_finished_track();
        self.poll_mix();
        self.keep_mix_filled();
        for uri in self.stale_images.drain(..) {
            ctx.forget_image(&uri);
        }
        crate::ui::widgets::repaint_after(ctx, self.idle_tick());

        // Nothing is visible while minimized: keep playback, Jam, and
        // scrobbling running above, but skip building the interface.
        let minimized = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
        if !minimized {
            self.render(ctx);
        }
    }
}

/// Visual defaults for egui widgets that the design does not draw itself
/// (text cursors, selections, tooltips, scroll bars, spinners).
fn apply_style(ctx: &egui::Context) {
    let theme = Theme::default();
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(theme.text);
    visuals.panel_fill = theme.background;
    visuals.window_fill = theme.panel;
    visuals.window_stroke = egui::Stroke::new(1.0_f32, theme.border);
    visuals.extreme_bg_color = egui::Color32::BLACK;
    visuals.faint_bg_color = theme.panel;
    visuals.selection.bg_fill = egui::Color32::from_white_alpha(60);
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, theme.text);
    visuals.text_cursor.stroke = egui::Stroke::new(2.0_f32, theme.text);
    visuals.widgets.noninteractive.fg_stroke.color = theme.secondary_text;
    visuals.widgets.inactive.bg_fill = theme.surface;
    visuals.widgets.inactive.weak_bg_fill = theme.surface;
    visuals.widgets.hovered.bg_fill = theme.hover;
    visuals.widgets.hovered.weak_bg_fill = theme.hover;
    visuals.widgets.active.bg_fill = theme.hover;
    visuals.widgets.active.weak_bg_fill = theme.hover;
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.scroll = egui::style::ScrollStyle::floating();
        style.interaction.selectable_labels = false;
    });
}

pub fn run() -> eframe::Result<()> {
    // The first run of a new build unpacks its mpv and yt-dlp; do it before
    // the window exists so it never shows a frozen window.
    crate::infrastructure::bundled::unpack();
    let (size, position) = initial_window();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("MusiKPL")
        .with_inner_size(size)
        .with_resizable(false)
        .with_maximize_button(false)
        .with_decorations(false);
    if let Some(position) = position {
        viewport = viewport.with_position(position);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "MusiKPL",
        options,
        Box::new(|cc| {
            round_window_corners(cc);
            crate::ui::fonts::install(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            apply_style(&cc.egui_ctx);
            let mut app = YtMusicApp::default();
            app.load_settings();
            app.load_library();
            if !app.settings.onboarded && !app.library.taste.has_history() {
                app.genre_picks = Some(Vec::new());
            }
            app.jam.start_discovery();
            crate::infrastructure::bundled::update_yt_dlp_in_background(
                app.tools.yt_dlp.clone(),
                app.registry.clone(),
            );
            Ok(Box::new(app))
        }),
    )
}

/// A design-proportioned window that fits the screen, centered on it.
fn initial_window() -> (egui::Vec2, Option<egui::Pos2>) {
    use crate::ui::responsive::{window_size, DESIGN_SIZE};
    match crate::infrastructure::system::work_area() {
        Some([left, top, width, height]) => {
            let size = window_size(egui::vec2(width, height));
            let position = egui::pos2(left + (width - size.x) / 2.0, top + (height - size.y) / 2.0);
            (size, Some(position))
        }
        None => (DESIGN_SIZE * 0.8, None),
    }
}

/// Without the system title bar, ask Windows 11 to keep rounded corners.
#[cfg(windows)]
fn round_window_corners(cc: &eframe::CreationContext<'_>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{ffi::c_void, mem::size_of};
    use windows_sys::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
    };

    let Ok(window_handle) = cc.window_handle() else {
        return;
    };
    let RawWindowHandle::Win32(window) = window_handle.as_raw() else {
        return;
    };
    let hwnd = window.hwnd.get() as HWND;
    let preference = DWMWCP_ROUND;
    // SAFETY: a valid window handle and a pointer to a correctly sized value.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &preference as *const _ as *const c_void,
            size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(windows))]
fn round_window_corners(_: &eframe::CreationContext<'_>) {}

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

    /// An app whose yt-dlp and mpv don't exist, so loads fail fast and
    /// nothing reaches the network.
    fn offline_app() -> YtMusicApp {
        let mut app = YtMusicApp::default();
        app.tools.yt_dlp = "missing-yt-dlp-for-tests".into();
        app.tools.mpv = "missing-mpv-for-tests".into();
        app
    }

    fn queued_ids(app: &YtMusicApp) -> Vec<String> {
        app.queue.iter().map(|track| track.id.clone()).collect()
    }

    /// Stand in for a finished mix search.
    fn deliver_mix(app: &mut YtMusicApp, result: Result<Vec<Track>, String>) {
        let (sender, receiver) = mpsc::channel();
        app.automix.rx = Some(receiver);
        sender.send(result).unwrap();
        app.poll_mix();
    }

    #[test]
    fn mix_picks_skip_what_was_played_queued_or_is_playing() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));
        app.history.push(track("played", "Played"));
        app.queue.push_back(track("queued", "Already queued"));

        let added = app.queue_mix_picks(vec![
            track("current", "Current"),
            track("played", "Played"),
            track("queued", "Already queued"),
            track("lyrics", "Current (Lyrics)"),
            track("new", "New song"),
        ]);

        assert_eq!(added, 1);
        assert_eq!(queued_ids(&app), ["queued", "new"]);
        assert!(app.automix.picks.contains("new"));
    }

    #[test]
    fn a_new_search_keeps_the_users_songs_and_drops_old_mix_picks() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));
        app.queue_mix_picks(vec![
            track("pick-1", "Pick one"),
            track("pick-2", "Pick two"),
        ]);
        app.add_to_queue(track("mine", "My song"));

        app.reset_mix();

        assert_eq!(queued_ids(&app), ["mine"]);
        assert!(!app.automix.busy() && !app.automix.waiting);
    }

    #[test]
    fn songs_the_user_adds_play_before_mix_picks() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));
        app.queue_mix_picks(vec![track("pick", "Pick")]);
        app.add_to_queue(track("mine-1", "Mine one"));
        app.add_to_queue(track("mine-2", "Mine two"));
        assert_eq!(queued_ids(&app), ["mine-1", "mine-2", "pick"]);
    }

    #[test]
    fn adding_a_song_with_nothing_playing_plays_it() {
        let mut app = offline_app();
        app.add_to_queue(track("mine", "Mine"));
        assert!(app.loading);
        assert_eq!(
            app.pending_track.as_ref().map(|t| t.id.as_str()),
            Some("mine")
        );
        assert!(app.queue.is_empty());
    }

    #[test]
    fn an_empty_queue_waits_for_auto_mix_then_plays_its_first_song() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));

        app.next();
        assert!(app.automix.waiting, "the session keeps going");
        assert_eq!(app.current.as_ref().map(|t| t.id.as_str()), Some("current"));

        app.keep_mix_filled();
        assert!(app.automix.busy(), "a refill starts right away");

        deliver_mix(
            &mut app,
            Ok(vec![track("next", "Next"), track("after", "After")]),
        );
        assert!(!app.automix.waiting);
        assert!(app.loading);
        assert_eq!(
            app.pending_track.as_ref().map(|t| t.id.as_str()),
            Some("next")
        );
        assert_eq!(queued_ids(&app), ["after"]);
    }

    #[test]
    fn a_failed_or_crashed_refill_is_retried_later() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));
        app.next();

        deliver_mix(&mut app, Err("YouTube had no Mix".into()));
        assert!(!app.automix.busy());
        assert_eq!(app.automix.failures, 1);
        assert!(app.automix.retry_at.is_some());
        assert!(matches!(app.error, Some(AppError::Mix(_))));
        // Waiting for the retry, not hammering yt-dlp every frame.
        app.keep_mix_filled();
        assert!(!app.automix.busy());

        // A worker that disappears without answering doesn't jam Auto Mix.
        let (sender, receiver) = mpsc::channel::<Result<Vec<Track>, String>>();
        app.automix.rx = Some(receiver);
        drop(sender);
        app.poll_mix();
        assert!(!app.automix.busy());
        assert_eq!(app.automix.failures, 2);
    }

    #[test]
    fn the_queue_is_refilled_before_it_runs_out() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));
        app.queue_mix_picks(vec![track("a", "A"), track("b", "B"), track("c", "C")]);
        app.keep_mix_filled();
        assert!(!app.automix.busy(), "enough songs are waiting");

        app.queue.pop_next();
        app.keep_mix_filled();
        assert!(app.automix.busy(), "refilled while the song still plays");
    }

    #[test]
    fn turning_auto_mix_off_ends_the_wait() {
        let mut app = offline_app();
        app.current = Some(track("current", "Current"));
        app.next();
        app.keep_mix_filled();

        app.set_auto_mix(false);
        assert!(!app.automix.waiting && !app.automix.busy());
        assert!(app.current.is_none());
        app.keep_mix_filled();
        assert!(!app.automix.busy());
    }

    #[test]
    fn songs_that_fail_to_load_are_skipped_a_few_times_at_most() {
        let mut app = offline_app();
        for id in ["a", "b", "c", "d", "e"] {
            app.queue.push_back(track(id, id));
        }
        for _ in 0..MAX_FAILED_SKIPS + 2 {
            app.skip_failed_track("stream unavailable".into());
        }
        // Two skips, then it stops instead of burning through the queue.
        assert_eq!(app.queue.len(), 3);
    }

    #[test]
    fn the_app_starts_on_home_and_searching_switches_to_results() {
        let mut app = offline_app();
        assert_eq!(app.view, View::Home);
        app.search = "hardstyle".into();
        app.search();
        assert_eq!(app.view, View::Search);
        assert_eq!(app.settings.recent_searches, ["hardstyle"]);
    }

    #[test]
    fn a_resumed_song_loads_from_where_it_was_left_on_play() {
        let mut app = offline_app();
        app.current = Some(track("last", "Last"));
        app.player.show_paused_at(80.0);
        app.resume_at = Some(80.0);

        app.seek(95.0);
        assert_eq!(app.resume_at, Some(95.0));
        assert!(!app.loading, "seeking doesn't load anything yet");

        app.toggle_pause();
        assert!(app.loading);
        assert_eq!(app.resume_at, None);
        assert_eq!(app.start_at, 95.0);
        assert_eq!(
            app.pending_track.as_ref().map(|t| t.id.as_str()),
            Some("last")
        );
    }

    #[test]
    fn the_player_bar_shows_the_song_being_loaded() {
        let mut app = YtMusicApp::default();
        app.current = Some(track("old", "Old"));
        app.pending_track = Some(track("new", "New"));
        app.loading = true;
        assert_eq!(app.shown_track().map(|t| t.id.as_str()), Some("new"));
        app.loading = false;
        assert_eq!(app.shown_track().map(|t| t.id.as_str()), Some("old"));
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
