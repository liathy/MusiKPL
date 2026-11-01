//! The app itself, without drawing: what plays, the queue and Auto Mix,
//! search, Home, playlists, likes, Last.fm, Jams and friends.
//!
//! A shell (the desktop's egui window, the Android app) owns one [`Engine`],
//! calls [`Engine::tick`] regularly, draws from its public state, and turns
//! the user's actions into calls to its methods.

mod automix;
mod crossfade;
mod devices;
mod downloads;
mod errors;
mod friends;
mod home_paging;
mod jam_sync;
mod lastfm_browse;
mod lastfm_sync;
mod library;
mod lyrics;
mod navigation;
mod playlist_recs;
mod playlists;
mod settings;
mod shuffle;

pub use automix::AutoMix;
pub use devices::{DevicesState, LinkPrompt, LinkedDevice};
pub use downloads::{DownloadMark, DownloadsState};
pub use errors::AppError;
pub use friends::{Friend, FriendsState, ReceivedInvite};
pub use home_paging::HomePaging;
pub use jam_sync::{track_from, GuestSync, NOTICE_DURATION};
pub use lastfm_browse::{Bookmark, Browse, Photos, SongIntent};
pub use library::{unix_now, Library};
pub use lyrics::{LyricsState, LyricsStatus};
pub use navigation::{Navigation, Page};
pub use playlist_recs::PlaylistRecs;
pub use playlists::playlist_cover;
pub use settings::LastFmConnect;
pub use shuffle::PlayOrder;

use std::{
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};

use crate::{
    audio::AudioOutput,
    domain::{CoverArt, Track},
    extractor::Extractor,
    jam::{JamController, Role},
    lastfm::{LastFm, ScrobbleTracker},
    platform::{quiet_waker, Waker},
    playback::{Player, Queue},
    recommendations::History,
    settings::Settings,
};

/// A resolved stream: the track, its URL, and whether the track it
/// replaces belongs in the history.
pub type PlayResult = Result<(Track, String, bool), String>;

/// Songs that may fail to load in a row before the player gives up instead
/// of skipping to the next one.
pub const MAX_FAILED_SKIPS: u32 = 3;

/// A looked-up stream link is trusted this long (YouTube's last hours).
const PRELOAD_FRESH: Duration = Duration::from_secs(30 * 60);

/// How often to wake up with nothing to animate, by situation.
const IDLE_TICK: Duration = Duration::from_secs(1);
const PLAYING_TICK: Duration = Duration::from_millis(250);
const JAM_HOST_TICK: Duration = Duration::from_millis(100);
/// A guest corrects drift on frame boundaries, so it needs frequent ticks.
const JAM_GUEST_TICK: Duration = Duration::from_millis(20);

/// The next song's stream, looked up ahead of time.
pub struct Preload {
    pub id: String,
    pub rx: Option<Receiver<Result<String, String>>>,
    pub url: Option<String>,
    pub at: Instant,
}

/// What the main area shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Home,
    Search,
    /// The playlist in open_playlist.
    Playlist,
    /// A Last.fm artist's page.
    Artist,
    /// The connected Last.fm user's profile and stats.
    Profile,
}

pub struct Engine {
    /// Finds songs and their audio (yt-dlp, NewPipe).
    pub extractor: Arc<dyn Extractor>,
    /// Wakes the shell when background work has news.
    pub waker: Waker,
    pub player: Player,
    /// What the search box holds.
    pub search: String,
    /// The query the current results belong to.
    pub results_query: String,
    pub results: Vec<Track>,
    pub queue: Queue,
    pub history: History,
    pub current: Option<Track>,
    pub error: Option<AppError>,
    pub auto_mix: bool,
    pub automix: AutoMix,
    pub searching: bool,
    pub loading: bool,
    pub finished_handled: bool,
    /// Songs in a row that failed to load.
    pub failed_plays: u32,
    pub search_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    pub play_rx: Option<Receiver<PlayResult>>,
    pub jam: JamController,
    pub jam_sync: GuestSync,
    pub jam_notice: Option<(String, Instant)>,
    /// The track currently being resolved, so a Jam host can announce it early.
    pub pending_track: Option<Track>,
    /// Host: when a new track started waiting, paused, for guests to load it.
    pub jam_hold: Option<Instant>,
    /// The invite being pasted to join a Jam over the internet.
    pub jam_invite_input: String,
    pub view: View,
    /// The shell shows the menu (friends, bookmarked artists): friends are
    /// checked more often and bookmark photos are looked up.
    pub menu_visible: bool,
    /// The user's taste, likes, session, and Home.
    pub library: Library,
    /// The last session's song is shown paused at this position but not
    /// loaded yet; pressing play loads it from here.
    pub resume_at: Option<f64>,
    /// Where the song being loaded starts.
    pub start_at: f64,
    /// The first-run (or Settings) genre picker is showing, with its picks.
    pub genre_picks: Option<Vec<String>>,
    pub open_playlist: Option<u64>,
    /// The open playlist is in edit mode, with its name being typed here.
    pub playlist_editing: bool,
    pub playlist_name_draft: String,
    /// The volume to go back to when unmuting.
    pub unmute_volume: u8,
    /// Songs suggested for the open playlist.
    pub playlist_recs: PlaylistRecs,
    /// The next song's stream, looked up before the current one ends.
    pub preload: Option<Preload>,
    /// Last.fm artists, artist pages, bookmarks, and the user's profile.
    pub browse: Browse,
    /// Friends: who's online in MusiKPL, and Jam invites.
    pub friends: FriendsState,
    /// Likes kept in step with Last.fm loves.
    pub loves: lastfm_sync::LovesSync,
    /// The playing song's lyrics, while shown.
    pub lyrics: LyricsState,
    /// The user's other devices, linked to sync playlists.
    pub devices: DevicesState,
    /// Text for the shell to put on the clipboard.
    pub clipboard: Option<String>,
    pub settings: Settings,
    /// Problem to show on the settings screen.
    pub settings_error: Option<String>,
    pub lastfm: LastFm,
    pub lastfm_connect: LastFmConnect,
    pub scrobbler: ScrobbleTracker,
    pub cover_art: CoverArt,
    /// Cover images no longer on screen, for the shell to drop from its
    /// image cache.
    pub stale_images: Vec<String>,
    /// Back and forward between pages.
    pub nav: Navigation,
    /// Shuffle, Smart Shuffle and Repeat.
    pub order: PlayOrder,
    /// The next song starts with this crossfade (the one playing keeps
    /// going under it).
    pub pending_fade: Option<crate::audio::Fade>,
    /// Songs and playlists saved for offline listening.
    pub downloads: DownloadsState,
}

/// An extractor for an engine that hasn't got one (tests, and before the
/// platform's is ready): every request fails, nothing reaches the network.
struct NoExtractor;

impl Extractor for NoExtractor {
    fn list(&self, _source: &str, _limit: Option<usize>) -> Result<Vec<Track>, String> {
        Err("Searching isn't available".into())
    }
    fn search(&self, _query: &str) -> Result<Vec<Track>, String> {
        Err("Searching isn't available".into())
    }
    fn stream_url(&self, _track: &Track) -> Result<String, String> {
        Err("Playing isn't available".into())
    }
}

impl Default for Engine {
    /// Silent and offline: plays nothing and finds nothing.
    fn default() -> Self {
        Self::new(
            Box::new(crate::audio::SilentOutput::default()),
            Arc::new(NoExtractor),
            quiet_waker(),
        )
    }
}

impl Engine {
    pub fn new(output: Box<dyn AudioOutput>, extractor: Arc<dyn Extractor>, waker: Waker) -> Self {
        Self {
            extractor,
            waker,
            player: Player::new(output),
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
            jam_invite_input: String::new(),
            view: View::Home,
            menu_visible: false,
            library: Library::default(),
            resume_at: None,
            start_at: 0.0,
            genre_picks: None,
            open_playlist: None,
            playlist_editing: false,
            playlist_name_draft: String::new(),
            unmute_volume: 100,
            playlist_recs: PlaylistRecs::default(),
            preload: None,
            browse: Browse::default(),
            friends: FriendsState::default(),
            loves: lastfm_sync::LovesSync::default(),
            lyrics: LyricsState::default(),
            devices: DevicesState::default(),
            clipboard: None,
            settings: Settings::default(),
            settings_error: None,
            lastfm: LastFm::default(),
            lastfm_connect: LastFmConnect::Idle,
            scrobbler: ScrobbleTracker::default(),
            cover_art: CoverArt,
            stale_images: Vec::new(),
            nav: Navigation::default(),
            order: PlayOrder::default(),
            pending_fade: None,
            downloads: DownloadsState::default(),
        }
    }

    /// Load everything saved (settings, taste, playlists, the session,
    /// bookmarks, friends) and start what runs in the background.
    pub fn start(&mut self) {
        self.load_settings();
        self.load_library();
        self.load_browse();
        self.load_friends();
        self.load_devices();
        self.load_downloads();
        self.prefetch_profile();
        if !self.settings.onboarded && !self.library.taste.has_history() {
            self.genre_picks = Some(Vec::new());
        }
        self.jam.start_discovery();
    }

    /// Do what's due: collect background results, keep playback, Auto Mix,
    /// Jams, scrobbling and saving going. Call it every frame, or at least
    /// as often as [`Engine::idle_tick`] says.
    pub fn tick(&mut self) {
        self.player.refresh_position();
        self.poll_background_work();
        self.poll_jam();
        self.poll_lastfm();
        self.update_scrobbling();
        self.tick_library();
        self.poll_home_paging();
        self.keep_playlist_recs();
        self.tick_browse();
        self.tick_friends();
        self.tick_devices();
        self.tick_lastfm_sync();
        self.tick_lyrics();
        self.tick_downloads();
        self.track_page();
        self.preload_next();
        self.tick_crossfade();
        self.handle_finished_track();
        self.poll_mix();
        self.keep_mix_filled();
        self.keep_smart_shuffle();
    }

    /// Search for what the search box holds.
    pub fn search(&mut self) {
        let query = self.search.trim().to_owned();
        if query.is_empty() || self.searching {
            return;
        }
        self.searching = true;
        self.error = None;
        self.view = View::Search;
        self.search_artists(&query);
        self.settings.remember_search(&query);
        self.library.settings_changed = true;
        self.results_query = query.clone();
        let extractor = self.extractor.clone();
        let (sender, receiver) = mpsc::channel();
        self.search_rx = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(extractor.search(&query));
        });
    }

    /// Play a search result: a fresh start, so Auto Mix builds a new mix
    /// around it (songs the user queued stay).
    pub fn play_search_track(&mut self, track: Track) {
        if self.is_guest() {
            return;
        }
        self.reset_mix();
        self.failed_plays = 0;
        self.start_song_context(&track);
        self.start_track(track, true);
    }

    pub fn play_queue_track(&mut self, track: Track) {
        if self.is_guest() {
            return;
        }
        self.start_track(track, true);
    }

    pub fn start_track(&mut self, track: Track, remember_current: bool) {
        self.start_track_at(track, remember_current, 0.0);
    }

    pub fn start_track_at(&mut self, track: Track, remember_current: bool, start_at: f64) {
        // Crossfading: the song playing goes on under the next one.
        if self.pending_fade.is_none() {
            self.player.stop();
        }
        self.resume_at = None;
        self.start_at = start_at;
        self.loading = true;
        self.automix.waiting = false;
        self.pending_track = Some(track.clone());
        self.jam_hold = None;
        self.error = None;
        self.finished_handled = false;
        let (sender, receiver) = mpsc::channel();
        self.play_rx = Some(receiver);
        // Downloaded: play the file.
        if let Some(path) = self.local_file(&track.id) {
            let _ = sender.send(Ok((track, path, remember_current)));
            return;
        }
        if self.offline() {
            let _ = sender.send(Err("You're offline, and this song isn't downloaded".into()));
            return;
        }
        // Looked up ahead of time: start without waiting for the extractor.
        if let Some(url) = self.take_preloaded(&track.id) {
            let _ = sender.send(Ok((track, url, remember_current)));
            return;
        }
        let extractor = self.extractor.clone();
        std::thread::spawn(move || {
            let result = extractor
                .stream_url(&track)
                .map(|url| (track, url, remember_current));
            let _ = sender.send(result);
        });
    }

    /// The preloaded stream for id, if it is ready and still fresh.
    fn take_preloaded(&mut self, id: &str) -> Option<String> {
        let preload = self.preload.take()?;
        (preload.id == id && preload.at.elapsed() < PRELOAD_FRESH)
            .then_some(preload.url)
            .flatten()
    }

    /// Called every tick: while a song plays, keep the next queued song's
    /// stream looked up in the background, so skipping or reaching the end
    /// starts it right away. A new next song (the queue changed) is looked
    /// up again; one lookup runs at a time.
    fn preload_next(&mut self) {
        if let Some(preload) = &mut self.preload {
            if let Some(rx) = &preload.rx {
                match rx.try_recv() {
                    Ok(result) => {
                        preload.url = result.ok();
                        preload.rx = None;
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => preload.rx = None,
                }
            }
        }
        // Nothing to prepare for while a song loads, before anything plays,
        // or for a Jam guest (the host's songs are loaded on its clock).
        if self.is_guest() || self.loading || self.current.is_none() || self.resume_at.is_some() {
            return;
        }
        if self
            .preload
            .as_ref()
            .is_some_and(|preload| preload.rx.is_some())
        {
            return;
        }
        let Some(next) = self.queue.iter().next().cloned() else {
            return;
        };
        // Downloaded songs need no lookup; offline, none is possible.
        if self.offline() || self.local_file(&next.id).is_some() {
            return;
        }
        let current = self
            .preload
            .as_ref()
            .is_some_and(|preload| preload.id == next.id && preload.at.elapsed() < PRELOAD_FRESH);
        if current {
            return;
        }
        let extractor = self.extractor.clone();
        let (sender, receiver) = mpsc::channel();
        let id = next.id.clone();
        std::thread::spawn(move || {
            let _ = sender.send(extractor.stream_url(&next));
        });
        self.preload = Some(Preload {
            id,
            rx: Some(receiver),
            url: None,
            at: Instant::now(),
        });
    }

    /// Play the next queued song. With nothing queued, Auto Mix keeps the
    /// session going: the player waits for its next pick instead of ending.
    pub fn next(&mut self) {
        if self.is_guest() {
            return;
        }
        // The user skipped while a song plays: fade quickly when allowed.
        if self.pending_fade.is_none() && !self.finished_handled {
            self.pending_fade = self.skip_fade();
        }
        if let Some(track) = self.queue.pop_next() {
            self.play_queue_track(track);
            return;
        }
        // Repeat all: what's being listened to starts over.
        if self.repeat_context() {
            if let Some(track) = self.queue.pop_next() {
                self.play_queue_track(track);
                return;
            }
        }
        // Offline: the downloads, shuffled.
        if self.fill_queue_offline() {
            if let Some(track) = self.queue.pop_next() {
                self.play_queue_track(track);
                return;
            }
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
    pub fn skip_failed_track(&mut self, error: String) {
        self.error = Some(AppError::Playback(error));
        self.failed_plays += 1;
        if self.failed_plays < MAX_FAILED_SKIPS {
            self.next();
        }
    }

    pub fn previous(&mut self) {
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
                Err(e) => {
                    // Old results would read as this search's; clear them so
                    // Last.fm's songs can show instead.
                    let old = std::mem::take(&mut self.results);
                    self.retire_covers(old);
                    if self.browse.tracks.is_empty() {
                        self.error = Some(AppError::Search(e));
                    }
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
                    Ok((track, url, _)) => self.finish_guest_load(track, &url),
                    Err(error) => self.fail_guest_load(None, error),
                }
                return;
            }
            match result {
                Ok((track, url, remember_current)) => {
                    self.note_played(&track);
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
                    let started = match self.pending_fade.take() {
                        Some(fade) if !hold => self.player.crossfade_stream(&url, start_at, fade),
                        _ => self.player.play_stream_at(&track, &url, start_at, hold),
                    };
                    match started {
                        Ok(()) => {
                            self.jam_hold = hold.then(Instant::now);
                            self.error = None;
                            self.finished_handled = false;
                            self.failed_plays = 0;
                        }
                        Err(e) => self.skip_failed_track(e),
                    }
                }
                Err(e) => {
                    if self.pending_fade.take().is_some() {
                        self.player.stop();
                    }
                    self.skip_failed_track(e);
                }
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
        // Repeat one: the same song again.
        if self.settings.repeat == crate::settings::RepeatMode::One {
            if let Some(track) = self.current.clone() {
                self.start_track(track, false);
                return;
            }
        }
        self.next();
    }

    /// Mark covers of replaced search results for removal from the image
    /// cache, unless the song is still playing or queued. Every cover is a
    /// decoded image plus a GPU texture, so a long session would otherwise
    /// keep every search it ever showed.
    pub fn retire_covers(&mut self, old: Vec<Track>) {
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

    /// Whether background work is under way that the shell should check on
    /// soon.
    pub fn busy(&self) -> bool {
        self.searching
            || self.loading
            || self.automix.busy()
            || self.automix.waiting
            || self.library.paging.busy()
            || self.playlist_recs.loading()
            || self.browse.busy()
            || self.library.building_home()
            || self.order.busy()
            || self.downloads.pending() > 0
    }

    /// How long until the next tick is needed when nothing animates.
    pub fn idle_tick(&self) -> Duration {
        match self.jam.info.role {
            Some(Role::Guest) => JAM_GUEST_TICK,
            Some(Role::Host) => JAM_HOST_TICK,
            // The clock and seek bar move, and the end of a song must be
            // noticed promptly to start the next one.
            // Lyrics light up line by line.
            None if self.lyrics.wanted && self.current.is_some() && !self.player.paused => {
                JAM_HOST_TICK
            }
            None if self.current.is_some() && !self.player.paused => PLAYING_TICK,
            // Background work reports through channels polled each tick.
            None if self.busy() => PLAYING_TICK,
            None => IDLE_TICK,
        }
    }

    /// The song shown in the player bar: the one loading, or the one playing.
    pub fn shown_track(&self) -> Option<&Track> {
        if self.loading {
            self.pending_track.as_ref().or(self.current.as_ref())
        } else {
            self.current.as_ref()
        }
    }

    /// Play or pause. The last session's song is loaded on the first play,
    /// from where it was left.
    pub fn toggle_pause(&mut self) {
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

    pub fn seek(&mut self, position: f64) {
        if self.resume_at.is_some() {
            self.resume_at = Some(position);
            self.player.show_paused_at(position);
        } else {
            self.player.seek(position);
        }
    }

    pub fn set_volume(&mut self, volume: u8) {
        self.player.set_volume(volume);
        self.settings.volume = volume;
        self.library.settings_changed = true;
    }

    /// Mute, or go back to the volume from before muting.
    pub fn toggle_mute(&mut self) {
        if self.player.volume > 0 {
            self.unmute_volume = self.player.volume;
            self.set_volume(0);
        } else {
            let volume = if self.unmute_volume > 0 {
                self.unmute_volume
            } else {
                50
            };
            self.set_volume(volume);
        }
    }

    /// Pick or unpick a genre in the genre picker.
    pub fn toggle_genre(&mut self, genre: String) {
        if let Some(picks) = &mut self.genre_picks {
            if let Some(index) = picks.iter().position(|pick| *pick == genre) {
                picks.remove(index);
            } else {
                picks.push(genre);
            }
        }
    }

    /// The genre picker's Done: keep the picks and build Home from them.
    pub fn finish_genres(&mut self) {
        let picks = self.genre_picks.take().unwrap_or_default();
        let changed = picks != self.settings.favorite_genres;
        self.settings.favorite_genres = picks;
        self.finish_onboarding(changed);
    }

    pub fn skip_genres(&mut self) {
        self.genre_picks = None;
        self.finish_onboarding(false);
    }

    fn finish_onboarding(&mut self, genres_changed: bool) {
        self.settings.onboarded = true;
        self.library.settings_changed = true;
        self.refresh_home(genres_changed);
    }

    /// Open the genre picker on Home, starting from the saved picks.
    pub fn pick_genres(&mut self) {
        self.genre_picks = Some(self.settings.favorite_genres.clone());
        self.view = View::Home;
    }

    /// Go to Home and look for new recommendations if it's old.
    pub fn go_home(&mut self) {
        self.view = View::Home;
        self.refresh_home(false);
    }

    /// Home's rows, each with all its songs: favorite playlists first, then
    /// the first two built rows, Liked songs, the other built rows, and
    /// Recently played.
    pub fn home_rows_full(&self) -> Vec<(&str, &[Track], Option<u64>)> {
        // Offline: what can play, the downloads and downloaded playlists.
        if self.offline() {
            let mut rows: Vec<(&str, &[Track], Option<u64>)> = Vec::new();
            if !self.downloads.songs.is_empty() {
                rows.push(("Downloaded", &self.downloads.songs, None));
            }
            rows.extend(
                self.library
                    .playlists
                    .iter()
                    .filter(|p| {
                        self.downloads.index.playlists.contains(&p.uid) && !p.tracks.is_empty()
                    })
                    .map(|p| (p.name.as_str(), p.tracks.as_slice(), Some(p.id))),
            );
            return rows;
        }
        let built = &self.library.home.rows;
        let split = built.len().min(2);
        let mut rows: Vec<(&str, &[Track], Option<u64>)> = self
            .library
            .playlists
            .iter()
            .filter(|playlist| playlist.favorite && !playlist.tracks.is_empty())
            .map(|playlist| {
                (
                    playlist.name.as_str(),
                    playlist.tracks.as_slice(),
                    Some(playlist.id),
                )
            })
            .collect();
        rows.extend(
            built[..split]
                .iter()
                .map(|row| (row.title.as_str(), row.tracks.as_slice(), None)),
        );
        if !self.library.liked.is_empty() {
            rows.push(("Liked songs", &self.library.liked, None));
        }
        if !self.downloads.songs.is_empty() {
            rows.push(("Downloaded", &self.downloads.songs, None));
        }
        rows.extend(
            built[split..]
                .iter()
                .map(|row| (row.title.as_str(), row.tracks.as_slice(), None)),
        );
        if !self.library.recent.is_empty() {
            rows.push(("Recently played", &self.library.recent, None));
        }
        rows
    }

    /// How many songs a Home row has in all.
    pub fn home_row_len(&self, title: &str) -> usize {
        self.home_rows_full()
            .into_iter()
            .find(|(row, _, _)| *row == title)
            .map_or(0, |(_, tracks, _)| tracks.len())
    }

    /// The user reached the end of a Home row: show (and fetch) more.
    pub fn more_in_home_row(&mut self, title: &str) {
        let available = self.home_row_len(title);
        self.more_in_row(title, available);
    }

    /// Save everything now (the app is closing).
    pub fn shut_down(&mut self) {
        self.save_library(true);
        self.player.stop();
    }
}

#[cfg(test)]
mod tests;
