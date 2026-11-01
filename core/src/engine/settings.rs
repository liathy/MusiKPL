//! Settings, connecting Last.fm, and scrobbling what plays.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::{
    domain::song_info,
    lastfm::{app_keys, Credentials, LastFmEvent, LastFmJob, ScrobbleStep},
    settings::Settings,
};

use super::{errors::AppError, Engine};

/// Progress of connecting a Last.fm account in the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LastFmConnect {
    Idle,
    Starting,
    AwaitingApproval(String),
}

impl Engine {
    pub fn load_settings(&mut self) {
        self.settings = Settings::load();
    }

    pub fn save_settings(&mut self) {
        if let Err(error) = self.settings.save() {
            self.settings_error = Some(error);
        }
    }

    /// What scrobbling and loving need: a session, plus the user's own key
    /// pair or MusiKPL's built-in one.
    pub fn credentials(&self) -> Option<Credentials> {
        let lastfm = &self.settings.lastfm;
        if lastfm.session_key.is_empty() {
            return None;
        }
        let (api_key, api_secret) = app_keys(&lastfm.api_key, &lastfm.api_secret)?;
        Some(Credentials {
            api_key,
            api_secret,
            session_key: lastfm.session_key.clone(),
        })
    }

    pub fn lastfm_connected(&self) -> bool {
        self.credentials().is_some() && !self.settings.lastfm.username.is_empty()
    }

    pub fn poll_lastfm(&mut self) {
        for event in self.lastfm.poll() {
            let Some(event) = self.receive_browse(event) else {
                continue;
            };
            match event {
                LastFmEvent::Tags { video_id, tags } => self.receive_tags(&video_id, tags),
                LastFmEvent::Failed(message) => {
                    let error = AppError::LastFm(message);
                    if self.error.as_ref() != Some(&error) {
                        self.error = Some(error);
                    }
                }
                LastFmEvent::AwaitingApproval { url } => {
                    if self.lastfm_connect != LastFmConnect::Idle {
                        self.lastfm_connect = LastFmConnect::AwaitingApproval(url);
                    }
                }
                LastFmEvent::Connected {
                    username,
                    session_key,
                } => {
                    self.lastfm_connect = LastFmConnect::Idle;
                    self.settings.lastfm.username = username;
                    self.settings.lastfm.session_key = session_key;
                    self.settings_error = None;
                    self.save_settings();
                    self.prefetch_profile();
                }
                LastFmEvent::ConnectFailed(message) => {
                    self.lastfm_connect = LastFmConnect::Idle;
                    self.settings_error = Some(message);
                }
                // Handled by receive_browse.
                LastFmEvent::Artists { .. }
                | LastFmEvent::Artist { .. }
                | LastFmEvent::People { .. }
                | LastFmEvent::Tracks { .. }
                | LastFmEvent::Following { .. }
                | LastFmEvent::Loved { .. }
                | LastFmEvent::TopArtists { .. }
                | LastFmEvent::Profile { .. } => {}
            }
        }
    }

    /// Tell Last.fm what is playing, and scrobble it once heard long enough.
    pub fn update_scrobbling(&mut self) {
        let Some(credentials) = self.credentials() else {
            return;
        };
        let track = if self.loading {
            None
        } else {
            self.current.clone()
        };
        let playing = !self.player.paused && self.player.has_started();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |time| time.as_secs_f64());
        let step = self.scrobbler.update(
            track.as_ref().map(|track| track.id.as_str()),
            self.player.position_seconds(),
            playing,
            track.as_ref().and_then(|track| track.duration),
            now,
        );
        let Some(track) = track else {
            return;
        };
        match step {
            ScrobbleStep::Nothing => {}
            ScrobbleStep::NowPlaying => self.lastfm.submit(LastFmJob::NowPlaying {
                credentials,
                song: song_info(&track),
                duration: track.duration,
            }),
            ScrobbleStep::Scrobble { started_at } => self.lastfm.submit(LastFmJob::Scrobble {
                credentials,
                song: song_info(&track),
                started_at,
                duration: track.duration,
            }),
        }
    }
}

impl Engine {
    /// Connect a Last.fm account: with the user's own key and secret when
    /// given, else MusiKPL's built-in pair. The approval page opens in the
    /// browser; [`LastFmConnect`] follows the progress.
    pub fn connect_lastfm(&mut self, api_key: &str, api_secret: &str) {
        let (api_key, api_secret) = (api_key.trim().to_owned(), api_secret.trim().to_owned());
        let Some((key, secret)) = app_keys(&api_key, &api_secret) else {
            self.settings_error = Some("Enter your API key and shared secret first".into());
            return;
        };
        self.settings.lastfm.api_key = api_key;
        self.settings.lastfm.api_secret = api_secret;
        self.settings.lastfm.session_key.clear();
        self.settings.lastfm.username.clear();
        self.settings_error = None;
        self.save_settings();
        self.lastfm_connect = LastFmConnect::Starting;
        self.lastfm.connect(key, secret);
    }

    pub fn cancel_lastfm_connect(&mut self) {
        self.lastfm.cancel_connect();
        self.lastfm_connect = LastFmConnect::Idle;
    }

    pub fn disconnect_lastfm(&mut self) {
        self.settings.lastfm.session_key.clear();
        self.settings.lastfm.username.clear();
        self.browse.profile = None;
        self.browse.my_avatar = None;
        self.browse.my_scrobbles = None;
        self.settings_error = None;
        self.save_settings();
    }

    pub fn set_invite_popups(&mut self, enabled: bool) {
        self.settings.invite_popups = enabled;
        self.save_settings();
    }

    /// Forget everything listened to and liked (Settings).
    pub fn clear_history(&mut self) {
        self.clear_taste();
        self.settings_error = None;
    }
}
