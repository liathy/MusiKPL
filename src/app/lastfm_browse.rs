//! Browsing Last.fm inside MusiKPL: artists found by a search, artist pages
//! (with bookmarks that feed recommendations), the connected user's profile
//! and stats, artist photos from YouTube, and playing a Last.fm song by
//! finding it on YouTube.

use std::{
    collections::{HashMap, VecDeque},
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{youtube_thumbnail, Cover, Track},
    infrastructure::storage,
    lastfm::{
        browse::{ArtistHit, ArtistPage, Period, Profile},
        lookup_key, BrowseRequest, LastFmEvent,
    },
    media::query_ytdlp_limited,
    recommendations::home::Followed,
    ui::{
        artist::{ArtistAction, SongPick},
        profile::ProfileAction,
    },
};

use super::{errors::AppError, View, YtMusicApp};

const BOOKMARKS_FILE: &str = "artists.json";
const IMAGES_FILE: &str = "artist_images.json";
/// A profile older than this is fetched again when opened.
const PROFILE_FRESH: Duration = Duration::from_secs(5 * 60);

/// An artist the user bookmarked: shown in the menu, and used for Home and
/// Auto Mix. Similar artists are kept for "Similar to …" rows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Bookmark {
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) similar: Vec<String>,
}

/// What to do with a Last.fm song once it's found on YouTube.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SongIntent {
    Play,
    AddToMix,
    Like,
    AddToPlaylist(crate::ui::menu::PlaylistTarget),
}

/// Pictures for one frame: the user's avatar and artists' photos.
#[derive(Default)]
pub(super) struct Photos {
    /// The open profile's picture.
    pub(super) avatar: Option<Cover>,
    /// The user's own picture, for the menu.
    pub(super) my_avatar: Option<Cover>,
    pub(super) search: Vec<Option<Cover>>,
    pub(super) artist: Option<Cover>,
    pub(super) similar: Vec<Option<Cover>>,
    pub(super) top_artists: Vec<Option<Cover>>,
    pub(super) bookmarks: Vec<Option<Cover>>,
}

#[derive(Default)]
pub(super) struct Browse {
    /// Artists for the current search, and the query they're for.
    pub(super) artists: Vec<ArtistHit>,
    artists_query: String,
    /// The artist page open (or last opened), and its result once loaded.
    pub(super) artist_name: String,
    pub(super) artist: Option<Result<ArtistPage, String>>,
    pub(super) profile: Option<Result<Profile, String>>,
    /// Whose profile profile is (the user's own, or a friend's).
    pub(super) profile_user: String,
    /// The user's own avatar and scrobbles, for the menu: kept apart so
    /// opening someone else's profile doesn't replace them.
    pub(super) my_avatar: Option<String>,
    pub(super) my_scrobbles: Option<u64>,
    /// Songs Last.fm found for the search, shown when YouTube finds none.
    pub(super) tracks: Vec<crate::lastfm::browse::TrackHit>,
    /// Last.fm users found by the search (an exact username match).
    pub(super) people: Vec<Profile>,
    profile_at: Option<Instant>,
    pub(super) profile_loading: bool,
    pub(super) period: Period,
    pub(super) bookmarks: Vec<Bookmark>,
    /// Artist name (lower-case) to the YouTube video whose cover is its
    /// photo; `None` when none was found.
    images: HashMap<String, Option<String>>,
    image_queue: VecDeque<String>,
    image_rx: Option<Receiver<(String, Option<String>)>>,
    song_rx: Option<(Receiver<Result<Track, String>>, SongIntent)>,
    /// YouTube artist names (lower-case) to the Last.fm artist found for
    /// them, so a second click opens straight away.
    resolved: HashMap<String, String>,
}

impl Browse {
    pub(super) fn busy(&self) -> bool {
        self.image_rx.is_some() || self.song_rx.is_some() || self.profile_loading
    }

    pub(super) fn is_bookmarked(&self, name: &str) -> bool {
        self.bookmarks
            .iter()
            .any(|bookmark| bookmark.name.eq_ignore_ascii_case(name))
    }

    pub(super) fn finding_song(&self) -> bool {
        self.song_rx.is_some()
    }
}

impl YtMusicApp {
    pub(super) fn load_browse(&mut self) {
        self.browse.bookmarks = storage::read(BOOKMARKS_FILE).unwrap_or_default();
        self.browse.images = storage::read(IMAGES_FILE).unwrap_or_default();
    }

    fn lookup_key(&self) -> Option<String> {
        lookup_key(&self.settings.lastfm.api_key)
    }

    /// With a song search, look for artists of that name too.
    pub(super) fn search_artists(&mut self, query: &str) {
        self.browse.artists.clear();
        self.browse.tracks.clear();
        self.browse.people.clear();
        self.browse.artists_query = query.to_owned();
        let single = query.starts_with("http://") || query.starts_with("https://");
        if let (Some(api_key), false) = (self.lookup_key(), single) {
            self.lastfm.browse(BrowseRequest::Artists {
                api_key: api_key.clone(),
                query: query.to_owned(),
            });
            self.lastfm.browse(BrowseRequest::Tracks {
                api_key: api_key.clone(),
                query: query.to_owned(),
            });
            self.lastfm.browse(BrowseRequest::People {
                api_key,
                query: query.to_owned(),
            });
        }
    }

    /// Fetch friends' Last.fm pictures, once a run each, so the Friends
    /// card shows them (a saved one shows meanwhile).
    pub(super) fn request_friend_avatars(&mut self) {
        let Some(api_key) = self.lookup_key() else {
            return;
        };
        let users: Vec<String> = self
            .friends
            .list
            .iter()
            .filter_map(|friend| friend.lastfm.clone())
            .filter(|user| !self.friends.avatars_asked.contains(&user.to_lowercase()))
            .collect();
        for user in users {
            self.friends.avatars_asked.insert(user.to_lowercase());
            self.lastfm.browse(BrowseRequest::People {
                api_key: api_key.clone(),
                query: user,
            });
        }
    }

    /// Open someone's Last.fm profile (a friend's, or a person found by
    /// search).
    pub(super) fn open_user_profile(&mut self, user: &str) {
        self.view = View::Profile;
        if !self.browse.profile_user.eq_ignore_ascii_case(user) {
            self.browse.profile_user = user.to_owned();
            self.browse.profile = None;
            self.browse.profile_at = None;
        }
        let fresh = self
            .browse
            .profile_at
            .is_some_and(|at| at.elapsed() < PROFILE_FRESH);
        if !fresh || !matches!(self.browse.profile, Some(Ok(_))) {
            self.request_profile();
        }
    }

    /// Whether the profile shown is the user's own.
    pub(super) fn profile_is_mine(&self) -> bool {
        !self.settings.lastfm.username.is_empty()
            && self
                .browse
                .profile_user
                .eq_ignore_ascii_case(&self.settings.lastfm.username)
    }

    pub(super) fn open_artist(&mut self, name: &str) {
        self.view = View::Artist;
        if self.browse.artist_name.eq_ignore_ascii_case(name) && self.browse.artist.is_some() {
            return;
        }
        self.browse.artist_name = name.to_owned();
        self.browse.artist = None;
        match self.lookup_key() {
            Some(api_key) => self.lastfm.browse(BrowseRequest::Artist {
                api_key,
                name: name.to_owned(),
            }),
            None => {
                self.browse.artist = Some(Err(
                    "Artist pages need a Last.fm API key (Settings → Last.fm)".into(),
                ))
            }
        }
    }

    /// Open the Last.fm page of a YouTube song's artist (the player bar's
    /// artist name). The most listened Last.fm artist named like it is
    /// picked, since Last.fm has many with similar names.
    pub(super) fn open_track_artist(&mut self, track: &Track) {
        let names = crate::domain::artist_candidates(track);
        let Some(first) = names.first().cloned() else {
            return;
        };
        if let Some(name) = self.browse.resolved.get(&first.to_lowercase()).cloned() {
            self.open_artist(&name);
            return;
        }
        self.view = View::Artist;
        if self.browse.artist_name.eq_ignore_ascii_case(&first) && self.browse.artist.is_some() {
            return;
        }
        self.browse.artist_name = first;
        self.browse.artist = None;
        match self.lookup_key() {
            Some(api_key) => self
                .lastfm
                .browse(BrowseRequest::BestArtist { api_key, names }),
            None => {
                self.browse.artist = Some(Err(
                    "Artist pages need a Last.fm API key (Settings → Last.fm)".into(),
                ))
            }
        }
    }

    pub(super) fn toggle_bookmark(&mut self) {
        let name = self.browse.artist_name.clone();
        if self.browse.is_bookmarked(&name) {
            self.browse
                .bookmarks
                .retain(|bookmark| !bookmark.name.eq_ignore_ascii_case(&name));
        } else {
            let (name, similar) = match &self.browse.artist {
                Some(Ok(page)) => (page.name.clone(), page.similar.clone()),
                _ => (name, Vec::new()),
            };
            self.browse.bookmarks.insert(0, Bookmark { name, similar });
        }
        storage::write_in_background(BOOKMARKS_FILE, &self.browse.bookmarks);
    }

    /// Open the connected user's profile, fetching it when it's old.
    pub(super) fn open_profile(&mut self) {
        let me = self.settings.lastfm.username.clone();
        self.open_user_profile(&me);
    }

    /// Load the profile in the background (at startup, after connecting),
    /// so the menu card can show it.
    pub(super) fn prefetch_profile(&mut self) {
        if self.lastfm_connected() {
            self.browse
                .profile_user
                .clone_from(&self.settings.lastfm.username);
            self.request_profile();
        }
    }

    pub(super) fn set_profile_period(&mut self, period: Period) {
        if self.browse.period != period {
            self.browse.period = period;
            self.request_profile();
        }
    }

    fn request_profile(&mut self) {
        if self.browse.profile_user.is_empty() {
            self.browse
                .profile_user
                .clone_from(&self.settings.lastfm.username);
        }
        let user = self.browse.profile_user.clone();
        let Some(api_key) = self.lookup_key().filter(|_| !user.is_empty()) else {
            return;
        };
        self.browse.profile_loading = true;
        self.lastfm.browse(BrowseRequest::Profile {
            api_key,
            user,
            period: self.browse.period,
        });
    }

    /// Handle a browse result from Last.fm. Returns the event back when it
    /// isn't one.
    pub(super) fn receive_browse(&mut self, event: LastFmEvent) -> Option<LastFmEvent> {
        match event {
            LastFmEvent::Artists { query, result } => {
                // A newer search may have replaced this one.
                if query == self.browse.artists_query {
                    self.browse.artists = result.unwrap_or_default();
                }
            }
            LastFmEvent::Tracks { query, result } => {
                if query == self.browse.artists_query {
                    self.browse.tracks = result.unwrap_or_default();
                }
            }
            LastFmEvent::People { query, result } => {
                if self.friends.is_friend(&query) {
                    if let Ok(Some(person)) = &result {
                        self.set_friend_avatar(&query, person.image.clone());
                    }
                }
                if query == self.browse.artists_query {
                    self.browse.people = result.ok().flatten().into_iter().collect();
                }
            }
            LastFmEvent::Artist { name, result } => {
                if let Ok(page) = &result {
                    if !page.name.eq_ignore_ascii_case(&name) {
                        self.browse
                            .resolved
                            .insert(name.to_lowercase(), page.name.clone());
                    }
                }
                if name.eq_ignore_ascii_case(&self.browse.artist_name) {
                    // Show Last.fm's spelling of the name (and bookmark,
                    // photo and history under it).
                    if let Ok(page) = &result {
                        if self.browse.artist_name != page.name {
                            self.browse.artist_name.clone_from(&page.name);
                            self.artist_renamed();
                        }
                    }
                    if let Ok(page) = &result {
                        // Keep a bookmark's similar artists current.
                        if let Some(bookmark) = self
                            .browse
                            .bookmarks
                            .iter_mut()
                            .find(|bookmark| bookmark.name.eq_ignore_ascii_case(&page.name))
                        {
                            if bookmark.similar != page.similar {
                                bookmark.similar = page.similar.clone();
                                storage::write_in_background(
                                    BOOKMARKS_FILE,
                                    &self.browse.bookmarks,
                                );
                            }
                        }
                    }
                    self.browse.artist = Some(result);
                }
            }
            LastFmEvent::Profile {
                user,
                period,
                result,
            } => {
                if user.eq_ignore_ascii_case(&self.settings.lastfm.username) {
                    if let Ok(profile) = &result {
                        self.browse.my_avatar.clone_from(&profile.image);
                        self.browse.my_scrobbles = Some(profile.scrobbles);
                    }
                }
                if user == self.browse.profile_user && period == self.browse.period {
                    self.browse.profile_loading = false;
                    if result.is_ok() {
                        self.browse.profile_at = Some(Instant::now());
                    }
                    self.browse.profile = Some(result);
                }
            }
            other => return Some(other),
        }
        None
    }

    /// The artist's photo: the cover of their top YouTube result, looked up
    /// once in the background and remembered.
    pub(super) fn artist_image(&mut self, name: &str) -> Option<Cover> {
        let key = name.to_lowercase();
        match self.browse.images.get(&key) {
            Some(Some(video)) => youtube_thumbnail(video),
            Some(None) => None,
            None => {
                if !self.browse.image_queue.contains(&key) {
                    self.browse.image_queue.push_back(key);
                }
                None
            }
        }
    }

    /// Called every frame: one artist photo lookup at a time, and songs
    /// being found on YouTube.
    pub(super) fn tick_browse(&mut self) {
        if let Some(rx) = &self.browse.image_rx {
            match rx.try_recv() {
                Ok((name, video)) => {
                    self.browse.images.insert(name, video);
                    self.browse.image_rx = None;
                    storage::write_in_background(IMAGES_FILE, &self.browse.images);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.browse.image_rx = None,
            }
        }
        if self.browse.image_rx.is_none() {
            if let Some(name) = self.browse.image_queue.pop_front() {
                let (tools, registry) = (self.tools.clone(), self.registry.clone());
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || {
                    let video = query_ytdlp_limited(
                        &tools,
                        &format!("ytsearch1:{name} music"),
                        true,
                        Some(1),
                        &registry,
                    )
                    .ok()
                    .and_then(|tracks| tracks.into_iter().next())
                    .map(|track| track.id);
                    let _ = sender.send((name, video));
                });
                self.browse.image_rx = Some(receiver);
            }
        }

        let Some((rx, intent)) = &self.browse.song_rx else {
            return;
        };
        let intent = *intent;
        let found = match rx.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The search stopped unexpectedly".into()),
        };
        self.browse.song_rx = None;
        match (found, intent) {
            (Ok(track), SongIntent::Play | SongIntent::AddToMix) if self.is_guest() => {
                self.request_from_host(&track)
            }
            (Ok(track), SongIntent::Play) => self.play_search_track(track),
            (Ok(track), SongIntent::AddToMix) => self.add_to_queue(track),
            (Ok(track), SongIntent::Like) => {
                if !self.is_liked(&track.id) {
                    self.toggle_like(&track);
                }
            }
            (Ok(track), SongIntent::AddToPlaylist(target)) => self.add_to_playlist(track, target),
            (Err(error), _) => self.error = Some(AppError::Search(error)),
        }
    }

    /// Bookmarked artists, for Home's recommendations.
    pub(super) fn followed_artists(&self) -> Vec<Followed> {
        self.browse
            .bookmarks
            .iter()
            .map(|bookmark| Followed {
                name: bookmark.name.clone(),
                similar: bookmark.similar.clone(),
            })
            .collect()
    }

    /// Photos for what is about to be drawn; missing ones are queued for
    /// lookup.
    pub(super) fn gather_photos(&mut self) -> Photos {
        let mut photos = Photos {
            avatar: match &self.browse.profile {
                Some(Ok(profile)) => profile.image.clone().map(|url| Cover {
                    url,
                    uv: [0.0, 0.0, 1.0, 1.0],
                }),
                _ => None,
            },
            my_avatar: self.browse.my_avatar.clone().map(|url| Cover {
                url,
                uv: [0.0, 0.0, 1.0, 1.0],
            }),
            ..Photos::default()
        };
        let names = |list: &[String]| list.to_vec();
        match self.view {
            View::Search => {
                let artists: Vec<String> = self
                    .browse
                    .artists
                    .iter()
                    .take(6)
                    .map(|artist| artist.name.clone())
                    .collect();
                photos.search = artists.iter().map(|name| self.artist_image(name)).collect();
            }
            View::Artist => {
                let name = self.browse.artist_name.clone();
                photos.artist = self.artist_image(&name);
                let similar = match &self.browse.artist {
                    Some(Ok(page)) => names(&page.similar),
                    _ => Vec::new(),
                };
                photos.similar = similar.iter().map(|name| self.artist_image(name)).collect();
            }
            View::Profile => {
                let top: Vec<String> = match &self.browse.profile {
                    Some(Ok(profile)) => profile
                        .top_artists
                        .iter()
                        .map(|artist| artist.name.clone())
                        .collect(),
                    _ => Vec::new(),
                };
                photos.top_artists = top.iter().map(|name| self.artist_image(name)).collect();
            }
            _ => {}
        }
        if self.menu_open {
            let bookmarks: Vec<String> = self
                .browse
                .bookmarks
                .iter()
                .map(|bookmark| bookmark.name.clone())
                .collect();
            photos.bookmarks = bookmarks
                .iter()
                .map(|name| self.artist_image(name))
                .collect();
        } else {
            photos.bookmarks = vec![None; self.browse.bookmarks.len()];
        }
        photos
    }

    /// Open Settings at the Last.fm page, where the account is connected.
    pub(super) fn connect_lastfm_from_ui(&mut self) {
        self.settings_page = crate::ui::overlays::SettingsPage::LastFm;
        self.settings_error = None;
        self.settings_open = true;
    }

    pub(super) fn apply_artist_action(&mut self, action: ArtistAction) {
        let artist = match &self.browse.artist {
            Some(Ok(page)) => page.name.clone(),
            _ => self.browse.artist_name.clone(),
        };
        match action {
            ArtistAction::ToggleBookmark => self.toggle_bookmark(),
            ArtistAction::Play(title) => self.find_and_play(&artist, &title, SongIntent::Play),
            ArtistAction::Add(title, pick) => {
                let intent = match pick {
                    SongPick::Like => SongIntent::Like,
                    SongPick::Mix => SongIntent::AddToMix,
                    SongPick::Playlist(target) => SongIntent::AddToPlaylist(target),
                };
                self.find_and_play(&artist, &title, intent)
            }
            ArtistAction::OpenArtist(name) => self.open_artist(&name),
            ArtistAction::OpenUrl(url) => self.open_link(&url),
        }
    }

    pub(super) fn apply_profile_action(&mut self, action: ProfileAction) {
        match action {
            ProfileAction::Period(period) => self.set_profile_period(period),
            ProfileAction::OpenArtist(name) => self.open_artist(&name),
            ProfileAction::Play { artist, title } => {
                self.find_and_play(&artist, &title, SongIntent::Play)
            }
            ProfileAction::OpenUrl(url) => self.open_link(&url),
            ProfileAction::Connect => self.connect_lastfm_from_ui(),
            ProfileAction::AddFriend => {
                let user = self.browse.profile_user.clone();
                self.add_lastfm_friend(&user);
            }
            ProfileAction::RemoveFriend => {
                let user = self.browse.profile_user.clone();
                self.remove_lastfm_friend(&user);
            }
            ProfileAction::CopyFriendCode => self.clipboard = Some(self.my_friend_code()),
        }
    }

    fn open_link(&mut self, url: &str) {
        if let Err(error) = crate::infrastructure::system::open_url(url) {
            self.error = Some(AppError::Ui(error));
        }
    }

    /// Play (or add) a song known by its artist and title, by finding it on
    /// YouTube first.
    pub(super) fn find_and_play(&mut self, artist: &str, title: &str, intent: SongIntent) {
        let query = format!("ytsearch1:{artist} - {title}");
        let (tools, registry) = (self.tools.clone(), self.registry.clone());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let found =
                query_ytdlp_limited(&tools, &query, true, Some(1), &registry).and_then(|tracks| {
                    tracks
                        .into_iter()
                        .next()
                        .ok_or_else(|| "That song wasn't found on YouTube".to_string())
                });
            let _ = sender.send(found);
        });
        self.browse.song_rx = Some((receiver, intent));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bookmarks_toggle_and_keep_similar_artists() {
        let mut app = YtMusicApp::default();
        app.browse.artist_name = "Queen".into();
        app.browse.artist = Some(Ok(ArtistPage {
            name: "Queen".into(),
            listeners: 1,
            plays: 1,
            bio: String::new(),
            tags: vec![],
            similar: vec!["David Bowie".into()],
            top_tracks: vec![],
            url: String::new(),
        }));
        app.toggle_bookmark();
        assert!(app.browse.is_bookmarked("queen"));
        assert_eq!(app.browse.bookmarks[0].similar, ["David Bowie"]);
        app.toggle_bookmark();
        assert!(app.browse.bookmarks.is_empty());
    }

    #[test]
    fn old_artist_searches_dont_replace_newer_ones() {
        let mut app = YtMusicApp::default();
        app.browse.artists_query = "new".into();
        let stale = LastFmEvent::Artists {
            query: "old".into(),
            result: Ok(vec![ArtistHit {
                name: "Old".into(),
                listeners: 1,
            }]),
        };
        assert!(app.receive_browse(stale).is_none());
        assert!(app.browse.artists.is_empty());
    }

    #[test]
    fn artist_photos_are_looked_up_once() {
        let mut app = YtMusicApp::default();
        assert!(app.artist_image("Queen").is_none());
        assert!(app.artist_image("queen").is_none());
        assert_eq!(app.browse.image_queue.len(), 1);
        app.browse.images.insert("queen".into(), Some("abc".into()));
        assert!(app.artist_image("Queen").is_some());
    }
}
