//! Playlists: creating, opening, editing, and playing them.

use crate::{
    covers,
    domain::{Cover, Playlist, PlaylistTarget, Track},
};

use super::{Engine, View};

/// A playlist's cover photo, when the user chose one; otherwise the screens
/// show the default playlist icon.
pub fn playlist_cover(playlist: &Playlist) -> Option<Cover> {
    let uri = covers::cover_uri(playlist.cover.as_deref()?)?;
    Some(Cover {
        url: uri,
        uv: [0.0, 0.0, 1.0, 1.0],
    })
}

impl Engine {
    /// Delete a replaced cover photo and drop it from the image cache.
    pub fn forget_cover(&mut self, file_name: &str) {
        if let Some(uri) = covers::cover_uri(file_name) {
            self.stale_images.push(uri);
        }
        covers::delete_cover(file_name);
    }

    /// The playlist whose screen is open.
    pub fn open_playlist_id(&self) -> Option<u64> {
        (self.view == View::Playlist)
            .then_some(self.open_playlist)
            .flatten()
    }

    pub fn open_playlist(&mut self, id: u64) {
        if self.library.playlists.get(id).is_none() {
            return;
        }
        if self.open_playlist != Some(id) {
            self.playlist_editing = false;
        }
        self.open_playlist = Some(id);
        self.view = View::Playlist;
    }

    /// Make a new playlist and open it with its name ready to edit.
    pub fn new_playlist(&mut self) -> u64 {
        let id = self.library.playlists.create();
        self.library.playlists_changed = true;
        self.open_playlist(id);
        self.start_editing_playlist();
        id
    }

    pub fn start_editing_playlist(&mut self) {
        if let Some(playlist) = self
            .open_playlist
            .and_then(|id| self.library.playlists.get(id))
        {
            self.playlist_name_draft.clone_from(&playlist.name);
            self.playlist_editing = true;
        }
    }

    pub fn delete_playlist(&mut self, id: u64) {
        if let Some(playlist) = self.library.playlists.delete(id) {
            self.library.playlists_changed = true;
            if let Some(cover) = playlist.cover {
                covers::delete_cover(&cover);
            }
        }
        if self.open_playlist == Some(id) {
            self.open_playlist = None;
            self.playlist_editing = false;
            if self.view == View::Playlist {
                self.view = View::Home;
            }
        }
    }

    pub fn add_to_playlist(&mut self, track: Track, target: PlaylistTarget) {
        match target {
            PlaylistTarget::Existing(id) => {
                if self.library.playlists.add(id, track) {
                    self.library.playlists_changed = true;
                }
            }
            PlaylistTarget::New => {
                let id = self.new_playlist();
                self.library.playlists.add(id, track);
            }
            PlaylistTarget::Download => self.toggle_download(&track),
        }
    }

    /// Play the open playlist from `index`: the rest of it becomes the
    /// queue, replacing what was queued.
    pub fn play_playlist_from(&mut self, id: u64, index: usize) {
        if self.is_guest() {
            return;
        }
        let Some(playlist) = self.library.playlists.get(id) else {
            return;
        };
        let Some(first) = playlist.tracks.get(index).cloned() else {
            return;
        };
        let all = playlist.tracks.clone();
        let shuffled = self.settings.shuffle != crate::settings::ShuffleMode::Off;
        // Shuffled: every other song, in a random order; else the rest of
        // the playlist after this one.
        let mut rest: Vec<Track> = if shuffled {
            all.iter()
                .filter(|track| track.id != first.id)
                .cloned()
                .collect()
        } else {
            all[index + 1..].to_vec()
        };
        if shuffled {
            crate::recommendations::rng::Rng::from_time().shuffle(&mut rest);
        }
        self.reset_mix();
        self.queue.retain(|_| false);
        for track in rest {
            self.queue.enqueue_unique(track);
        }
        self.start_playlist_context(all);
        self.order.playlist = Some(id);
        self.failed_plays = 0;
        self.start_track(first, true);
    }

    /// Play a playlist from the start, or from a random song with shuffle on.
    pub fn play_playlist(&mut self, id: u64) {
        let Some(playlist) = self.library.playlists.get(id) else {
            return;
        };
        let index = if self.settings.shuffle == crate::settings::ShuffleMode::Off {
            0
        } else {
            crate::recommendations::rng::Rng::from_time().below(playlist.tracks.len().max(1))
        };
        self.play_playlist_from(id, index);
    }

    /// The name typed for the open playlist is done: keep it.
    pub fn finish_editing_playlist(&mut self) {
        self.playlist_editing = false;
        if let Some(id) = self.open_playlist_id() {
            if self.library.playlists.rename(id, &self.playlist_name_draft) {
                self.library.playlists_changed = true;
            }
        }
    }

    /// A photo the user chose for a playlist was saved as `name` (see
    /// [`crate::covers::save_cover`]); it becomes the cover.
    pub fn set_playlist_cover(&mut self, id: u64, name: String) {
        if self.library.playlists.get(id).is_none() {
            // Deleted while the photo was being chosen.
            covers::delete_cover(&name);
            return;
        }
        if let Some(old) = self.library.playlists.set_cover(id, Some(name)) {
            self.forget_cover(&old);
        }
        self.library.playlists_changed = true;
    }

    pub fn remove_playlist_cover(&mut self, id: u64) {
        if let Some(old) = self.library.playlists.set_cover(id, None) {
            self.forget_cover(&old);
        }
        self.library.playlists_changed = true;
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(180.0),
            channel: "Artist".into(),
        }
    }

    fn offline_app() -> Engine {
        Engine::default()
    }

    #[test]
    fn a_new_playlist_from_a_song_opens_ready_to_rename() {
        let mut app = offline_app();
        app.add_to_playlist(track("a"), PlaylistTarget::New);
        let id = app.open_playlist_id().expect("the new playlist is open");
        assert!(app.playlist_editing);
        assert_eq!(app.playlist_name_draft, "Playlist 1");
        assert!(app.library.playlists.contains(id, "a"));

        app.playlist_name_draft = "Hardstyle".into();
        app.finish_editing_playlist();
        assert!(!app.playlist_editing);
        assert_eq!(app.library.playlists.get(id).unwrap().name, "Hardstyle");
        assert!(app.library.playlists_changed);
    }

    #[test]
    fn playing_a_playlist_queues_the_rest_of_it() {
        let mut app = offline_app();
        let id = app.new_playlist();
        for song in ["a", "b", "c"] {
            app.add_to_playlist(track(song), PlaylistTarget::Existing(id));
        }
        app.queue.push_back(track("old"));
        app.play_playlist_from(id, 1);
        assert_eq!(app.pending_track.as_ref().map(|t| t.id.as_str()), Some("b"));
        let queued: Vec<_> = app.queue.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(queued, ["c"]);
    }

    #[test]
    fn deleting_the_open_playlist_goes_home() {
        let mut app = offline_app();
        let id = app.new_playlist();
        app.delete_playlist(id);
        assert_eq!(app.view, View::Home);
        assert!(app.library.playlists.get(id).is_none());
    }
}
