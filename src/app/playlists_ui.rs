//! Playlist screen buttons, and choosing a cover photo with the file dialog.

use std::sync::mpsc::{self, TryRecvError};

use musikpl_core::{covers, engine::AppError};

use crate::{infrastructure::images, ui::playlist::PlaylistAction};

use super::YtMusicApp;

/// A photo being chosen for a playlist: its id, then the saved file name,
/// None if the dialog was cancelled, or why it couldn't be used.
pub(super) type CoverPick = mpsc::Receiver<(u64, Option<Result<String, String>>)>;

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
                .map(|path| covers::save_cover(&path, &format!("playlist-{id}")));
            let _ = sender.send((id, saved));
        });
        self.cover_pick = Some(receiver);
    }

    pub fn poll_cover_pick(&mut self) {
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
            Some(Ok(name)) => self.engine.set_playlist_cover(id, name),
            Some(Err(error)) => self.engine.error = Some(AppError::Ui(error)),
        }
    }

    pub fn apply_playlist_action(&mut self, action: PlaylistAction) {
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
                self.engine.finish_editing_playlist();
                false
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
                self.engine.remove_playlist_cover(id);
                false
            }
        };
        if changed {
            self.library.playlists_changed = true;
        }
    }
}
