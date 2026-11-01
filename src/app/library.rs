//! The user's own data, kept in `%APPDATA%\MusiKPL`: what they listen to
//! and like (their taste), the session to come back to, and Home.
//!
//! Everything is saved in the background at most every few seconds, and
//! once more when the app closes, so the disk is barely touched.

use std::{
    collections::HashSet,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{song_info, Playlists, Track},
    infrastructure::storage,
    lastfm::{lookup_key, LastFmJob},
    media::query_ytdlp_limited,
    recommendations::{
        home::{finish_row, plan_home, HomeCache, RowPlan},
        rng::Rng,
        taste::{genres_from_tags, ListenTracker, Taste},
    },
};

use super::{home_paging::HomePaging, YtMusicApp};

const TASTE_FILE: &str = "taste.json";
const SESSION_FILE: &str = "session.json";
const HOME_FILE: &str = "home.json";
const PLAYLISTS_FILE: &str = "playlists.json";
/// How often changed data is written, genres are looked up, and Home is
/// checked for staleness.
const SAVE_INTERVAL: Duration = Duration::from_secs(10);
/// New genre lookups started per tick.
const TAG_LOOKUPS: usize = 4;
/// After genre lookups fail (offline, say), wait this long before more.
const TAG_RETRY: Duration = Duration::from_secs(300);
/// After Home couldn't be built, wait this long before trying again.
const HOME_RETRY: Duration = Duration::from_secs(600);
/// Queued songs kept in the session.
const SESSION_QUEUE: usize = 100;
/// Songs kept for the Liked and Recently played rows, shown a page at a time.
const LOCAL_ROW: usize = 200;

/// Seconds since 1970, the clock taste and Home are stamped with.
pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}

/// What was playing, shown again (paused) on the next start.
#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Session {
    track: Option<Track>,
    position: f64,
    queue: Vec<Track>,
    /// Queued songs that Auto Mix picked.
    picks: Vec<String>,
}

/// Each planned Home row with the songs its sources returned.
pub(super) type HomeBuild = Vec<(RowPlan, Vec<Track>)>;

#[derive(Default)]
pub(super) struct Library {
    pub(super) taste: Taste,
    pub(super) playlists: Playlists,
    pub(super) playlists_changed: bool,
    listens: ListenTracker,
    /// Data was loaded from disk, so it may be saved back. Tests never set it.
    persist: bool,
    taste_changed: bool,
    pub(super) settings_changed: bool,
    last_tick: Option<Instant>,
    /// The session as last written, to skip saving it unchanged.
    saved_session: String,
    tags_pending: HashSet<String>,
    tags_retry_at: Option<Instant>,
    pub(super) home: HomeCache,
    home_rx: Option<Receiver<HomeBuild>>,
    home_retry_at: Option<Instant>,
    /// Loading more of Home as the user scrolls.
    pub(super) paging: HomePaging,
    /// Liked and recently played songs, refreshed when taste changes.
    pub(super) liked: Vec<Track>,
    pub(super) recent: Vec<Track>,
}

impl Library {
    pub(super) fn building_home(&self) -> bool {
        self.home_rx.is_some()
    }

    fn taste_changed(&mut self) {
        self.taste_changed = true;
        self.liked = self.taste.liked(LOCAL_ROW);
        self.recent = self.taste.recently_played(LOCAL_ROW);
    }

    /// Every song Home shows, for cover lookups and the image cache.
    pub(super) fn home_tracks(&self) -> impl Iterator<Item = &Track> {
        self.home
            .rows
            .iter()
            .flat_map(|row| row.tracks.iter())
            .chain(self.liked.iter())
            .chain(self.recent.iter())
    }
}

impl YtMusicApp {
    /// Load taste, Home, and the last session, and apply saved preferences.
    /// Call after `load_settings`.
    pub(super) fn load_library(&mut self) {
        self.library.persist = true;
        self.library.taste = storage::read(TASTE_FILE).unwrap_or_default();
        self.library.home = storage::read(HOME_FILE).unwrap_or_default();
        self.library.playlists = storage::read(PLAYLISTS_FILE).unwrap_or_default();
        self.library.taste_changed();
        self.library.taste_changed = false;
        self.player.volume = self.settings.volume;
        self.auto_mix = self.settings.auto_mix;
        if let Some(session) = storage::read::<Session>(SESSION_FILE) {
            self.restore_session(session);
        }
        self.library.saved_session = serde_json::to_string(&self.session()).unwrap_or_default();
        // New recommendations on every start; the saved Home shows meanwhile.
        self.refresh_home(true);
    }

    /// Show the last song, paused where it was left, without loading it:
    /// nothing starts (no mpv, no yt-dlp) until the user presses play.
    fn restore_session(&mut self, session: Session) {
        for track in session.queue {
            self.queue.enqueue_unique(track);
        }
        self.automix.picks = session
            .picks
            .into_iter()
            .filter(|id| self.queue.contains_id(id))
            .collect();
        if let Some(track) = session.track {
            let position = session.position.max(0.0);
            self.player.show_paused_at(position);
            self.current = Some(track);
            self.resume_at = Some(position);
        }
    }

    fn session(&self) -> Session {
        let track = self.shown_track().cloned();
        let position = if self.loading {
            0.0
        } else {
            self.resume_at
                .unwrap_or_else(|| self.player.position_seconds())
                .floor()
        };
        Session {
            track,
            position,
            queue: self.queue.iter().take(SESSION_QUEUE).cloned().collect(),
            picks: {
                // Sorted, so an unchanged session encodes the same way.
                let mut picks: Vec<String> = self.automix.picks.iter().cloned().collect();
                picks.sort();
                picks
            },
        }
    }

    /// Called every frame: follow listening, collect background results,
    /// and every few seconds save, look up genres, and check Home.
    pub(super) fn tick_library(&mut self) {
        self.follow_listening();
        self.poll_home();
        if self
            .library
            .last_tick
            .is_some_and(|last| last.elapsed() < SAVE_INTERVAL)
        {
            return;
        }
        self.library.last_tick = Some(Instant::now());
        // Linux: switch to MusiKPL's own yt-dlp once it has downloaded.
        if let Some(managed) = crate::infrastructure::bundled::managed_yt_dlp() {
            if self.tools.yt_dlp != managed {
                self.tools.yt_dlp = managed;
            }
        }
        self.save_library(false);
        self.request_tags();
        self.refresh_home(false);
    }

    /// Turn playback into taste: how long each song was heard, and whether
    /// it was finished or skipped.
    fn follow_listening(&mut self) {
        let track = if self.loading || self.resume_at.is_some() {
            None
        } else {
            self.current.as_ref()
        };
        let playing = !self.player.paused && self.player.has_started();
        let clock = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |time| time.as_secs_f64());
        let position = self.player.position_seconds();
        if let Some((track, listened, end)) =
            self.library.listens.update(track, position, playing, clock)
        {
            self.library.taste.record(&track, listened, end, unix_now());
            self.library.taste_changed();
        }
    }

    /// Write whatever changed. `now` writes on this thread (when closing);
    /// otherwise files are written in the background.
    pub(super) fn save_library(&mut self, now: bool) {
        if !self.library.persist {
            return;
        }
        if now {
            if let Some((track, listened, end)) = self.library.listens.finish() {
                self.library.taste.record(&track, listened, end, unix_now());
                self.library.taste_changed = true;
            }
        }
        if std::mem::take(&mut self.library.taste_changed) {
            if now {
                let _ = storage::write(TASTE_FILE, &self.library.taste);
            } else {
                storage::write_in_background(TASTE_FILE, &self.library.taste);
            }
        }
        if std::mem::take(&mut self.library.playlists_changed) {
            if now {
                let _ = storage::write(PLAYLISTS_FILE, &self.library.playlists);
            } else {
                storage::write_in_background(PLAYLISTS_FILE, &self.library.playlists);
            }
        }
        if std::mem::take(&mut self.library.settings_changed) {
            if now {
                let _ = self.settings.save();
            } else {
                self.settings.save_in_background();
            }
        }
        // A guest plays the host's songs; keep the user's own session.
        if !self.is_guest() {
            let session = self.session();
            let text = serde_json::to_string(&session).unwrap_or_default();
            if text != self.library.saved_session {
                if now {
                    let _ = storage::write(SESSION_FILE, &session);
                } else {
                    storage::write_in_background(SESSION_FILE, &session);
                }
                self.library.saved_session = text;
            }
        }
    }

    /// Ask Last.fm for the genres of a few listened songs not looked up yet.
    fn request_tags(&mut self) {
        if self
            .library
            .tags_retry_at
            .is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        let Some(api_key) = lookup_key(&self.settings.lastfm.api_key) else {
            return;
        };
        let pending = self.library.tags_pending.len();
        let wanted: Vec<Track> = self
            .library
            .taste
            .needing_tags(TAG_LOOKUPS + pending)
            .into_iter()
            .filter(|track| !self.library.tags_pending.contains(&track.id))
            .take(TAG_LOOKUPS)
            .collect();
        for track in wanted {
            self.library.tags_pending.insert(track.id.clone());
            self.lastfm.submit(LastFmJob::Tags {
                api_key: api_key.clone(),
                video_id: track.id.clone(),
                song: song_info(&track),
            });
        }
    }

    /// A genre lookup finished; `None` means it failed and is retried later.
    pub(super) fn receive_tags(&mut self, video_id: &str, tags: Option<Vec<String>>) {
        self.library.tags_pending.remove(video_id);
        match tags {
            Some(tags) => {
                self.library
                    .taste
                    .set_tags(video_id, genres_from_tags(tags));
                self.library.taste_changed = true;
            }
            None => self.library.tags_retry_at = Some(Instant::now() + TAG_RETRY),
        }
    }

    /// Like or unlike a song; with Last.fm connected, it is loved or
    /// unloved on the user's profile too.
    pub(super) fn toggle_like(&mut self, track: &Track) {
        let liked = self.library.taste.toggle_like(track, unix_now());
        self.library.taste_changed();
        if let Some(credentials) = self.credentials() {
            self.lastfm.submit(LastFmJob::Love {
                credentials,
                song: song_info(track),
                love: liked,
            });
        }
    }

    pub(super) fn is_liked(&self, id: &str) -> bool {
        self.library.taste.is_liked(id)
    }

    /// Forget everything listened to and liked.
    pub(super) fn clear_taste(&mut self) {
        self.library.taste = Taste::default();
        self.library.listens = ListenTracker::default();
        self.library.taste_changed();
        let old = std::mem::take(&mut self.library.home);
        self.retire_covers(old.rows.into_iter().flat_map(|row| row.tracks).collect());
        if self.library.persist {
            storage::delete(HOME_FILE);
        }
        self.refresh_home(true);
    }

    /// Rebuild Home in the background when it is old, or when asked to.
    /// Before the first-run genre picker is done there is nothing to go on.
    pub(super) fn refresh_home(&mut self, force: bool) {
        if self.library.building_home() {
            return;
        }
        let now = unix_now();
        if !force {
            let waiting = self
                .library
                .home_retry_at
                .is_some_and(|at| Instant::now() < at);
            if waiting || !self.library.home.is_stale(now) {
                return;
            }
        }
        if !self.settings.onboarded && !self.library.taste.has_history() {
            return;
        }
        let plans = plan_home(
            &self.library.taste,
            &self.settings.favorite_genres,
            &self.followed_artists(),
            now,
            &mut Rng::from_time(),
        );
        self.library.home_rx = Some(self.spawn_home_build(plans));
    }

    /// Search for each planned row's songs on a background thread, one
    /// yt-dlp at a time so building Home never hogs the PC.
    pub(super) fn spawn_home_build(&self, plans: Vec<RowPlan>) -> Receiver<HomeBuild> {
        let tools = self.tools.clone();
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut build = Vec::new();
            for plan in plans {
                let mut candidates = Vec::new();
                for source in &plan.sources {
                    if let Ok(items) =
                        query_ytdlp_limited(&tools, source, true, Some(30), &registry)
                    {
                        candidates.extend(items);
                    }
                }
                build.push((plan, candidates));
            }
            let _ = sender.send(build);
        });
        receiver
    }

    pub(super) fn save_home(&self) {
        if self.library.persist {
            storage::write_in_background(HOME_FILE, &self.library.home);
        }
    }

    fn poll_home(&mut self) {
        let Some(receiver) = &self.library.home_rx else {
            return;
        };
        let build = match receiver.try_recv() {
            Ok(build) => build,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        self.library.home_rx = None;
        let now = unix_now();
        // Prefer songs the previous Home didn't show, so it feels new.
        let shown: HashSet<String> = self
            .library
            .home
            .rows
            .iter()
            .flat_map(|row| row.tracks.iter().map(|track| track.id.clone()))
            .collect();
        let mut rng = Rng::from_time();
        let rows: Vec<_> = build
            .into_iter()
            .filter_map(|(plan, candidates)| {
                finish_row(
                    &plan,
                    candidates,
                    &self.library.taste,
                    now,
                    &shown,
                    &mut rng,
                )
            })
            .collect();
        if rows.is_empty() {
            // Offline or YouTube said no: keep what was there, try later.
            self.library.home_retry_at = Some(Instant::now() + HOME_RETRY);
            return;
        }
        self.library.home_retry_at = None;
        self.library.paging = HomePaging::default();
        let old = std::mem::replace(
            &mut self.library.home,
            HomeCache {
                built_at: now,
                rows,
            },
        );
        self.retire_covers(old.rows.into_iter().flat_map(|row| row.tracks).collect());
        if self.library.persist {
            storage::write_in_background(HOME_FILE, &self.library.home);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: format!("Song {id}"),
            id: id.into(),
            duration: Some(200.0),
            channel: "Artist".into(),
        }
    }

    #[test]
    fn a_restored_session_waits_paused_without_loading() {
        let mut app = YtMusicApp::default();
        app.restore_session(Session {
            track: Some(track("last")),
            position: 42.0,
            queue: vec![track("a"), track("b")],
            picks: vec!["b".into(), "gone".into()],
        });
        assert_eq!(app.current.as_ref().map(|t| t.id.as_str()), Some("last"));
        assert!(app.player.paused && !app.loading);
        assert_eq!(app.resume_at, Some(42.0));
        assert_eq!(app.player.position_seconds(), 42.0);
        assert_eq!(app.queue.len(), 2);
        assert!(app.automix.picks.contains("b") && !app.automix.picks.contains("gone"));

        let session = app.session();
        assert_eq!(session.position, 42.0);
        assert_eq!(session.track.map(|t| t.id), Some("last".into()));
    }

    #[test]
    fn likes_show_up_in_the_liked_row() {
        let mut app = YtMusicApp::default();
        app.toggle_like(&track("x"));
        assert!(app.is_liked("x"));
        assert_eq!(app.library.liked.len(), 1);
        app.toggle_like(&track("x"));
        assert!(!app.is_liked("x") && app.library.liked.is_empty());
    }

    #[test]
    fn failed_genre_lookups_back_off() {
        let mut app = YtMusicApp::default();
        app.library.tags_pending.insert("x".into());
        app.receive_tags("x", None);
        assert!(app.library.tags_pending.is_empty());
        assert!(app.library.tags_retry_at.is_some());
    }
}
