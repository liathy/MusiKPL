//! Likes and taste that follow the Last.fm account across devices.
//!
//! - Likes are Last.fm loves: songs loved on another device (or on the
//!   Last.fm site, or in another player) are found on YouTube and liked
//!   here; songs unloved elsewhere are unliked here.
//! - The user's most-scrobbled artists count as favorites for Home and
//!   Auto Mix.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    domain::{song_info, Track},
    lastfm::{browse::Loved, lookup_key, BrowseRequest},
    storage,
};

use super::{unix_now, Engine};

const LOVES_FILE: &str = "lastfm-loves.json";
/// How often loves and top artists are read again.
const SYNC_EVERY: Duration = Duration::from_secs(15 * 60);
/// New loves looked up on YouTube per sync (the rest come next time).
const RESOLVE_PER_SYNC: usize = 40;
/// A like changed here isn't undone by Last.fm for this long (it may not
/// show the change yet).
const LOCAL_GRACE: Duration = Duration::from_secs(10 * 60);

/// What's remembered between runs: the loves already handled.
#[derive(Default, Serialize, Deserialize)]
struct Known {
    loves: HashSet<String>,
}

#[derive(Default)]
pub struct LovesSync {
    known: HashSet<String>,
    loaded: bool,
    checked: Option<Instant>,
    /// Loves waiting to be found on YouTube.
    to_resolve: VecDeque<Loved>,
    resolving: Option<(String, Receiver<Option<Track>>)>,
    /// Likes changed here recently, by key.
    local: HashMap<String, Instant>,
}

/// The same song on every device: artist and title, lower-case.
fn key(artist: &str, title: &str) -> String {
    format!(
        "{}\u{1f}{}",
        artist.trim().to_lowercase(),
        title.trim().to_lowercase()
    )
}

fn track_key(track: &Track) -> String {
    let info = song_info(track);
    key(&info.artist, &info.title)
}

impl Engine {
    fn save_loves(&self) {
        storage::write_in_background(
            LOVES_FILE,
            &Known {
                loves: self.loves.known.clone(),
            },
        );
    }

    /// A like changed here (and is being sent to Last.fm).
    pub(super) fn note_local_like(&mut self, track: &Track, liked: bool) {
        let key = track_key(track);
        self.loves.local.insert(key.clone(), Instant::now());
        let changed = if liked {
            self.loves.known.insert(key)
        } else {
            self.loves.known.remove(&key)
        };
        if changed && self.loves.loaded {
            self.save_loves();
        }
    }

    /// Every tick, with Last.fm connected: now and then read loves and top
    /// artists, and find new loves on YouTube one at a time.
    pub fn tick_lastfm_sync(&mut self) {
        self.poll_resolving();
        let user = self.settings.lastfm.username.clone();
        if user.is_empty() || !self.lastfm_connected() {
            return;
        }
        if !self.loves.loaded {
            self.loves.loaded = true;
            if let Some(known) = storage::read::<Known>(LOVES_FILE) {
                self.loves.known.extend(known.loves);
            }
        }
        if self
            .loves
            .checked
            .is_some_and(|at| at.elapsed() < SYNC_EVERY)
        {
            return;
        }
        let Some(api_key) = lookup_key(&self.settings.lastfm.api_key) else {
            return;
        };
        self.loves.checked = Some(Instant::now());
        self.lastfm.browse(BrowseRequest::Loved {
            api_key: api_key.clone(),
            user: user.clone(),
        });
        self.lastfm
            .browse(BrowseRequest::TopArtists { api_key, user });
    }

    /// The user's loves arrived.
    pub fn receive_loves(&mut self, loves: Vec<Loved>) {
        let remote: HashSet<String> = loves
            .iter()
            .map(|love| key(&love.artist, &love.title))
            .collect();
        let liked_here: HashMap<String, Track> = self
            .library
            .taste
            .liked(usize::MAX)
            .into_iter()
            .map(|track| (track_key(&track), track))
            .collect();
        let recent = |key: &str| {
            self.loves
                .local
                .get(key)
                .is_some_and(|at| at.elapsed() < LOCAL_GRACE)
        };

        // Unloved elsewhere: unlike here (unless just changed here).
        let gone: Vec<String> = self
            .loves
            .known
            .iter()
            .filter(|key| !remote.contains(*key) && !recent(key))
            .cloned()
            .collect();
        let mut changed = false;
        for key in gone {
            self.loves.known.remove(&key);
            if let Some(track) = liked_here.get(&key) {
                changed |= self.library.taste.set_like(track, false, unix_now());
            }
        }

        // Loved elsewhere: like here, finding the song first if needed.
        self.loves.to_resolve.clear();
        for love in loves {
            let love_key = key(&love.artist, &love.title);
            if self.loves.known.contains(&love_key) || recent(&love_key) {
                continue;
            }
            if liked_here.contains_key(&love_key) {
                self.loves.known.insert(love_key);
            } else if self.loves.to_resolve.len() < RESOLVE_PER_SYNC {
                self.loves.to_resolve.push_back(love);
            }
        }
        if changed {
            self.library.taste_changed();
        }
        self.save_loves();
        self.resolve_next_love();
    }

    /// The user's top artists on Last.fm arrived.
    pub fn receive_top_artists(&mut self, artists: Vec<String>) {
        if self.library.taste.set_scrobbled_artists(artists) {
            self.library.taste_changed();
        }
    }

    fn resolve_next_love(&mut self) {
        if self.loves.resolving.is_some() {
            return;
        }
        let Some(love) = self.loves.to_resolve.pop_front() else {
            return;
        };
        let extractor = self.extractor.clone();
        let waker = self.waker.clone();
        let (sender, receiver) = mpsc::channel();
        let source = format!("ytsearch1:{} - {}", love.artist, love.title);
        thread::spawn(move || {
            let found = extractor
                .list(&source, Some(1))
                .ok()
                .and_then(|tracks| tracks.into_iter().next());
            let _ = sender.send(found);
            waker();
        });
        self.loves.resolving = Some((key(&love.artist, &love.title), receiver));
    }

    fn poll_resolving(&mut self) {
        let Some((love_key, receiver)) = &self.loves.resolving else {
            return;
        };
        let found = match receiver.try_recv() {
            Ok(found) => found,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => None,
        };
        let love_key = love_key.clone();
        self.loves.resolving = None;
        // Not found on YouTube: not tried again.
        self.loves.known.insert(love_key.clone());
        if let Some(track) = found {
            let recent = self
                .loves
                .local
                .get(&love_key)
                .is_some_and(|at| at.elapsed() < LOCAL_GRACE);
            if !recent && self.library.taste.set_like(&track, true, unix_now()) {
                self.library.taste_changed();
            }
        }
        self.save_loves();
        self.resolve_next_love();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn love(artist: &str, title: &str) -> Loved {
        Loved {
            artist: artist.into(),
            title: title.into(),
        }
    }

    fn song(id: &str, artist: &str, title: &str) -> Track {
        Track {
            title: title.into(),
            id: id.into(),
            duration: None,
            channel: format!("{artist} - Topic"),
        }
    }

    #[test]
    fn unloved_elsewhere_is_unliked_here_but_fresh_local_changes_stay() {
        let mut app = Engine::default();
        let queen = song("q", "Queen", "Bohemian Rhapsody");
        let muse = song("m", "Muse", "Uprising");
        app.library.taste.set_like(&queen, true, 1);
        app.library.taste.set_like(&muse, true, 1);
        // Both known as loved (from an earlier sync).
        app.loves.known.insert(track_key(&queen));
        app.loves.known.insert(track_key(&muse));
        // Muse was just liked here again, Last.fm may not show it yet.
        app.loves.local.insert(track_key(&muse), Instant::now());

        app.receive_loves(vec![love("Abba", "Waterloo")]);
        assert!(!app.is_liked("q"), "unloved on another device");
        assert!(app.is_liked("m"), "liked here a moment ago");
        // The new love waits to be found on YouTube.
        assert!(app.loves.resolving.is_some());
    }

    #[test]
    fn loves_already_liked_here_are_just_remembered() {
        let mut app = Engine::default();
        let queen = song("q", "Queen", "Bohemian Rhapsody");
        app.library.taste.set_like(&queen, true, 1);
        app.receive_loves(vec![love("queen", "bohemian rhapsody")]);
        assert!(app.loves.known.contains(&track_key(&queen)));
        assert!(app.loves.resolving.is_none());
    }
}
