//! Songs to add to the open playlist, found from the songs already in it:
//! YouTube Mixes of its songs and searches for its artists, a page at a
//! time as the user scrolls.

use std::{
    collections::HashSet,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use crate::{
    domain::{song_info, youtube_mix_url, Track},
    recommendations::{home::usable_songs, rng::Rng},
};

use super::{library::unix_now, Engine};

/// Suggestions shown per page.
pub const RECS_PAGE: usize = 10;

#[derive(Default)]
pub struct PlaylistRecs {
    /// The playlist these are for.
    playlist: Option<u64>,
    tracks: Vec<Track>,
    shown: usize,
    /// Searches already used.
    used: HashSet<String>,
    rx: Option<Receiver<Vec<Track>>>,
    /// Every search has been used.
    exhausted: bool,
}

impl PlaylistRecs {
    pub fn loading(&self) -> bool {
        self.rx.is_some()
    }
}

impl Engine {
    /// Suggestions to show for the open playlist, without songs added since.
    pub fn playlist_recs(&self) -> Vec<&Track> {
        let Some(id) = self.open_playlist_id() else {
            return Vec::new();
        };
        let recs = &self.playlist_recs;
        if recs.playlist != Some(id) {
            return Vec::new();
        }
        recs.tracks
            .iter()
            .filter(|track| !self.library.playlists.contains(id, &track.id))
            .take(recs.shown)
            .collect()
    }

    /// The user scrolled to the end of the suggestions.
    pub fn more_playlist_recs(&mut self) {
        let recs = &mut self.playlist_recs;
        if recs.tracks.len() > recs.shown {
            recs.shown += RECS_PAGE;
        } else if !recs.exhausted {
            // Show the next page as soon as it arrives.
            recs.shown = recs.tracks.len() + RECS_PAGE;
        }
    }

    /// Called every frame: keep enough suggestions ready for the open
    /// playlist, and collect finished searches.
    pub fn keep_playlist_recs(&mut self) {
        self.poll_playlist_recs();
        let Some(id) = self.open_playlist_id() else {
            return;
        };
        let Some(playlist) = self.library.playlists.get(id) else {
            return;
        };
        if self.playlist_recs.playlist != Some(id) {
            self.playlist_recs = PlaylistRecs {
                playlist: Some(id),
                shown: RECS_PAGE,
                ..PlaylistRecs::default()
            };
        }
        let recs = &self.playlist_recs;
        let available = recs
            .tracks
            .iter()
            .filter(|track| !playlist.tracks.iter().any(|t| t.id == track.id))
            .count();
        // Keep one page beyond what is shown ready, so scrolling never waits.
        if playlist.tracks.is_empty()
            || recs.rx.is_some()
            || recs.exhausted
            || available >= recs.shown + RECS_PAGE
        {
            return;
        }
        // The next search: a Mix of one of its songs or a search for one of
        // its artists, at random among those not used yet.
        let mut sources: Vec<String> = playlist.tracks.iter().map(youtube_mix_url).collect();
        let mut artists = HashSet::new();
        for track in &playlist.tracks {
            let artist = song_info(track).artist;
            if !artist.is_empty() && artists.insert(artist.to_lowercase()) {
                sources.push(format!("ytsearch20:{artist} songs"));
            }
        }
        sources.retain(|source| !recs.used.contains(source));
        let Some(source) = Rng::from_time().pick(&sources).cloned() else {
            self.playlist_recs.exhausted = true;
            return;
        };
        self.playlist_recs.used.insert(source.clone());
        let extractor = self.extractor.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(extractor.list(&source, Some(30)).unwrap_or_default());
        });
        self.playlist_recs.rx = Some(receiver);
    }

    fn poll_playlist_recs(&mut self) {
        let Some(receiver) = &self.playlist_recs.rx else {
            return;
        };
        let found = match receiver.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        self.playlist_recs.rx = None;
        let Some(playlist) = self
            .playlist_recs
            .playlist
            .and_then(|id| self.library.playlists.get(id))
        else {
            return;
        };
        let exclude: HashSet<String> = playlist
            .tracks
            .iter()
            .chain(self.playlist_recs.tracks.iter())
            .map(|track| track.id.clone())
            .collect();
        let mut new = usable_songs(found, false, &self.library.taste, unix_now(), &exclude);
        Rng::from_time().shuffle(&mut new);
        self.playlist_recs.tracks.extend(new);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::PlaylistTarget;

    fn track(id: &str, artist: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(200.0),
            channel: artist.into(),
        }
    }

    #[test]
    fn suggestions_skip_songs_in_the_playlist_and_page() {
        let mut app = Engine::default();
        let id = app.new_playlist();
        app.add_to_playlist(track("in", "A"), PlaylistTarget::Existing(id));
        app.keep_playlist_recs();
        assert!(app.playlist_recs.loading(), "a search starts");

        let (sender, receiver) = mpsc::channel();
        app.playlist_recs.rx = Some(receiver);
        let found: Vec<Track> = (0..15)
            .map(|i| track(&format!("r{i}"), "B"))
            .chain([track("in", "A")])
            .collect();
        sender.send(found).unwrap();
        app.poll_playlist_recs();
        let shown = app.playlist_recs();
        assert_eq!(shown.len(), RECS_PAGE);
        assert!(shown.iter().all(|t| t.id != "in"));

        app.more_playlist_recs();
        assert_eq!(app.playlist_recs().len(), 15);

        // Added songs drop out of the suggestions.
        app.add_to_playlist(track("r0", "B"), PlaylistTarget::Existing(id));
        assert!(app.playlist_recs().iter().all(|t| t.id != "r0"));
    }
}
