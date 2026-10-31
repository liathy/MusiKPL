//! Application side of the settings screen: Start with Windows, connecting
//! Last.fm, and scrobbling what plays.

use std::time::{SystemTime, UNIX_EPOCH};

use eframe::egui;

use crate::{
    domain::song_info,
    infrastructure::{settings::Settings, system},
    lastfm::{Credentials, LastFmEvent, LastFmJob, ScrobbleStep},
    ui::{
        icons::IconTextures,
        overlays::{self, LastFmState, SettingsAction, SettingsView},
        responsive::ResponsiveUi,
    },
};

use super::{errors::AppError, Overlay, YtMusicApp};

/// Progress of connecting a Last.fm account in the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum LastFmConnect {
    Idle,
    Starting,
    AwaitingApproval(String),
}

impl YtMusicApp {
    pub(super) fn load_settings(&mut self) {
        self.settings = Settings::load();
        // The registry is the truth; the user may have changed it elsewhere.
        self.settings.start_with_windows = system::starts_with_windows();
        self.lastfm_key_draft = self.settings.lastfm.api_key.clone();
        self.lastfm_secret_draft = self.settings.lastfm.api_secret.clone();
    }

    fn save_settings(&mut self) {
        if let Err(error) = self.settings.save() {
            self.settings_error = Some(error);
        }
    }

    fn credentials(&self) -> Option<Credentials> {
        let lastfm = &self.settings.lastfm;
        lastfm.connected().then(|| Credentials {
            api_key: lastfm.api_key.clone(),
            api_secret: lastfm.api_secret.clone(),
            session_key: lastfm.session_key.clone(),
        })
    }

    pub(super) fn poll_lastfm(&mut self) {
        for event in self.lastfm.poll() {
            match event {
                LastFmEvent::AlbumArt { video_id, url } => self.cover_art.insert(video_id, url),
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
                }
                LastFmEvent::ConnectFailed(message) => {
                    self.lastfm_connect = LastFmConnect::Idle;
                    self.settings_error = Some(message);
                }
            }
        }
    }

    /// Tell Last.fm what is playing, and scrobble it once heard long enough.
    pub(super) fn update_scrobbling(&mut self) {
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

    pub(super) fn render_settings(
        &mut self,
        ctx: &egui::Context,
        window: egui::Rect,
        open: f32,
        icons: &IconTextures,
        r: &ResponsiveUi,
    ) {
        let lastfm = if self.settings.lastfm.connected() {
            LastFmState::Connected {
                username: &self.settings.lastfm.username,
            }
        } else {
            match &self.lastfm_connect {
                LastFmConnect::Idle => LastFmState::Disconnected,
                LastFmConnect::Starting => LastFmState::Starting,
                LastFmConnect::AwaitingApproval(url) => LastFmState::AwaitingApproval { url },
            }
        };
        let view = SettingsView {
            page: self.settings_page,
            start_with_windows: self.settings.start_with_windows,
            lastfm,
            error: self.settings_error.as_deref(),
        };
        let actions = overlays::show_settings(
            ctx,
            window,
            open,
            icons,
            &view,
            &mut self.lastfm_key_draft,
            &mut self.lastfm_secret_draft,
            r,
        );
        for action in actions {
            self.apply_settings_action(action);
        }
    }

    fn apply_settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::Close => {
                if self.overlay == Overlay::Settings {
                    self.overlay = Overlay::None;
                }
            }
            SettingsAction::Page(page) => {
                self.settings_page = page;
                self.settings_error = None;
            }
            SettingsAction::SetStartWithWindows(enabled) => {
                match system::set_start_with_windows(enabled) {
                    Ok(()) => {
                        self.settings.start_with_windows = enabled;
                        self.settings_error = None;
                        self.save_settings();
                    }
                    Err(error) => self.settings_error = Some(error),
                }
            }
            SettingsAction::LastFmConnect => {
                let api_key = self.lastfm_key_draft.trim().to_owned();
                let api_secret = self.lastfm_secret_draft.trim().to_owned();
                self.lastfm_key_draft.clone_from(&api_key);
                self.lastfm_secret_draft.clone_from(&api_secret);
                // A new key may know different songs; look covers up again.
                if api_key != self.settings.lastfm.api_key {
                    self.cover_art = Default::default();
                }
                self.settings.lastfm.api_key.clone_from(&api_key);
                self.settings.lastfm.api_secret.clone_from(&api_secret);
                self.settings.lastfm.session_key.clear();
                self.settings.lastfm.username.clear();
                self.settings_error = None;
                self.save_settings();
                self.lastfm_connect = LastFmConnect::Starting;
                self.lastfm.connect(api_key, api_secret);
            }
            SettingsAction::LastFmCancel => {
                self.lastfm.cancel_connect();
                self.lastfm_connect = LastFmConnect::Idle;
            }
            SettingsAction::LastFmDisconnect => {
                self.settings.lastfm.session_key.clear();
                self.settings.lastfm.username.clear();
                self.settings_error = None;
                self.save_settings();
            }
            SettingsAction::OpenUrl(url) => {
                if let Err(error) = system::open_url(&url) {
                    self.settings_error = Some(error);
                }
            }
        }
    }
}
