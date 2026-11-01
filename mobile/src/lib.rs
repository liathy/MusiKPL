//! MusiKPL's engine for the Android app.
//!
//! The Kotlin app implements three small interfaces: [`AndroidPlayer`]
//! (ExoPlayer), [`AndroidExtractor`] (NewPipe, with yt-dlp as a fallback)
//! and [`AndroidPlatform`] (links, waking the app). In return it gets a
//! [`MobileEngine`]: it calls `tick` regularly, reads plain snapshots to
//! draw (player, Home, search, queue, playlists, artist, profile,
//! settings), and calls methods for what the user does.

uniffi::setup_scaffolding!();

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};

use musikpl_core::{
    audio::{AudioOutput, Fade, FadeCurve, PositionSample},
    domain::{song_info, PlaylistTarget, Track, Transition},
    engine::{
        AppError, DownloadMark, Engine, LastFmConnect, LyricsStatus, SongIntent, View,
        NOTICE_DURATION,
    },
    extractor::Extractor,
    jam::{DiscoveryStatus, Progress, Role},
    lastfm::{browse::Period, built_in_keys},
    platform::{self, Platform},
    recommendations::home::STARTER_GENRES,
    settings::{RepeatMode, ShuffleMode},
    storage,
};

// ---------------------------------------------------------------------------
// What Kotlin implements
// ---------------------------------------------------------------------------

/// Why something failed, in plain words.
#[derive(Debug, uniffi::Error)]
pub enum MobileError {
    Failed { reason: String },
}

impl std::fmt::Display for MobileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Failed { reason } => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for MobileError {}

impl From<uniffi::UnexpectedUniFFICallbackError> for MobileError {
    fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Failed {
            reason: error.reason,
        }
    }
}

fn message(error: MobileError) -> String {
    error.to_string()
}

/// Plays one stream at a time (ExoPlayer). Called from the engine's thread;
/// the Kotlin side hands the calls to the player's own thread.
#[uniffi::export(with_foreign)]
pub trait AndroidPlayer: Send + Sync {
    /// Start `url` at `start_secs`, paused or not. An error says why it
    /// couldn't start.
    fn play(&self, url: String, volume: u8, start_secs: f64, paused: bool) -> Option<String>;
    fn set_paused(&self, paused: bool);
    fn seek(&self, position: f64);
    fn set_speed(&self, speed: f64);
    fn set_volume(&self, volume: u8);
    fn stop(&self);
    /// Where the stream is now, in seconds; None until it's playing.
    fn position(&self) -> Option<f64>;
    /// The stream ended, or failed while playing.
    fn is_finished(&self) -> bool;
    /// Start `url` while the song playing fades out over `seconds`.
    fn crossfade(
        &self,
        url: String,
        volume: u8,
        start_secs: f64,
        seconds: f64,
        curve: FadeShape,
    ) -> Option<String>;
}

/// How songs fade into each other (see `FadeCurve`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FadeShape {
    Linear,
    Smooth,
    EqualPower,
    Exponential,
}

impl From<FadeCurve> for FadeShape {
    fn from(curve: FadeCurve) -> Self {
        match curve {
            FadeCurve::Linear => Self::Linear,
            FadeCurve::Smooth => Self::Smooth,
            FadeCurve::EqualPower => Self::EqualPower,
            FadeCurve::Exponential => Self::Exponential,
        }
    }
}

impl From<FadeShape> for FadeCurve {
    fn from(shape: FadeShape) -> Self {
        match shape {
            FadeShape::Linear => Self::Linear,
            FadeShape::Smooth => Self::Smooth,
            FadeShape::EqualPower => Self::EqualPower,
            FadeShape::Exponential => Self::Exponential,
        }
    }
}

/// Finds songs and their audio. See `musikpl_core::extractor` for the
/// sources `list` receives (`ytsearchN:query`, Mix and video links).
#[uniffi::export(with_foreign)]
pub trait AndroidExtractor: Send + Sync {
    fn list(&self, source: String, limit: u32) -> Result<Vec<Song>, MobileError>;
    fn search(&self, query: String) -> Result<Vec<Song>, MobileError>;
    fn stream_url(&self, song: Song) -> Result<String, MobileError>;
}

/// What the app does for the engine.
#[uniffi::export(with_foreign)]
pub trait AndroidPlatform: Send + Sync {
    /// Open a web page; false when it couldn't.
    fn open_url(&self, url: String) -> bool;
    /// Background work has news: tick the engine and redraw soon.
    fn wake(&self);
}

struct ExoOutput(Arc<dyn AndroidPlayer>);

impl AudioOutput for ExoOutput {
    fn crossfade(
        &mut self,
        url: &str,
        volume: u8,
        start_secs: f64,
        fade: Fade,
    ) -> Result<(), String> {
        match self.0.crossfade(
            url.to_owned(),
            volume,
            start_secs,
            fade.seconds,
            fade.curve.into(),
        ) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn play(&mut self, url: &str, volume: u8, start_secs: f64, paused: bool) -> Result<(), String> {
        match self.0.play(url.to_owned(), volume, start_secs, paused) {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
    fn set_paused(&mut self, paused: bool) {
        self.0.set_paused(paused);
    }
    fn seek(&mut self, position: f64) {
        self.0.seek(position);
    }
    fn set_speed(&mut self, speed: f64) {
        self.0.set_speed(speed);
    }
    fn set_volume(&mut self, volume: u8) {
        self.0.set_volume(volume);
    }
    fn stop(&mut self) {
        self.0.stop();
    }
    fn latest_position(&self) -> Option<PositionSample> {
        self.0.position().map(|position| PositionSample {
            position,
            taken: Instant::now(),
        })
    }
    fn reports_position(&self) -> bool {
        true
    }
    fn is_finished(&mut self) -> bool {
        self.0.is_finished()
    }
}

struct KotlinExtractor(Arc<dyn AndroidExtractor>);

impl Extractor for KotlinExtractor {
    fn list(&self, source: &str, limit: Option<usize>) -> Result<Vec<Track>, String> {
        let limit = limit.map_or(30, |limit| limit as u32);
        self.0
            .list(source.to_owned(), limit)
            .map(|songs| songs.into_iter().map(Song::into_track).collect())
            .map_err(message)
    }
    fn search(&self, query: &str) -> Result<Vec<Track>, String> {
        self.0
            .search(query.to_owned())
            .map(|songs| songs.into_iter().map(Song::into_track).collect())
            .map_err(message)
    }
    fn stream_url(&self, track: &Track) -> Result<String, String> {
        self.0.stream_url(Song::of(track)).map_err(message)
    }
}

struct KotlinPlatform(Arc<dyn AndroidPlatform>);

impl Platform for KotlinPlatform {
    fn open_url(&self, url: &str) -> Result<(), String> {
        if self.0.open_url(url.to_owned()) {
            Ok(())
        } else {
            Err("Couldn't open the link".into())
        }
    }
}

// ---------------------------------------------------------------------------
// What Kotlin draws
// ---------------------------------------------------------------------------

/// A song as the screens show it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct Song {
    /// The YouTube video id.
    pub id: String,
    pub title: String,
    /// The artist, as worked out from the title and channel.
    pub artist: String,
    /// The YouTube channel.
    pub channel: String,
    /// Seconds, when known.
    pub duration: Option<f64>,
    /// The YouTube cover (center-crop it to a square).
    pub cover: String,
}

impl Song {
    fn of(track: &Track) -> Self {
        Self {
            id: track.id.clone(),
            title: track.title.clone(),
            artist: song_info(track).artist,
            channel: track.channel.clone(),
            duration: track.duration,
            cover: format!("https://i.ytimg.com/vi/{}/mqdefault.jpg", track.id),
        }
    }

    fn into_track(self) -> Track {
        Track {
            id: self.id,
            title: self.title,
            channel: self.channel,
            duration: self.duration,
        }
    }

    fn track(&self) -> Track {
        self.clone().into_track()
    }
}

fn songs<'a>(tracks: impl IntoIterator<Item = &'a Track>) -> Vec<Song> {
    tracks.into_iter().map(Song::of).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Screen {
    Home,
    Search,
    Playlist,
    Artist,
    Profile,
}

/// The player bar and lock screen.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PlayerState {
    /// The song playing or loading.
    pub song: Option<Song>,
    /// Loading it, or waiting for Auto Mix's next song.
    pub loading: bool,
    pub paused: bool,
    pub position: f64,
    pub duration: f64,
    pub liked: bool,
    pub volume: u8,
    /// A Jam guest: playback follows the host.
    pub guest: bool,
    pub screen: Screen,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// Something went wrong (shown until dismissed).
    pub error: Option<String>,
    pub shuffle: Shuffle,
    pub repeat: Repeat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Shuffle {
    Off,
    On,
    /// Shuffled, with recommended songs mixed in.
    Smart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Repeat {
    Off,
    All,
    One,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct HomeRow {
    pub title: String,
    pub songs: Vec<Song>,
    /// A favorite playlist's row opens that playlist.
    pub playlist_id: Option<i64>,
    /// More songs for this row are on their way.
    pub loading: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct HomeState {
    pub rows: Vec<HomeRow>,
    /// New recommendations are being found.
    pub building: bool,
    /// More rows are on their way (the user reached the bottom).
    pub loading_rows: bool,
    /// The genre picker is showing, with the picks so far.
    pub picking_genres: Option<Vec<String>>,
    /// Every genre the picker offers.
    pub genres: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ArtistHit {
    pub name: String,
    pub listeners: u64,
    /// Their photo (found on YouTube), once looked up.
    pub photo: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LastFmSong {
    pub artist: String,
    pub title: String,
    pub listeners: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PersonHit {
    pub name: String,
    pub scrobbles: u64,
    pub avatar: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SearchState {
    pub query: String,
    pub searching: bool,
    pub songs: Vec<Song>,
    pub artists: Vec<ArtistHit>,
    pub people: Vec<PersonHit>,
    /// Songs Last.fm found, shown when YouTube finds none.
    pub lastfm_songs: Vec<LastFmSong>,
    pub recent: Vec<String>,
    /// A Last.fm song is being found on YouTube.
    pub finding_song: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct QueueState {
    pub songs: Vec<Song>,
    /// Songs Smart Shuffle added (marked in the queue).
    pub smart: Vec<String>,
    /// Auto Mix is looking for more.
    pub mixing: bool,
    pub auto_mix: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PlaylistSummary {
    pub id: i64,
    pub name: String,
    pub songs: u32,
    pub favorite: bool,
    /// The user's photo (a file:// link), if chosen.
    pub cover: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PlaylistDetail {
    pub id: i64,
    pub name: String,
    pub favorite: bool,
    pub cover: Option<String>,
    pub songs: Vec<Song>,
    /// Songs to add, found from the ones in it.
    pub suggestions: Vec<Song>,
    pub suggestions_loading: bool,
    pub transition: TransitionInfo,
    /// Kept downloaded for offline.
    pub downloaded: bool,
    /// Its songs saved so far.
    pub saved: u32,
}

/// Downloads: what's saved, what's coming, and whether we're offline.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DownloadsInfo {
    pub songs: Vec<Song>,
    /// Ids of songs downloaded.
    pub saved_ids: Vec<String>,
    /// Ids waiting or downloading.
    pub pending_ids: Vec<String>,
    /// The song downloading now, and how far (percent).
    pub current: Option<String>,
    pub percent: u32,
    pub offline: bool,
    pub notice: Option<String>,
    /// Megabytes used.
    pub megabytes: u64,
}

/// A playlist's crossfade: as in Settings, off, or its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransitionKind {
    Default,
    Off,
    Fade,
}

#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct TransitionInfo {
    pub kind: TransitionKind,
    pub seconds: u8,
    pub curve: FadeShape,
}

impl From<Transition> for TransitionInfo {
    fn from(transition: Transition) -> Self {
        match transition {
            Transition::Default => Self {
                kind: TransitionKind::Default,
                seconds: 6,
                curve: FadeShape::Smooth,
            },
            Transition::Off => Self {
                kind: TransitionKind::Off,
                seconds: 6,
                curve: FadeShape::Smooth,
            },
            Transition::Fade { seconds, curve } => Self {
                kind: TransitionKind::Fade,
                seconds,
                curve: curve.into(),
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct TopSong {
    pub title: String,
    pub plays: u64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ArtistState {
    pub name: String,
    pub loading: bool,
    pub error: Option<String>,
    pub photo: Option<String>,
    pub listeners: u64,
    pub plays: u64,
    pub bio: String,
    pub tags: Vec<String>,
    pub top_songs: Vec<TopSong>,
    pub similar: Vec<ArtistHit>,
    pub similar_photos: Vec<Option<String>>,
    pub url: String,
    pub bookmarked: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ProfileSong {
    pub artist: String,
    pub title: String,
    pub plays: u64,
    /// Unix seconds it was played; 0 for top songs.
    pub when: u64,
    pub now_playing: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ProfileState {
    pub user: String,
    pub is_me: bool,
    pub is_friend: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub avatar: Option<String>,
    pub scrobbles: u64,
    pub artists: u64,
    pub loved: u64,
    pub country: String,
    pub url: String,
    /// 0 week, 1 month, 2 year, 3 all time.
    pub period: u8,
    pub top_artists: Vec<ArtistHit>,
    pub top_songs: Vec<ProfileSong>,
    pub recent: Vec<ProfileSong>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Bookmark {
    pub name: String,
    pub photo: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct SettingsState {
    pub auto_mix: bool,
    pub invite_popups: bool,
    /// The connected Last.fm user, or empty.
    pub lastfm_user: String,
    pub lastfm_avatar: Option<String>,
    pub lastfm_scrobbles: Option<u64>,
    /// Connecting: the approval page to open (None while starting).
    pub lastfm_connecting: bool,
    pub lastfm_approval_url: Option<String>,
    /// MusiKPL can connect Last.fm in one step (its key is built in).
    pub lastfm_one_click: bool,
    pub error: Option<String>,
    pub bookmarks: Vec<Bookmark>,
    pub crossfade: bool,
    pub crossfade_seconds: u8,
    pub crossfade_curve: FadeShape,
    pub crossfade_on_skip: bool,
}

// ---------------------------------------------------------------------------
// The engine
// ---------------------------------------------------------------------------

#[derive(uniffi::Object)]
pub struct MobileEngine {
    engine: Mutex<Engine>,
}

fn period_index(period: Period) -> u8 {
    Period::ALL.iter().position(|p| *p == period).unwrap_or(1) as u8
}

impl MobileEngine {
    fn engine(&self) -> MutexGuard<'_, Engine> {
        self.engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[uniffi::export]
impl MobileEngine {
    /// The engine, keeping its data in `data_dir` (the app's own folder).
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        player: Arc<dyn AndroidPlayer>,
        extractor: Arc<dyn AndroidExtractor>,
        app: Arc<dyn AndroidPlatform>,
    ) -> Arc<Self> {
        storage::set_data_dir(PathBuf::from(data_dir));
        platform::install(Box::new(KotlinPlatform(app.clone())));
        let waker = Arc::new(move || app.wake());
        let engine = Engine::new(
            Box::new(ExoOutput(player)),
            Arc::new(KotlinExtractor(extractor)),
            waker,
        );
        Arc::new(Self {
            engine: Mutex::new(engine),
        })
    }

    /// Load what was saved and start the background work.
    pub fn start(&self) {
        self.engine().start();
    }

    /// Do what's due. Returns how many milliseconds until the next tick is
    /// needed if nothing happens meanwhile.
    pub fn tick(&self) -> u32 {
        let mut engine = self.engine();
        engine.tick();
        engine.idle_tick().as_millis().min(u32::MAX as u128) as u32
    }

    /// Save everything (the app is going away).
    pub fn shut_down(&self) {
        self.engine().shut_down();
    }

    // --- Snapshots -------------------------------------------------------

    pub fn player_state(&self) -> PlayerState {
        let engine = self.engine();
        let shown = engine.shown_track();
        let duration = engine
            .current
            .as_ref()
            .and_then(|track| track.duration)
            .unwrap_or(0.0);
        let loading = engine.loading || engine.automix.waiting;
        PlayerState {
            song: shown.map(Song::of),
            loading,
            paused: engine.player.paused,
            position: if duration > 0.0 && !engine.loading {
                engine.player.position_seconds().min(duration)
            } else {
                0.0
            },
            duration: if engine.loading { 0.0 } else { duration },
            liked: shown.is_some_and(|track| engine.is_liked(&track.id)),
            volume: engine.player.volume,
            guest: engine.is_guest(),
            screen: match engine.view {
                View::Home => Screen::Home,
                View::Search => Screen::Search,
                View::Playlist => Screen::Playlist,
                View::Artist => Screen::Artist,
                View::Profile => Screen::Profile,
            },
            can_go_back: engine.nav.can_back(),
            can_go_forward: engine.nav.can_forward(),
            error: engine.error.as_ref().map(ToString::to_string),
            shuffle: match engine.settings.shuffle {
                ShuffleMode::Off => Shuffle::Off,
                ShuffleMode::On => Shuffle::On,
                ShuffleMode::Smart => Shuffle::Smart,
            },
            repeat: match engine.settings.repeat {
                RepeatMode::Off => Repeat::Off,
                RepeatMode::All => Repeat::All,
                RepeatMode::One => Repeat::One,
            },
        }
    }

    pub fn home(&self) -> HomeState {
        let engine = self.engine();
        let paging = &engine.library.paging;
        let rows = engine
            .home_rows_full()
            .into_iter()
            .map(|(title, tracks, playlist)| HomeRow {
                title: title.to_owned(),
                songs: songs(&tracks[..paging.shown(title, tracks.len())]),
                playlist_id: playlist.map(|id| id as i64),
                loading: paging.loading_row(title),
            })
            .collect();
        HomeState {
            rows,
            building: engine.library.building_home(),
            loading_rows: paging.loading_rows(),
            picking_genres: engine.genre_picks.clone(),
            genres: STARTER_GENRES
                .iter()
                .map(|genre| genre.to_string())
                .collect(),
        }
    }

    pub fn search_state(&self) -> SearchState {
        let mut engine = self.engine();
        let hits: Vec<(String, u64)> = engine
            .browse
            .artists
            .iter()
            .take(8)
            .map(|artist| (artist.name.clone(), artist.listeners))
            .collect();
        let artists = hits
            .into_iter()
            .map(|(name, listeners)| ArtistHit {
                photo: engine.artist_image(&name).map(|cover| cover.url),
                name,
                listeners,
            })
            .collect();
        SearchState {
            query: engine.results_query.clone(),
            searching: engine.searching,
            songs: songs(&engine.results),
            artists,
            people: engine
                .browse
                .people
                .iter()
                .map(|person| PersonHit {
                    name: person.name.clone(),
                    scrobbles: person.scrobbles,
                    avatar: person.image.clone(),
                })
                .collect(),
            lastfm_songs: engine
                .browse
                .tracks
                .iter()
                .map(|song| LastFmSong {
                    artist: song.artist.clone(),
                    title: song.title.clone(),
                    listeners: song.listeners,
                })
                .collect(),
            recent: engine.settings.recent_searches.clone(),
            finding_song: engine.browse.finding_song(),
        }
    }

    pub fn queue(&self) -> QueueState {
        let engine = self.engine();
        let tracks: Vec<Track> = if engine.is_guest() {
            engine.jam_queue()
        } else {
            engine.queue.iter().cloned().collect()
        };
        QueueState {
            smart: tracks
                .iter()
                .filter(|track| engine.is_smart_pick(&track.id))
                .map(|track| track.id.clone())
                .collect(),
            songs: songs(&tracks),
            mixing: engine.automix.busy(),
            auto_mix: engine.auto_mix,
        }
    }

    pub fn playlists(&self) -> Vec<PlaylistSummary> {
        let engine = self.engine();
        engine
            .library
            .playlists
            .iter()
            .map(|playlist| PlaylistSummary {
                id: playlist.id as i64,
                name: playlist.name.clone(),
                songs: playlist.tracks.len() as u32,
                favorite: playlist.favorite,
                cover: musikpl_core::engine::playlist_cover(playlist).map(|cover| cover.url),
            })
            .collect()
    }

    /// The open playlist.
    pub fn playlist(&self) -> Option<PlaylistDetail> {
        let engine = self.engine();
        let id = engine.open_playlist?;
        let playlist = engine.library.playlists.get(id)?;
        Some(PlaylistDetail {
            id: id as i64,
            name: playlist.name.clone(),
            favorite: playlist.favorite,
            cover: musikpl_core::engine::playlist_cover(playlist).map(|cover| cover.url),
            songs: songs(&playlist.tracks),
            suggestions: engine.playlist_recs().into_iter().map(Song::of).collect(),
            suggestions_loading: engine.playlist_recs.loading(),
            transition: playlist.transition.into(),
            downloaded: engine.playlist_downloaded(id),
            saved: playlist
                .tracks
                .iter()
                .filter(|t| engine.downloads.is_downloaded(&t.id))
                .count() as u32,
        })
    }

    pub fn liked_songs(&self) -> Vec<Song> {
        songs(&self.engine().library.liked)
    }

    pub fn artist(&self) -> ArtistState {
        let mut engine = self.engine();
        let name = engine.browse.artist_name.clone();
        let photo = engine.artist_image(&name).map(|cover| cover.url);
        let bookmarked = engine.browse.is_bookmarked(&name);
        let page = engine.browse.artist.clone();
        let mut state = ArtistState {
            name: name.clone(),
            loading: page.is_none(),
            error: None,
            photo,
            listeners: 0,
            plays: 0,
            bio: String::new(),
            tags: Vec::new(),
            top_songs: Vec::new(),
            similar: Vec::new(),
            similar_photos: Vec::new(),
            url: String::new(),
            bookmarked,
        };
        match page {
            Some(Ok(page)) => {
                state.name = page.name.clone();
                state.listeners = page.listeners;
                state.plays = page.plays;
                state.bio = page.bio;
                state.tags = page.tags;
                state.top_songs = page
                    .top_tracks
                    .into_iter()
                    .map(|top| TopSong {
                        title: top.title,
                        plays: top.plays,
                    })
                    .collect();
                state.similar_photos = page
                    .similar
                    .iter()
                    .map(|name| engine.artist_image(name).map(|cover| cover.url))
                    .collect();
                state.similar = page
                    .similar
                    .into_iter()
                    .map(|name| ArtistHit {
                        name,
                        listeners: 0,
                        photo: None,
                    })
                    .collect();
                state.url = page.url;
            }
            Some(Err(error)) => state.error = Some(error),
            None => {}
        }
        state
    }

    pub fn profile(&self) -> ProfileState {
        let mut engine = self.engine();
        let top: Vec<String> = match &engine.browse.profile {
            Some(Ok(profile)) => profile.top_artists.iter().map(|a| a.name.clone()).collect(),
            _ => Vec::new(),
        };
        let photos: Vec<Option<String>> = top
            .iter()
            .map(|name| engine.artist_image(name).map(|cover| cover.url))
            .collect();
        let is_me = engine.browse.profile_user.is_empty() || engine.profile_is_mine();
        let user = if !is_me {
            engine.browse.profile_user.clone()
        } else if engine.lastfm_connected() {
            engine.settings.lastfm.username.clone()
        } else {
            String::new()
        };
        let song = |track: &musikpl_core::lastfm::browse::ProfileTrack| ProfileSong {
            artist: track.artist.clone(),
            title: track.title.clone(),
            plays: track.plays,
            when: track.when,
            now_playing: track.now_playing,
        };
        let mut state = ProfileState {
            is_friend: engine.friends.is_friend(&user),
            user,
            is_me,
            loading: engine.browse.profile_loading,
            error: None,
            avatar: None,
            scrobbles: 0,
            artists: 0,
            loved: 0,
            country: String::new(),
            url: String::new(),
            period: period_index(engine.browse.period),
            top_artists: Vec::new(),
            top_songs: Vec::new(),
            recent: Vec::new(),
        };
        match &engine.browse.profile {
            Some(Ok(profile)) => {
                state.avatar = profile.image.clone();
                state.scrobbles = profile.scrobbles;
                state.artists = profile.artists;
                state.loved = profile.loved;
                state.country = profile.country.clone();
                state.url = profile.url.clone();
                state.top_artists = profile
                    .top_artists
                    .iter()
                    .zip(photos)
                    .map(|(artist, photo)| ArtistHit {
                        name: artist.name.clone(),
                        listeners: artist.listeners,
                        photo,
                    })
                    .collect();
                state.top_songs = profile.top_tracks.iter().map(song).collect();
                state.recent = profile.recent.iter().map(song).collect();
            }
            Some(Err(error)) => state.error = Some(error.clone()),
            None => {}
        }
        state
    }

    pub fn settings(&self) -> SettingsState {
        let mut engine = self.engine();
        let connected = engine.lastfm_connected();
        let names: Vec<String> = engine
            .browse
            .bookmarks
            .iter()
            .map(|bookmark| bookmark.name.clone())
            .collect();
        let bookmarks = names
            .into_iter()
            .map(|name| Bookmark {
                photo: engine.artist_image(&name).map(|cover| cover.url),
                name,
            })
            .collect();
        SettingsState {
            auto_mix: engine.auto_mix,
            invite_popups: engine.settings.invite_popups,
            lastfm_user: if connected {
                engine.settings.lastfm.username.clone()
            } else {
                String::new()
            },
            lastfm_avatar: engine.browse.my_avatar.clone(),
            lastfm_scrobbles: engine.browse.my_scrobbles,
            lastfm_connecting: engine.lastfm_connect != LastFmConnect::Idle,
            lastfm_approval_url: match &engine.lastfm_connect {
                LastFmConnect::AwaitingApproval(url) => Some(url.clone()),
                _ => None,
            },
            lastfm_one_click: built_in_keys().is_some(),
            error: engine.settings_error.clone(),
            crossfade: engine.settings.crossfade.enabled,
            crossfade_seconds: engine.settings.crossfade.seconds,
            crossfade_curve: engine.settings.crossfade.curve.into(),
            crossfade_on_skip: engine.settings.crossfade.on_skip,
            bookmarks,
        }
    }

    /// Text the engine wants on the clipboard (a friend code), once.
    pub fn take_clipboard(&self) -> Option<String> {
        self.engine().clipboard.take()
    }

    pub fn dismiss_error(&self) {
        self.engine().error = None;
    }

    // --- Playback --------------------------------------------------------

    /// Play a song from a list: a fresh start, Auto Mix builds around it.
    pub fn play(&self, song: Song) {
        let mut engine = self.engine();
        if engine.is_guest() {
            engine.request_from_host(&song.track());
        } else {
            engine.play_search_track(song.track());
        }
    }

    pub fn add_to_mix(&self, song: Song) {
        let mut engine = self.engine();
        if engine.is_guest() {
            engine.request_from_host(&song.track());
        } else {
            engine.add_to_queue(song.track());
        }
    }

    pub fn toggle_like(&self, song: Song) {
        self.engine().toggle_like(&song.track());
    }

    /// Like or unlike what plays.
    pub fn toggle_like_current(&self) {
        let mut engine = self.engine();
        if let Some(track) = engine.shown_track().cloned() {
            engine.toggle_like(&track);
        }
    }

    pub fn toggle_pause(&self) {
        let mut engine = self.engine();
        if !engine.is_guest() {
            engine.toggle_pause();
        }
    }

    /// Pause or play, from outside the app's own buttons: the notification,
    /// headphones, Bluetooth, or the system (a call, headphones unplugged).
    pub fn set_paused(&self, paused: bool) {
        let mut engine = self.engine();
        let showing_paused = engine.player.paused || engine.resume_at.is_some();
        if !engine.is_guest() && showing_paused != paused {
            engine.toggle_pause();
        }
    }

    pub fn next(&self) {
        self.engine().next();
    }

    pub fn previous(&self) {
        self.engine().previous();
    }

    pub fn seek(&self, seconds: f64) {
        let mut engine = self.engine();
        if !engine.is_guest() {
            engine.seek(seconds);
        }
    }

    pub fn set_volume(&self, volume: u8) {
        self.engine().set_volume(volume);
    }

    pub fn set_auto_mix(&self, enabled: bool) {
        self.engine().set_auto_mix(enabled);
    }

    /// Play a queued song now.
    pub fn play_from_queue(&self, song: Song) {
        let mut engine = self.engine();
        engine.queue.remove_id(&song.id);
        engine.play_queue_track(song.track());
    }

    pub fn remove_from_queue(&self, index: u32) {
        self.engine().queue.remove(index as usize);
    }

    // --- Search and Home -------------------------------------------------

    pub fn search(&self, query: String) {
        let mut engine = self.engine();
        engine.search = query;
        engine.search();
    }

    pub fn go_home(&self) {
        self.engine().go_home();
    }

    pub fn refresh_home(&self) {
        self.engine().refresh_home(true);
    }

    pub fn more_in_home_row(&self, title: String) {
        self.engine().more_in_home_row(&title);
    }

    pub fn more_home_rows(&self) {
        self.engine().more_rows();
    }

    pub fn toggle_genre(&self, genre: String) {
        self.engine().toggle_genre(genre);
    }

    pub fn finish_genres(&self) {
        self.engine().finish_genres();
    }

    pub fn skip_genres(&self) {
        self.engine().skip_genres();
    }

    pub fn pick_genres(&self) {
        self.engine().pick_genres();
    }

    /// Back to the previous page; false when there is none (leave the app).
    pub fn go_back(&self) -> bool {
        let mut engine = self.engine();
        if !engine.nav.can_back() {
            return false;
        }
        engine.go_back();
        true
    }

    pub fn go_forward(&self) {
        self.engine().go_forward();
    }

    // --- Playlists -------------------------------------------------------

    pub fn open_playlist(&self, id: i64) {
        self.engine().open_playlist(id as u64);
    }

    /// A new, empty playlist, opened. Returns its id.
    pub fn create_playlist(&self, name: String) -> i64 {
        let mut engine = self.engine();
        let id = engine.new_playlist();
        engine.playlist_editing = false;
        if !name.trim().is_empty() && engine.library.playlists.rename(id, name.trim()) {
            engine.library.playlists_changed = true;
        }
        id as i64
    }

    /// Add a song to a playlist, or to a new one when `playlist_id` is None.
    pub fn add_to_playlist(&self, song: Song, playlist_id: Option<i64>) {
        let target = match playlist_id {
            Some(id) => PlaylistTarget::Existing(id as u64),
            None => PlaylistTarget::New,
        };
        let mut engine = self.engine();
        engine.add_to_playlist(song.track(), target);
        engine.playlist_editing = false;
    }

    pub fn rename_playlist(&self, id: i64, name: String) {
        let mut engine = self.engine();
        if engine.library.playlists.rename(id as u64, name.trim()) {
            engine.library.playlists_changed = true;
        }
    }

    pub fn delete_playlist(&self, id: i64) {
        self.engine().delete_playlist(id as u64);
    }

    pub fn toggle_playlist_favorite(&self, id: i64) {
        let mut engine = self.engine();
        if engine.library.playlists.toggle_favorite(id as u64) {
            engine.library.playlists_changed = true;
        }
    }

    pub fn remove_from_playlist(&self, id: i64, index: u32) {
        let mut engine = self.engine();
        if engine.library.playlists.remove(id as u64, index as usize) {
            engine.library.playlists_changed = true;
        }
    }

    pub fn move_in_playlist(&self, id: i64, from: u32, to: u32) {
        let mut engine = self.engine();
        if engine
            .library
            .playlists
            .reorder(id as u64, from as usize, to as usize)
        {
            engine.library.playlists_changed = true;
        }
    }

    /// Play a playlist: from the start, or a random song with shuffle on.
    pub fn play_playlist(&self, id: i64) {
        self.engine().play_playlist(id as u64);
    }

    /// Play a playlist from one of its songs.
    pub fn play_playlist_from(&self, id: i64, index: u32) {
        self.engine().play_playlist_from(id as u64, index as usize);
    }

    /// The shuffle button: off, on, smart.
    pub fn cycle_shuffle(&self) {
        self.engine().cycle_shuffle();
    }

    /// The repeat button: off, all, one.
    pub fn cycle_repeat(&self) {
        self.engine().cycle_repeat();
    }

    pub fn more_playlist_suggestions(&self) {
        self.engine().more_playlist_recs();
    }

    /// Use the photo at `path` (copied out of the photo picker) as the
    /// playlist's cover. An error says why it couldn't.
    pub fn set_playlist_cover(&self, id: i64, path: String) -> Option<String> {
        let saved = musikpl_core::covers::save_cover(
            std::path::Path::new(&path),
            &format!("playlist-{id}"),
        );
        match saved {
            Ok(name) => {
                self.engine().set_playlist_cover(id as u64, name);
                None
            }
            Err(error) => {
                self.engine().error = Some(AppError::Ui(error.clone()));
                Some(error)
            }
        }
    }

    pub fn remove_playlist_cover(&self, id: i64) {
        self.engine().remove_playlist_cover(id as u64);
    }

    // --- Last.fm: artists, profiles, account ------------------------------

    pub fn open_artist(&self, name: String) {
        self.engine().open_artist(&name);
    }

    /// The Last.fm page of a song's artist (the player's artist name).
    pub fn open_song_artist(&self, song: Song) {
        self.engine().open_track_artist(&song.track());
    }

    pub fn toggle_bookmark(&self) {
        self.engine().toggle_bookmark();
    }

    /// Find a Last.fm song on YouTube and play it.
    pub fn play_lastfm_song(&self, artist: String, title: String) {
        self.engine()
            .find_and_play(&artist, &title, SongIntent::Play);
    }

    /// Find a Last.fm song on YouTube and add it to the mix.
    pub fn add_lastfm_song(&self, artist: String, title: String) {
        self.engine()
            .find_and_play(&artist, &title, SongIntent::AddToMix);
    }

    /// Find a Last.fm song on YouTube and like it.
    pub fn like_lastfm_song(&self, artist: String, title: String) {
        self.engine()
            .find_and_play(&artist, &title, SongIntent::Like);
    }

    /// Find a Last.fm song on YouTube and add it to a playlist (or a new one).
    pub fn add_lastfm_song_to_playlist(
        &self,
        artist: String,
        title: String,
        playlist_id: Option<i64>,
    ) {
        let target = match playlist_id {
            Some(id) => PlaylistTarget::Existing(id as u64),
            None => PlaylistTarget::New,
        };
        self.engine()
            .find_and_play(&artist, &title, SongIntent::AddToPlaylist(target));
    }

    pub fn open_my_profile(&self) {
        self.engine().open_profile();
    }

    pub fn open_user_profile(&self, user: String) {
        self.engine().open_user_profile(&user);
    }

    /// 0 week, 1 month, 2 year, 3 all time.
    pub fn set_profile_period(&self, period: u8) {
        if let Some(period) = Period::ALL.get(period as usize) {
            self.engine().set_profile_period(*period);
        }
    }

    pub fn open_link(&self, url: String) {
        self.engine().open_link(&url);
    }

    pub fn connect_lastfm(&self) {
        self.engine().connect_lastfm("", "");
    }

    pub fn cancel_lastfm_connect(&self) {
        self.engine().cancel_lastfm_connect();
    }

    pub fn disconnect_lastfm(&self) {
        self.engine().disconnect_lastfm();
    }

    pub fn set_invite_popups(&self, enabled: bool) {
        self.engine().set_invite_popups(enabled);
    }

    pub fn clear_history(&self) {
        self.engine().clear_history();
    }

    pub fn set_crossfade(&self, enabled: bool, seconds: u8, curve: FadeShape, on_skip: bool) {
        self.engine()
            .set_crossfade(enabled, seconds, curve.into(), on_skip);
    }

    pub fn set_playlist_transition(
        &self,
        id: i64,
        kind: TransitionKind,
        seconds: u8,
        curve: FadeShape,
    ) {
        let transition = match kind {
            TransitionKind::Default => Transition::Default,
            TransitionKind::Off => Transition::Off,
            TransitionKind::Fade => Transition::Fade {
                seconds,
                curve: curve.into(),
            },
        };
        self.engine().set_playlist_transition(id as u64, transition);
    }
}

// ---------------------------------------------------------------------------
// Jams and friends
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum JamRole {
    None,
    Host,
    Guest,
}

/// A Jam hosted nearby (same Wi-Fi), ready to join.
#[derive(Clone, Debug, uniffi::Record)]
pub struct NearbyJam {
    pub session_id: u64,
    pub host: String,
    pub track: Option<String>,
    pub listeners: u32,
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum InternetShare {
    Off,
    Preparing,
    /// The invite to send (end-to-end encrypted; only people with it join).
    Ready {
        invite: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct JamState {
    pub role: JamRole,
    /// Guest: still connecting to the host.
    pub connecting: bool,
    /// Host: guests listening.
    pub listeners: u32,
    /// Guest: the host's name. Host: this device's.
    pub host_name: String,
    pub status: String,
    pub notice: Option<String>,
    /// Host: a new song waits for the guests to load it.
    pub waiting_for_guests: bool,
    pub internet: InternetShare,
    pub nearby: Vec<NearbyJam>,
    /// Why nearby Jams can't be found, if they can't.
    pub discovery_problem: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum FriendStatus {
    Online,
    Offline,
    /// Not checked yet.
    Unknown,
    /// Added from Last.fm; no friend code yet, so not seen online.
    NoCode,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FriendItem {
    pub index: u32,
    /// Their friend code's key (also the id of their Jam invites).
    pub id: Option<String>,
    pub name: String,
    pub lastfm: Option<String>,
    pub avatar: Option<String>,
    pub status: FriendStatus,
    /// They invited us to their Jam.
    pub invited_us: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FriendInvite {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FriendsState {
    pub friends: Vec<FriendItem>,
    /// This device hosts a Jam, so online friends can be invited.
    pub hosting: bool,
    pub notice: Option<String>,
    /// Jam invites waiting for Join or Not now.
    pub invites: Vec<FriendInvite>,
    /// Invites show as a pop-up (else quietly in the list).
    pub invite_popups: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LyricsKind {
    Idle,
    Loading,
    Synced,
    Plain,
    Instrumental,
    Missing,
    Failed,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LyricLine {
    /// Seconds into the song (0 for plain lyrics).
    pub at: f64,
    pub text: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LyricsState {
    pub kind: LyricsKind,
    pub lines: Vec<LyricLine>,
    pub error: Option<String>,
}

/// One of the user's devices: linked, or asking to be.
#[derive(Clone, Debug, uniffi::Record)]
pub struct DeviceItem {
    pub id: String,
    pub name: String,
    /// None: not checked yet.
    pub online: Option<bool>,
    pub synced: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DevicesState {
    pub linked: Vec<DeviceItem>,
    /// Devices of the same Last.fm user asking to link (Link or Not now).
    pub prompts: Vec<DeviceItem>,
    pub notice: Option<String>,
}

#[uniffi::export]
impl MobileEngine {
    pub fn jam(&self) -> JamState {
        let mut engine = self.engine();
        if engine
            .jam_notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > NOTICE_DURATION)
        {
            engine.jam_notice = None;
        }
        let info = &engine.jam.info;
        let role = match info.role {
            None => JamRole::None,
            Some(Role::Host) => JamRole::Host,
            Some(Role::Guest) => JamRole::Guest,
        };
        let guest = role == JamRole::Guest;
        JamState {
            role,
            connecting: info.connecting,
            listeners: info.peers as u32,
            host_name: info.host_name.clone(),
            status: if guest && !info.connecting {
                engine.jam_sync.status.clone()
            } else {
                info.status.clone()
            },
            notice: engine
                .jam_notice
                .as_ref()
                .map(|(text, _)| text.clone())
                .or_else(|| info.notice.clone()),
            waiting_for_guests: role == JamRole::Host && engine.jam_hold.is_some(),
            internet: match engine.jam.internet_status() {
                None => InternetShare::Off,
                Some(Progress::Preparing) => InternetShare::Preparing,
                Some(Progress::Ready(invite)) => InternetShare::Ready { invite },
                Some(Progress::Failed(reason)) => InternetShare::Failed { reason },
            },
            nearby: engine
                .jam
                .nearby()
                .into_iter()
                .map(|jam| NearbyJam {
                    session_id: jam.session_id,
                    host: jam.host_name,
                    track: jam.track,
                    listeners: jam.listeners,
                })
                .collect(),
            discovery_problem: match engine.jam.discovery_status() {
                DiscoveryStatus::Unavailable(reason) => Some(reason),
                _ => None,
            },
        }
    }

    /// Host a Jam on this Wi-Fi (share it over the internet afterwards).
    pub fn start_jam(&self) {
        let mut engine = self.engine();
        engine.jam_notice = None;
        if let Err(error) = engine.jam.host() {
            engine.error = Some(AppError::Jam(error));
        }
    }

    /// Leave (or, hosting, end) the Jam.
    pub fn end_jam(&self) {
        let mut engine = self.engine();
        if engine.is_guest() {
            engine.release_guest_playback();
        }
        engine.jam_notice = None;
        engine.jam.leave();
    }

    pub fn join_nearby_jam(&self, session_id: u64) {
        let mut engine = self.engine();
        let found = engine
            .jam
            .nearby()
            .into_iter()
            .find(|jam| jam.session_id == session_id);
        if let Some(jam) = found {
            engine.jam_notice = None;
            engine.error = None;
            engine.jam.join_nearby(&jam);
        }
    }

    /// Host: make an internet invite (end-to-end encrypted).
    pub fn share_jam_over_internet(&self) {
        self.engine().jam.share_over_internet();
    }

    /// Join a Jam over the internet with a pasted invite. An error says why
    /// it couldn't.
    pub fn join_jam_invite(&self, invite: String) -> Option<String> {
        let mut engine = self.engine();
        match engine.jam.join_invite(invite.trim()) {
            Ok(()) => {
                engine.jam_notice = None;
                engine.error = None;
                None
            }
            Err(error) => Some(error),
        }
    }

    pub fn friends(&self) -> FriendsState {
        let engine = self.engine();
        let state = &engine.friends;
        let friends = state
            .list
            .iter()
            .enumerate()
            .map(|(index, friend)| FriendItem {
                index: index as u32,
                id: friend.id.clone(),
                name: friend.name.clone(),
                lastfm: friend.lastfm.clone(),
                avatar: friend.avatar.clone(),
                status: match &friend.id {
                    None => FriendStatus::NoCode,
                    Some(id) => match state.online.get(id) {
                        Some(true) => FriendStatus::Online,
                        Some(false) => FriendStatus::Offline,
                        None => FriendStatus::Unknown,
                    },
                },
                invited_us: friend
                    .id
                    .as_ref()
                    .is_some_and(|id| state.invites.iter().any(|invite| &invite.id == id)),
            })
            .collect();
        FriendsState {
            friends,
            hosting: engine.jam.info.role == Some(Role::Host),
            notice: state.notice().map(str::to_owned),
            invites: state
                .invites
                .iter()
                .map(|invite| FriendInvite {
                    id: invite.id.clone(),
                    name: invite.name.clone(),
                })
                .collect(),
            invite_popups: engine.settings.invite_popups,
        }
    }

    /// The friends list is on screen: check who's online more often.
    pub fn set_friends_visible(&self, visible: bool) {
        let mut engine = self.engine();
        engine.menu_visible = visible;
        if visible {
            engine.refresh_friends();
            engine.refresh_follows();
        }
    }

    /// Put this device's friend code on the clipboard.
    pub fn copy_friend_code(&self) {
        let mut engine = self.engine();
        engine.clipboard = Some(engine.my_friend_code());
        engine.set_friend_notice("Your friend code is copied. Send it to a friend.");
    }

    /// Add a friend's code (it completes a Last.fm friend with that name).
    pub fn add_friend_code(&self, code: String) {
        let mut engine = self.engine();
        engine.friends.code_input = code;
        engine.add_friend_code();
    }

    pub fn remove_friend(&self, index: u32) {
        self.engine().remove_friend(index as usize);
    }

    pub fn invite_friend(&self, index: u32) {
        let mut engine = self.engine();
        let id = engine
            .friends
            .list
            .get(index as usize)
            .and_then(|friend| friend.id.clone());
        if let Some(id) = id {
            engine.invite_friend(&id);
        }
    }

    /// Join the Jam a friend invited us to. True when joining started.
    pub fn accept_friend_invite(&self, id: String) -> bool {
        self.engine().accept_invite(&id)
    }

    pub fn decline_friend_invite(&self, id: String) {
        self.engine().decline_invite(&id);
    }

    /// Add a Last.fm user as a friend (their profile's Add friend).
    pub fn add_lastfm_friend(&self, user: String) {
        self.engine().add_lastfm_friend(&user);
    }

    pub fn remove_lastfm_friend(&self, user: String) {
        self.engine().remove_lastfm_friend(&user);
    }

    /// The lyrics screen shows (lyrics are looked up only then).
    pub fn show_lyrics(&self, shown: bool) {
        self.engine().show_lyrics(shown);
    }

    pub fn retry_lyrics(&self) {
        self.engine().retry_lyrics();
    }

    /// Changes when the lyrics do (song or status): fetch `lyrics()` then.
    pub fn lyrics_key(&self) -> String {
        let engine = self.engine();
        let state = match &engine.lyrics.status {
            LyricsStatus::Idle => "idle",
            LyricsStatus::Loading => "loading",
            LyricsStatus::Found(_) => "found",
            LyricsStatus::Missing => "missing",
            LyricsStatus::Failed(_) => "failed",
        };
        format!("{}:{state}", engine.lyrics.id)
    }

    pub fn lyrics(&self) -> LyricsState {
        let engine = self.engine();
        let (kind, lines, error) = match &engine.lyrics.status {
            LyricsStatus::Idle => (LyricsKind::Idle, Vec::new(), None),
            LyricsStatus::Loading => (LyricsKind::Loading, Vec::new(), None),
            LyricsStatus::Missing => (LyricsKind::Missing, Vec::new(), None),
            LyricsStatus::Failed(error) => (LyricsKind::Failed, Vec::new(), Some(error.clone())),
            LyricsStatus::Found(found) if found.is_synced() => (
                LyricsKind::Synced,
                found
                    .synced
                    .iter()
                    .map(|line| LyricLine {
                        at: line.at,
                        text: line.text.clone(),
                    })
                    .collect(),
                None,
            ),
            LyricsStatus::Found(found) if !found.plain.is_empty() => (
                LyricsKind::Plain,
                found
                    .plain
                    .iter()
                    .map(|text| LyricLine {
                        at: 0.0,
                        text: text.clone(),
                    })
                    .collect(),
                None,
            ),
            LyricsStatus::Found(_) => (LyricsKind::Instrumental, Vec::new(), None),
        };
        LyricsState { kind, lines, error }
    }

    /// The line being sung, for synced lyrics.
    pub fn lyrics_line(&self) -> Option<u32> {
        self.engine().lyrics_line().map(|line| line as u32)
    }

    pub fn downloads(&self) -> DownloadsInfo {
        let engine = self.engine();
        let state = &engine.downloads;
        let marks = state.marks();
        DownloadsInfo {
            songs: state.songs.iter().map(Song::of).collect(),
            saved_ids: state
                .index
                .songs
                .iter()
                .map(|s| s.track.id.clone())
                .collect(),
            pending_ids: marks
                .iter()
                .filter(|(_, mark)| **mark != DownloadMark::Done)
                .map(|(id, _)| id.clone())
                .collect(),
            current: state.current().map(|(track, _)| track.title.clone()),
            percent: state.current().map_or(0, |(_, done)| done / 10),
            offline: engine.offline(),
            notice: state.notice().map(str::to_owned),
            megabytes: state.index.total_bytes() / (1024 * 1024),
        }
    }

    /// Download a song, or remove its download (or stop it).
    pub fn toggle_download(&self, song: Song) {
        let track = song.track();
        self.engine().toggle_download(&track);
    }

    pub fn toggle_playlist_download(&self, id: i64) {
        self.engine().toggle_playlist_download(id as u64);
    }

    /// The user's devices that sync playlists, and link requests.
    pub fn devices(&self) -> DevicesState {
        let engine = self.engine();
        let state = &engine.devices;
        let item = |id: &str, name: &str| DeviceItem {
            id: id.to_owned(),
            name: name.to_owned(),
            online: engine.friends.online.get(id).copied(),
            synced: state.synced_at.contains_key(id),
        };
        DevicesState {
            linked: state.linked.iter().map(|d| item(&d.id, &d.name)).collect(),
            prompts: state.prompts.iter().map(|p| item(&p.id, &p.name)).collect(),
            notice: state.notice().map(str::to_owned),
        }
    }

    pub fn accept_device_link(&self, id: String) {
        self.engine().accept_link(&id);
    }

    pub fn decline_device_link(&self, id: String) {
        self.engine().decline_link(&id);
    }

    pub fn unlink_device(&self, index: u32) {
        self.engine().unlink_device(index as usize);
    }
}

/// The device's name as its owner knows it ("Galaxy A56"): Jams and
/// friends show it.
#[uniffi::export]
pub fn set_device_name(name: String) {
    platform::set_device_name(&name);
}

/// The phone's networks ("192.168.1.67/24"), for finding Jams nearby.
#[uniffi::export]
pub fn set_networks(networks: Vec<String>) {
    let parsed = networks
        .iter()
        .filter_map(|network| {
            let (ip, prefix) = network.split_once('/')?;
            Some((ip.parse().ok()?, prefix.parse().ok()?))
        })
        .collect();
    musikpl_core::net::set_networks(parsed);
}

// ---------------------------------------------------------------------------
// Updates
// ---------------------------------------------------------------------------

/// The Android release file.
const APK_ASSET: &str = "MusiKPL-android-arm64.apk";

#[derive(Clone, Debug, uniffi::Record)]
pub struct Update {
    pub version: String,
    pub url: String,
}

/// Ask GitHub (found through Cloudflare's 1.1.1.1) for a newer MusiKPL.
/// None when this is the latest, or the release has no Android app.
#[uniffi::export]
pub fn check_for_update() -> Result<Option<Update>, MobileError> {
    let release =
        musikpl_core::updates::check().map_err(|reason| MobileError::Failed { reason })?;
    Ok(release.and_then(|release| {
        release.asset(APK_ASSET).map(|url| Update {
            version: release.version,
            url,
        })
    }))
}

/// Download progress, in per-mille.
#[uniffi::export(with_foreign)]
pub trait DownloadProgress: Send + Sync {
    fn progress(&self, per_mille: u32);
}

/// Download an update to `path` (through Cloudflare's DNS too). Blocks
/// until done; call it off the main thread.
#[uniffi::export]
pub fn download_update(
    url: String,
    path: String,
    progress: Arc<dyn DownloadProgress>,
) -> Result<(), MobileError> {
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    let done = Arc::new(AtomicBool::new(false));
    let amount = Arc::new(AtomicU32::new(0));
    let watcher = {
        let (done, amount) = (done.clone(), amount.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                progress.progress(amount.load(Ordering::Relaxed));
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            progress.progress(amount.load(Ordering::Relaxed));
        })
    };
    let result = musikpl_core::updates::download(&url, std::path::Path::new(&path), &amount);
    done.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    result.map_err(|reason| MobileError::Failed { reason })
}

/// This build's version ("3.5.0").
#[uniffi::export]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn songs_and_tracks_convert_both_ways() {
        let track = Track {
            id: "abc".into(),
            title: "Hardwell - Spaceman".into(),
            channel: "Revealed".into(),
            duration: Some(200.0),
        };
        let song = Song::of(&track);
        assert_eq!(song.artist, "Hardwell");
        assert!(song.cover.contains("abc"));
        assert_eq!(song.into_track(), track);
    }
}
