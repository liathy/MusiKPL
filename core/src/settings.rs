//! Settings that persist between runs, stored as JSON in
//! `%APPDATA%\MusiKPL\settings.json`.

use serde::{Deserialize, Serialize};

use crate::storage;

const FILE: &str = "settings.json";
/// Recent searches remembered for the search box.
pub const MAX_RECENT_SEARCHES: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub start_with_windows: bool,
    pub lastfm: LastFmSettings,
    pub volume: u8,
    pub auto_mix: bool,
    /// Newest first.
    pub recent_searches: Vec<String>,
    /// Genres picked on first run, used until listening history takes over.
    pub favorite_genres: Vec<String>,
    /// The first-run genre picker was completed or skipped.
    pub onboarded: bool,
    /// Closing hides MusiKPL to the tray (Windows) instead of quitting.
    pub close_to_tray: bool,
    /// Friends' Jam invites pop up (else they wait in the Friends card).
    pub invite_popups: bool,
    pub shuffle: ShuffleMode,
    pub repeat: RepeatMode,
}

/// The order songs play in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShuffleMode {
    #[default]
    Off,
    /// Random order.
    On,
    /// Random order, with recommended songs that fit mixed in.
    Smart,
}

impl ShuffleMode {
    /// The next mode for the shuffle button: off, on, smart, off.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::On,
            Self::On => Self::Smart,
            Self::Smart => Self::Off,
        }
    }
}

/// What happens when the songs run out, or one ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepeatMode {
    #[default]
    Off,
    /// Start the playlist (or the songs played) over.
    All,
    /// Play the same song again.
    One,
}

impl RepeatMode {
    /// The next mode for the repeat button: off, all, one, off.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            start_with_windows: false,
            lastfm: LastFmSettings::default(),
            volume: 100,
            auto_mix: true,
            recent_searches: Vec::new(),
            favorite_genres: Vec::new(),
            onboarded: false,
            close_to_tray: false,
            invite_popups: true,
            shuffle: ShuffleMode::Off,
            repeat: RepeatMode::Off,
        }
    }
}

/// Last.fm API credentials and the session granted after the user approved
/// MusiKPL in the browser. The session key is what allows scrobbling.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastFmSettings {
    pub api_key: String,
    pub api_secret: String,
    pub username: String,
    pub session_key: String,
}

impl Settings {
    pub fn load() -> Self {
        storage::read(FILE).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        storage::write(FILE, self)
    }

    /// Save without making the interface wait, for frequent small changes.
    pub fn save_in_background(&self) {
        storage::write_in_background(FILE, self);
    }

    /// Remember a search, newest first, without duplicates.
    pub fn remember_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.recent_searches
            .retain(|known| !known.eq_ignore_ascii_case(query));
        self.recent_searches.insert(0, query.to_owned());
        self.recent_searches.truncate(MAX_RECENT_SEARCHES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"lastfm":{"api_key":"k"}}"#).expect("parses");
        assert_eq!(settings.lastfm.api_key, "k");
        assert!(!settings.start_with_windows);
        // Settings saved by older versions keep today's behavior.
        assert_eq!(settings.volume, 100);
        assert!(settings.auto_mix);
        assert!(!settings.onboarded);
    }

    #[test]
    fn recent_searches_are_newest_first_unique_and_bounded() {
        let mut settings = Settings::default();
        for query in ["queen", "daft punk", "Queen", "  "] {
            settings.remember_search(query);
        }
        assert_eq!(settings.recent_searches, ["Queen", "daft punk"]);
        for index in 0..20 {
            settings.remember_search(&format!("search {index}"));
        }
        assert_eq!(settings.recent_searches.len(), MAX_RECENT_SEARCHES);
        assert_eq!(settings.recent_searches[0], "search 19");
    }
}
