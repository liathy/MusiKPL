//! Settings that persist between runs, stored as JSON in
//! `%APPDATA%\MusiKPL\settings.json`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) start_with_windows: bool,
    pub(crate) lastfm: LastFmSettings,
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
        settings_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        let path = settings_path().ok_or("Could not find the AppData folder")?;
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|e| format!("Could not create the settings folder: {e}"))?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Could not encode settings: {e}"))?;
        // Write beside the file and swap it in, so a crash never leaves
        // half-written settings behind.
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, text).map_err(|e| format!("Could not save settings: {e}"))?;
        std::fs::rename(&temporary, &path).map_err(|e| format!("Could not save settings: {e}"))
    }
}

fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
    Some(PathBuf::from(base).join("MusiKPL").join("settings.json"))
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
}
