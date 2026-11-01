//! The artist and profile pages' buttons, turned into engine calls.

use musikpl_core::engine::SongIntent;

use crate::ui::{
    artist::{ArtistAction, SongPick},
    overlays::SettingsPage,
    profile::ProfileAction,
};

use super::YtMusicApp;

impl YtMusicApp {
    /// Open Settings at the Last.fm page, where the account is connected.
    pub fn connect_lastfm_from_ui(&mut self) {
        self.settings_page = SettingsPage::LastFm;
        self.settings_error = None;
        self.settings_open = true;
    }

    pub fn apply_artist_action(&mut self, action: ArtistAction) {
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

    pub fn apply_profile_action(&mut self, action: ProfileAction) {
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
}
