//! Settings that persist between runs, stored as JSON in
//! `%APPDATA%\MusiKPL\settings.json`.

use serde::{Deserialize, Serialize};

use super::storage;

const FILE: &str = "settings.json";
/// Recent searches remembered for the search box.
pub(crate) const MAX_RECENT_SEARCHES: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) start_with_windows: bool,
    pub(crate) lastfm: LastFmSettings,
    pub(crate) volume: u8,
    pub(crate) auto_mix: bool,
    /// Newest first.
    pub(crate) recent_searches: Vec<String>,
    /// Genres picked on first run, used until listening history takes over.
    pub(crate) favorite_genres: Vec<String>,
    /// The first-run genre picker was completed or skipped.
    pub(crate) onboarded: bool,
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
        }
    }
}

/// Last.fm API credentials and the session granted after the user approved
/// MusiKPL in the browser. The session key is what allows scrobbling.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LastFmSettings {
    pub(crate) api_key: String,
    pub(crate) api_secret: String,
    pub(crate) username: String,
    pub(crate) session_key: String,
}

impl LastFmSettings {
    pub(crate) fn connected(&self) -> bool {
        !self.api_key.is_empty() && !self.api_secret.is_empty() && !self.session_key.is_empty()
    }
}

impl Settings {
    pub(crate) fn load() -> Self {
        storage::read(FILE).unwrap_or_default()
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        storage::write(FILE, self)
    }

    /// Save without making the interface wait, for frequent small changes.
    pub(crate) fn save_in_background(&self) {
        storage::write_in_background(FILE, self);
    }

    /// Remember a search, newest first, without duplicates.
    pub(crate) fn remember_search(&mut self, query: &str) {
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
        assert!(!settings.lastfm.connected());
        // Settings saved by older versions keep today's behavior.
        assert_eq!(settings.volume, 100);
        assert!(settings.auto_mix);
        assert!(!settings.onboarded);
    }

    #[test]
    fn a_session_needs_key_secret_and_session() {
        let mut lastfm = LastFmSettings {
            api_key: "k".into(),
            api_secret: "s".into(),
            ..LastFmSettings::default()
        };
        assert!(!lastfm.connected());
        lastfm.session_key = "sk".into();
        assert!(lastfm.connected());
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
