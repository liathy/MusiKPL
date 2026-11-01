//! Playlists: creating, opening, editing, and playing them.

use std::sync::mpsc::{self, TryRecvError};

use crate::{
    domain::{Cover, Playlist, Track},
    infrastructure::images,
    ui::{menu::PlaylistTarget, playlist::PlaylistAction},
};

use super::{View, YtMusicApp};

/// A photo being chosen for a playlist: its id, then the saved file name,
/// None if the dialog was cancelled, or why it couldn't be used.
pub(super) type CoverPick = mpsc::Receiver<(u64, Option<Result<String, String>>)>;

/// A playlist's cover photo, when the user chose one; otherwise the screens
/// show the default playlist icon.
pub(super) fn playlist_cover(playlist: &Playlist) -> Option<Cover> {
    let uri = images::cover_uri(playlist.cover.as_deref()?)?;
    Some(Cover {
        url: uri,
        uv: [0.0, 0.0, 1.0, 1.0],
    })
}

impl YtMusicApp {
    /// Open the file dialog (on its own thread, since it waits for the user)
    /// and save the chosen photo as the playlist's cover.
    fn pick_playlist_cover(&mut self, id: u64) {
        if self.cover_pick.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let saved = images::pick_image()
                .map(|path| images::save_cover(&path, &format!("playlist-{id}")));
            let _ = sender.send((id, saved));
        });
        self.cover_pick = Some(receiver);
    }

    pub(super) fn poll_cover_pick(&mut self) {
        let Some(receiver) = &self.cover_pick else {
            return;
        };
        let (id, saved) = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.cover_pick = None;
                return;
            }
        };
        self.cover_pick = None;
        match saved {
            None => {}
            Some(Ok(name)) => {
                if self.library.playlists.get(id).is_none() {
                    // Deleted while the dialog was open.
                    images::delete_cover(&name);
                    return;
                }
                if let Some(old) = self.library.playlists.set_cover(id, Some(name)) {
                    self.forget_cover(&old);
                }
                self.library.playlists_changed = true;
            }
            Some(Err(error)) => self.error = Some(super::errors::AppError::Ui(error)),
        }
    }

    /// Delete a replaced cover photo and drop it from the image cache.
    fn forget_cover(&mut self, file_name: &str) {
        if let Some(uri) = images::cover_uri(file_name) {
            self.stale_images.push(uri);
        }
        images::delete_cover(file_name);
    }

    /// The playlist whose screen is open.
    pub(super) fn open_playlist_id(&self) -> Option<u64> {
        (self.view == View::Playlist)
            .then_some(self.open_playlist)
            .flatten()
    }

    pub(super) fn open_playlist(&mut self, id: u64) {
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
    pub(super) fn new_playlist(&mut self) -> u64 {
        let id = self.library.playlists.create();
        self.library.playlists_changed = true;
        self.open_playlist(id);
        self.start_editing_playlist();
        id
    }

    fn start_editing_playlist(&mut self) {
        if let Some(playlist) = self
            .open_playlist
            .and_then(|id| self.library.playlists.get(id))
        {
            self.playlist_name_draft.clone_from(&playlist.name);
            self.playlist_editing = true;
        }
    }

    pub(super) fn delete_playlist(&mut self, id: u64) {
        if let Some(playlist) = self.library.playlists.delete(id) {
            self.library.playlists_changed = true;
            if let Some(cover) = playlist.cover {
                images::delete_cover(&cover);
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

    pub(super) fn add_to_playlist(&mut self, track: Track, target: PlaylistTarget) {
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
        }
    }

    /// Play the open playlist from `index`: the rest of it becomes the
    /// queue, replacing what was queued.
    fn play_playlist_from(&mut self, id: u64, index: usize) {
        if self.is_guest() {
            return;
        }
        let Some(playlist) = self.library.playlists.get(id) else {
            return;
        };
        let Some(first) = playlist.tracks.get(index).cloned() else {
            return;
        };
        let rest: Vec<Track> = playlist.tracks[index + 1..].to_vec();
        self.reset_mix();
        self.queue.retain(|_| false);
        for track in rest {
            self.queue.enqueue_unique(track);
        }
        self.failed_plays = 0;
        self.start_track(first, true);
    }

    pub(super) fn apply_playlist_action(&mut self, action: PlaylistAction) {
        let Some(id) = self.open_playlist_id() else {
            return;
        };
        let changed = match action {
            PlaylistAction::PlayAll => {
                self.play_playlist_from(id, 0);
                false
            }
            PlaylistAction::PlayFrom(index) => {
                self.play_playlist_from(id, index);
                false
            }
            PlaylistAction::ToggleFavorite => self.library.playlists.toggle_favorite(id),
            PlaylistAction::Edit => {
                self.start_editing_playlist();
                false
            }
            PlaylistAction::Done => {
                self.playlist_editing = false;
                self.library.playlists.rename(id, &self.playlist_name_draft)
            }
            PlaylistAction::Remove(index) => self.library.playlists.remove(id, index),
            PlaylistAction::Move { from, to } => self.library.playlists.reorder(id, from, to),
            PlaylistAction::ToggleLike(track) => {
                self.toggle_like(&track);
                false
            }
            PlaylistAction::AddSuggestion(track) => self.library.playlists.add(id, track),
            PlaylistAction::PlaySuggestion(track) => {
                if self.is_guest() {
                    self.request_from_host(&track);
                } else {
                    self.play_search_track(track);
                }
                false
            }
            PlaylistAction::MoreSuggestions => {
                self.more_playlist_recs();
                false
            }
            PlaylistAction::ChangeCover => {
                self.pick_playlist_cover(id);
                false
            }
            PlaylistAction::RemoveCover => {
                if let Some(old) = self.library.playlists.set_cover(id, None) {
                    self.forget_cover(&old);
                }
                true
            }
        };
        if changed {
            self.library.playlists_changed = true;
        }
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

    fn offline_app() -> YtMusicApp {
        let mut app = YtMusicApp::default();
        app.tools.yt_dlp = "missing-yt-dlp-for-tests".into();
        app.tools.mpv = "missing-mpv-for-tests".into();
        app
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
        app.apply_playlist_action(PlaylistAction::Done);
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
        app.apply_playlist_action(PlaylistAction::PlayFrom(1));
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
