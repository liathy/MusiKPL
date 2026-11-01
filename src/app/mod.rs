//! The desktop app: an egui window around the shared [`Engine`].
//!
//! The engine (in `musikpl-core`, shared with Android) holds what plays and
//! everything the app knows; this shell draws it, turns clicks into engine
//! calls, and adds what only the desktop has: the docked menu and queue,
//! the settings window, the tray, updates, and the window itself.

mod actions;
mod browse_ui;
mod friends_ui;
mod jam_ui;
mod playlists_ui;
mod settings_ui;
mod updates;

use std::{
    ops::{Deref, DerefMut},
    sync::{mpsc::Receiver, Arc},
    time::{Duration, Instant},
};

use eframe::egui;
use musikpl_core::engine::{self as core_engine, playlist_cover, unix_now, AppError, Engine, View};

use crate::{
    domain::Track,
    infrastructure::{mpv::MpvOutput, process::ProcessRegistry},
    media::{Tools, YtDlp},
    theme::Theme,
    ui::{
        artist::{self, ArtistView},
        footer::{self, FooterState},
        home::{self, HomeAction, HomeRowView, HomeView},
        icons::IconTextures,
        layout::window_layout,
        menu::PlaylistItem,
        menu::PlaylistItemAction,
        overlays::{self, MenuAction, MenuProfile, MenuView, QueueAction, QueueView, SettingsPage},
        playlist::{self, PlaylistView},
        profile::{self, ProfileView},
        responsive::ResponsiveUi,
        screens::{self, CardContext, PersonCard, ResultAction, ResultsView},
        topbar::{self, TopBarAction, TopBarState},
    },
};

use self::updates::Updates;

/// Shortest time between frames: at most 60 per second, whatever asks for
/// repaints (animations, scrolling text, mouse movement). Without a cap egui
/// draws as fast as the GPU allows when vsync doesn't apply.
const MIN_FRAME_TIME: Duration = Duration::from_micros(16_667);

struct YtMusicApp {
    engine: Engine,
    icons: Option<IconTextures>,
    /// mpv and yt-dlp, and the processes started from them.
    tools: Tools,
    registry: ProcessRegistry,
    /// The docked menu and queue; both may be open, and they stay open.
    menu_open: bool,
    queue_open: bool,
    settings_open: bool,
    updates: Updates,
    /// Which menu cards are expanded.
    menu_playlists_open: bool,
    menu_jam_open: bool,
    menu_artists_open: bool,
    menu_friends_open: bool,
    /// A playlist cover photo being chosen.
    cover_pick: Option<playlists_ui::CoverPick>,
    /// The tray icon while MusiKPL is closed to the tray.
    tray: Option<crate::infrastructure::tray::Tray>,
    /// Really closing (Quit from the tray, an update): skip close to tray.
    quitting: bool,
    /// Adding the Windows Firewall rule for Jams, waiting on the user's
    /// approval in the Windows prompt.
    firewall_rx: Option<Receiver<Result<(), String>>>,
    settings_page: SettingsPage,
    /// Settings form fields, saved when the user connects.
    lastfm_key_draft: String,
    lastfm_secret_draft: String,
    /// When the previous frame started, for the frame-rate cap.
    last_frame: Option<Instant>,
}

/// The engine's state and methods read as the app's own.
impl Deref for YtMusicApp {
    type Target = Engine;
    fn deref(&self) -> &Engine {
        &self.engine
    }
}

impl DerefMut for YtMusicApp {
    fn deref_mut(&mut self) -> &mut Engine {
        &mut self.engine
    }
}

impl YtMusicApp {
    /// The app around an engine that plays through mpv and finds songs with
    /// yt-dlp. `waker` repaints the window when background work has news.
    fn new(waker: musikpl_core::platform::Waker) -> Self {
        let registry = ProcessRegistry::default();
        let tools = Tools::discover();
        let output = MpvOutput::new(tools.mpv.clone(), registry.clone());
        let extractor = Arc::new(YtDlp::new(tools.clone(), registry.clone()));
        Self {
            engine: Engine::new(Box::new(output), extractor, waker),
            icons: None,
            tools,
            registry,
            menu_open: false,
            queue_open: false,
            settings_open: false,
            updates: Updates::default(),
            menu_playlists_open: true,
            menu_jam_open: false,
            menu_artists_open: true,
            menu_friends_open: true,
            cover_pick: None,
            tray: None,
            quitting: false,
            firewall_rx: None,
            settings_page: SettingsPage::General,
            lastfm_key_draft: String::new(),
            lastfm_secret_draft: String::new(),
            last_frame: None,
        }
    }

    /// Load everything saved and start the engine's background work.
    fn start(&mut self) {
        self.engine.start();
        // The registry is the truth; the user may have changed it elsewhere.
        self.engine.settings.start_with_windows =
            crate::infrastructure::system::starts_with_windows();
        self.lastfm_key_draft = self.engine.settings.lastfm.api_key.clone();
        self.lastfm_secret_draft = self.engine.settings.lastfm.api_secret.clone();
    }

    /// How long until the next frame is needed when nothing animates.
    fn idle_tick(&self) -> Duration {
        let busy = self.updates.busy() || self.cover_pick.is_some();
        let tick = self.engine.idle_tick();
        if busy {
            tick.min(Duration::from_millis(250))
        } else {
            tick
        }
    }

    fn apply_card_action(&mut self, action: ResultAction) {
        let guest = self.is_guest();
        let engine = &mut self.engine;
        match action {
            ResultAction::ToggleLike(track) => engine.toggle_like(&track),
            ResultAction::OpenArtist(name) => engine.open_artist(&name),
            ResultAction::OpenUser(name) => engine.open_user_profile(&name),
            ResultAction::PlayLastFm { artist, title } => {
                engine.find_and_play(&artist, &title, core_engine::SongIntent::Play)
            }
            ResultAction::AddLastFm { artist, title } => {
                engine.find_and_play(&artist, &title, core_engine::SongIntent::AddToMix)
            }
            ResultAction::AddToPlaylist(track, target) => engine.add_to_playlist(track, target),
            // A guest's picks become requests; only the host's queue plays.
            ResultAction::Play(track) | ResultAction::AddToMix(track) if guest => {
                engine.request_from_host(&track);
            }
            ResultAction::Play(track) => engine.play_search_track(track),
            ResultAction::AddToMix(track) => engine.add_to_queue(track),
        }
    }

    fn apply_home_action(&mut self, action: HomeAction) {
        match action {
            HomeAction::Card(action) => self.apply_card_action(action),
            HomeAction::Refresh => self.engine.refresh_home(true),
            HomeAction::OpenPlaylist(id) => self.engine.open_playlist(id),
            HomeAction::MoreInRow(title) => self.engine.more_in_home_row(&title),
            HomeAction::MoreRows => self.engine.more_rows(),
            HomeAction::ToggleGenre(genre) => self.engine.toggle_genre(genre),
            HomeAction::FinishGenres => self.engine.finish_genres(),
            HomeAction::SkipGenres => self.engine.skip_genres(),
        }
    }

    /// Open the genre picker on Home (from Settings, which closes).
    fn pick_genres(&mut self) {
        self.engine.pick_genres();
        self.settings_open = false;
    }
}

impl Drop for YtMusicApp {
    fn drop(&mut self) {
        self.engine.shut_down();
        self.registry.terminate_all();
    }
}
impl YtMusicApp {
    fn render(&mut self, ctx: &egui::Context) {
        if self.icons.is_none() {
            match IconTextures::load(ctx) {
                Ok(icons) => self.icons = Some(icons),
                Err(error) => {
                    self.engine.error = Some(AppError::Ui(format!(
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
        let mut playlist_actions = Vec::new();
        let mut artist_actions = Vec::new();
        let mut profile_actions = Vec::new();
        let mut name_draft = std::mem::take(&mut self.engine.playlist_name_draft);
        // Artist photos are looked up on demand (and queued here), so they
        // are gathered before drawing borrows the app.
        let photos = self.gather_photos();
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let mut resize = None;
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
                    &mut self.engine.search,
                    &TopBarState {
                        menu_open: self.menu_open,
                        home_open: self.engine.view == View::Home,
                        maximized,
                        can_back: self.engine.nav.can_back(),
                        can_forward: self.engine.nav.can_forward(),
                        recent: &self.engine.settings.recent_searches,
                    },
                    &icons,
                    &r,
                );
                ui.painter().line_segment(
                    [layout.main.left_top(), layout.main.right_top()],
                    egui::Stroke::new(1.0_f32, theme.border),
                );

                let queue = &self.engine.queue;
                let in_mix = |id: &str| queue.contains_id(id);
                let taste = &self.engine.library.taste;
                let liked = |id: &str| taste.is_liked(id);
                let cards = CardContext {
                    guest,
                    covers: &self.engine.cover_art,
                    in_mix: &in_mix,
                    liked: &liked,
                    playlists: &self.engine.library.playlists,
                };
                let open_playlist = self
                    .open_playlist_id()
                    .and_then(|id| self.engine.library.playlists.get(id));
                match self.engine.view {
                    View::Playlist => match open_playlist {
                        Some(playlist) => {
                            let suggestions = self.playlist_recs();
                            playlist_actions = playlist::show(
                                ui,
                                content,
                                &PlaylistView {
                                    playlist,
                                    editing: self.engine.playlist_editing,
                                    guest,
                                    covers: &self.engine.cover_art,
                                    liked: &liked,
                                    playing: self.shown_track().map(|t| t.id.as_str()),
                                    cover: playlist_cover(playlist),
                                    suggestions: &suggestions,
                                    suggestions_loading: self.engine.playlist_recs.loading(),
                                },
                                &mut name_draft,
                                &icons,
                                &r,
                            );
                        }
                        None => self.engine.view = View::Home,
                    },
                    View::Home => {
                        home_actions = home::show(
                            ui,
                            content,
                            &HomeView {
                                rows: self.home_rows(),
                                building: self.engine.library.building_home(),
                                picking_genres: self.engine.genre_picks.as_deref(),
                                cards,
                                loading_rows: self.engine.library.paging.loading_rows(),
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
                                results: &self.engine.results,
                                query: &self.engine.results_query,
                                searching: self.engine.searching,
                                guest,
                                covers: &self.engine.cover_art,
                                in_mix: &in_mix,
                                liked: &liked,
                                playlists: &self.engine.library.playlists,
                                artists: &self.engine.browse.artists,
                                artist_photos: &photos.search,
                                lastfm_songs: &self.engine.browse.tracks,
                                finding_song: self.engine.browse.finding_song(),
                                people: &self
                                    .browse
                                    .people
                                    .iter()
                                    .map(|person| PersonCard {
                                        name: &person.name,
                                        scrobbles: person.scrobbles,
                                        avatar: person.image.clone().map(|url| {
                                            crate::domain::Cover {
                                                url,
                                                uv: [0.0, 0.0, 1.0, 1.0],
                                            }
                                        }),
                                    })
                                    .collect::<Vec<_>>(),
                            },
                            &icons,
                            &r,
                        );
                    }
                    View::Artist => {
                        artist_actions = artist::show_artist(
                            ui,
                            content,
                            &ArtistView {
                                name: &self.engine.browse.artist_name,
                                page: self.engine.browse.artist.as_ref(),
                                photo: photos.artist.clone(),
                                bookmarked: self
                                    .engine
                                    .browse
                                    .is_bookmarked(&self.engine.browse.artist_name),
                                finding_song: self.engine.browse.finding_song(),
                                similar_photos: photos.similar.clone(),
                                guest,
                                playlists: &self.engine.library.playlists,
                            },
                            &icons,
                            &r,
                        );
                    }
                    View::Profile => {
                        let is_me =
                            self.engine.browse.profile_user.is_empty() || self.profile_is_mine();
                        let username = if !is_me {
                            self.engine.browse.profile_user.as_str()
                        } else if self.lastfm_connected() {
                            self.engine.settings.lastfm.username.as_str()
                        } else {
                            ""
                        };
                        profile_actions = profile::show(
                            ui,
                            content,
                            &ProfileView {
                                username,
                                profile: self.engine.browse.profile.as_ref(),
                                loading: self.engine.browse.profile_loading,
                                period: self.engine.browse.period,
                                avatar: photos.avatar.clone(),
                                artist_photos: photos.top_artists.clone(),
                                now: unix_now(),
                                is_me,
                                is_friend: self.engine.friends.is_friend(username),
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
                    let mut invite = std::mem::take(&mut self.engine.jam_invite_input);
                    let mut friend_code = std::mem::take(&mut self.engine.friends.code_input);
                    let actions = overlays::show_menu(
                        ui,
                        layout.main,
                        menu_open,
                        &icons,
                        &MenuView {
                            jam: self.jam_view(&data),
                            jam_open: self.menu_jam_open,
                            playlists: self
                                .library
                                .playlists
                                .iter()
                                .map(|playlist| PlaylistItem {
                                    id: playlist.id,
                                    name: &playlist.name,
                                    songs: playlist.tracks.len(),
                                    favorite: playlist.favorite,
                                    selected: self.engine.view == View::Playlist
                                        && self.engine.open_playlist == Some(playlist.id),
                                    cover: playlist_cover(playlist),
                                })
                                .collect(),
                            playlists_open: self.menu_playlists_open,
                            profile: MenuProfile {
                                username: if self.lastfm_connected() {
                                    &self.engine.settings.lastfm.username
                                } else {
                                    ""
                                },
                                scrobbles: self.engine.browse.my_scrobbles,
                                avatar: photos.my_avatar.clone(),
                            },
                            artists: self
                                .browse
                                .bookmarks
                                .iter()
                                .zip(photos.bookmarks.iter())
                                .map(|(bookmark, photo)| (bookmark.name.as_str(), photo.clone()))
                                .collect(),
                            artists_open: self.menu_artists_open,
                            friends: self.friends_view(),
                            friends_open: self.menu_friends_open,
                            update: self.updates.view(),
                        },
                        &mut invite,
                        &mut friend_code,
                        &r,
                    );
                    self.engine.jam_invite_input = invite;
                    self.engine.friends.code_input = friend_code;
                    menu_actions = actions;
                    menu_data = Some(data);
                }
                if queue_open > 0.0 {
                    let now_playing = self.shown_track().cloned();
                    // Guests see the host's queue.
                    let snapshot: Vec<Track> = if guest {
                        self.jam_queue()
                    } else {
                        self.engine.queue.iter().cloned().collect()
                    };
                    queue_actions = overlays::show_queue(
                        ui,
                        layout.main,
                        queue_open,
                        &icons,
                        &QueueView {
                            now_playing: now_playing.as_ref(),
                            queue: &snapshot,
                            mixing: !guest && self.engine.automix.busy(),
                            guest,
                            covers: &self.engine.cover_art,
                            playlists: &self.engine.library.playlists,
                            liked: &|id: &str| self.engine.library.taste.is_liked(id),
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
                let position = if duration > 0.0 && !self.engine.loading {
                    self.engine.player.position_seconds().min(duration)
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
                        loading: self.engine.loading || self.engine.automix.waiting,
                        cover: shown.and_then(|track| self.engine.cover_art.cover_for(track)),
                        paused: self.engine.player.paused,
                        position,
                        duration: if self.engine.loading { 0.0 } else { duration },
                        volume: self.engine.player.volume,
                        guest,
                        queue_open: self.queue_open,
                        liked: shown.is_some_and(|track| self.is_liked(&track.id)),
                        playlists: &self.engine.library.playlists,
                    },
                    &r,
                );
                // Last, so the edges win over what lies under them.
                if !maximized {
                    resize = crate::ui::layout::resize_edges(ui, window);
                }
            });

        for action in top_actions {
            match action {
                TopBarAction::ToggleMenu => {
                    self.menu_open = !self.menu_open;
                    if self.menu_open {
                        self.refresh_friends();
                    }
                }
                TopBarAction::Home => {
                    self.engine.view = View::Home;
                    self.refresh_home(false);
                }
                TopBarAction::Back => self.go_back(),
                TopBarAction::Forward => self.go_forward(),
                TopBarAction::Search => self.search(),
                TopBarAction::SearchFor(query) => {
                    self.engine.search = query;
                    self.search();
                }
                TopBarAction::StartDrag => ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag),
                TopBarAction::Minimize => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true))
                }
                TopBarAction::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                TopBarAction::ToggleMaximize => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized))
                }
            }
        }
        self.engine.playlist_name_draft = name_draft;
        for action in playlist_actions {
            self.apply_playlist_action(action);
        }
        if let Some(direction) = resize {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
        for action in artist_actions {
            self.apply_artist_action(action);
        }
        for action in profile_actions {
            self.apply_profile_action(action);
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
                    self.engine.settings_error = None;
                    self.settings_open = true;
                }
                MenuAction::Jam(action) => {
                    if let Some(data) = &menu_data {
                        self.apply_jam_action(action, data);
                    }
                }
                MenuAction::NewPlaylist => {
                    self.menu_playlists_open = true;
                    self.new_playlist();
                }
                MenuAction::Playlist(PlaylistItemAction::Open(id)) => self.open_playlist(id),
                MenuAction::Playlist(PlaylistItemAction::Delete(id)) => self.delete_playlist(id),
                MenuAction::Playlist(PlaylistItemAction::ToggleFavorite(id)) => {
                    if self.engine.library.playlists.toggle_favorite(id) {
                        self.engine.library.playlists_changed = true;
                    }
                }
                MenuAction::TogglePlaylists => self.menu_playlists_open = !self.menu_playlists_open,
                MenuAction::ToggleJam => self.menu_jam_open = !self.menu_jam_open,
                MenuAction::ToggleArtists => self.menu_artists_open = !self.menu_artists_open,
                MenuAction::ToggleFriends => self.menu_friends_open = !self.menu_friends_open,
                MenuAction::Friend(action) => self.apply_friend_action(action),
                MenuAction::OpenProfile => self.open_profile(),
                MenuAction::OpenArtist(name) => self.open_artist(&name),
                MenuAction::ConnectLastFm => self.connect_lastfm_from_ui(),
                MenuAction::CheckForUpdates => self.check_for_updates(),
                MenuAction::InstallUpdate => self.install_update(),
            }
        }
        for action in queue_actions {
            match action {
                QueueAction::Close => self.queue_open = false,
                QueueAction::Play(track) => {
                    self.engine.queue.remove_id(&track.id);
                    self.play_queue_track(track);
                }
                QueueAction::Remove(index) => {
                    self.engine.queue.remove(index);
                }
                QueueAction::AddToPlaylist(track, target) => self.add_to_playlist(track, target),
                QueueAction::ToggleLike(track) => self.toggle_like(&track),
            }
        }
        for action in footer_actions {
            actions::apply_footer_action(self, action);
        }

        if let Some(text) = self.engine.clipboard.take() {
            ctx.copy_text(text);
        }
        // A friend's Jam invite, as a pop-up when that's the chosen style.
        if self.engine.settings.invite_popups {
            if let Some(invite) = self.engine.friends.invites.first().cloned() {
                match crate::ui::menu::invite_popup(ctx, content, &invite.name, &icons, &r) {
                    Some(true) => self.join_invite_from(&invite.id),
                    Some(false) => self.decline_invite(&invite.id),
                    None => {}
                }
            }
        }

        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.settings_open = false;
        }
        let settings = overlays::open_amount(ctx, "settings", self.settings_open);
        if settings > 0.0 {
            self.render_settings(ctx, window, settings, &icons, &r);
        }

        // The settings window covers the main area and shows its own errors.
        let error = self.engine.error.as_ref().filter(|_| !self.settings_open);
        if let Some(error) = error {
            if screens::error_toast(ctx, content, &error.to_string(), &icons, &r) {
                self.engine.error = None;
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

    /// Home's rows as shown: a page or more of each.
    fn home_rows(&self) -> Vec<HomeRowView<'_>> {
        let paging = &self.engine.library.paging;
        self.home_rows_full()
            .into_iter()
            .map(|(title, tracks, playlist)| HomeRowView {
                title,
                tracks: &tracks[..paging.shown(title, tracks.len())],
                playlist,
                loading: paging.loading_row(title),
            })
            .collect()
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

        crate::ui::fonts::apply_loaded(ctx);
        self.handle_close_to_tray(ctx);
        self.engine.menu_visible = self.menu_open;
        self.engine.tick();
        self.poll_cover_pick();
        self.poll_firewall();
        self.poll_updates(ctx);
        self.follow_back_and_forward(ctx);
        for uri in self.engine.stale_images.drain(..) {
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
        .with_icon(window_icon())
        .with_inner_size(size)
        .with_resizable(true)
        .with_maximize_button(true)
        .with_min_inner_size(crate::ui::layout::MIN_WINDOW)
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
            crate::infrastructure::system::install_platform();
            let ctx = cc.egui_ctx.clone();
            let mut app = YtMusicApp::new(Arc::new(move || ctx.request_repaint()));
            app.start();
            crate::infrastructure::bundled::download_yt_dlp_in_background();
            crate::infrastructure::bundled::update_yt_dlp_in_background(
                app.tools.yt_dlp.clone(),
                app.registry.clone(),
            );
            Ok(Box::new(app))
        }),
    )
}

impl YtMusicApp {
    /// The mouse's side buttons (back and forward) and Alt+← / Alt+→ move
    /// through the pages seen.
    fn follow_back_and_forward(&mut self, ctx: &egui::Context) {
        let (back, forward) = ctx.input(|input| {
            let alt = input.modifiers.alt;
            (
                input.pointer.button_pressed(egui::PointerButton::Extra1)
                    || (alt && input.key_pressed(egui::Key::ArrowLeft)),
                input.pointer.button_pressed(egui::PointerButton::Extra2)
                    || (alt && input.key_pressed(egui::Key::ArrowRight)),
            )
        });
        if back {
            self.engine.go_back();
        } else if forward {
            self.engine.go_forward();
        }
    }

    /// With close to tray on, closing (✕, Alt+F4) hides MusiKPL to the tray
    /// instead; the tray brings it back or quits.
    fn handle_close_to_tray(&mut self, ctx: &egui::Context) {
        use crate::infrastructure::tray::{self, Tray, TrayEvent};
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && self.engine.settings.close_to_tray && cfg!(windows) && !self.quitting
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.tray.is_none() {
                self.tray = Tray::show(ctx);
            }
            tray::hide_main_window();
        }
        match self.tray.as_ref().and_then(Tray::poll) {
            Some(TrayEvent::Show) => {
                self.tray = None;
                tray::show_main_window();
            }
            Some(TrayEvent::Quit) => {
                self.tray = None;
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }
}
/// The logo as the window and taskbar icon (drawn from logo.svg at build
/// time by build_icon.rs).
pub(crate) fn window_icon() -> egui::IconData {
    const SIZE: u32 = 256;
    egui::IconData {
        rgba: include_bytes!(concat!(env!("OUT_DIR"), "/icon.rgba")).to_vec(),
        width: SIZE,
        height: SIZE,
    }
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
    crate::infrastructure::tray::remember_main_window(hwnd);
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
