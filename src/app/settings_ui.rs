//! The settings window: what it shows, and its switches and buttons.

use eframe::egui;
use musikpl_core::{engine::LastFmConnect, lastfm::built_in_keys};

use crate::{
    infrastructure::system,
    ui::{
        icons::IconTextures,
        overlays::{self, LastFmState, SettingsAction, SettingsView},
        responsive::ResponsiveUi,
    },
};

use super::YtMusicApp;

impl YtMusicApp {
    pub fn render_settings(
        &mut self,
        ctx: &egui::Context,
        window: egui::Rect,
        open: f32,
        icons: &IconTextures,
        r: &ResponsiveUi,
    ) {
        let engine = &self.engine;
        let lastfm = if engine.lastfm_connected() {
            LastFmState::Connected {
                username: &engine.settings.lastfm.username,
            }
        } else {
            match &engine.lastfm_connect {
                LastFmConnect::Idle => LastFmState::Disconnected,
                LastFmConnect::Starting => LastFmState::Starting,
                LastFmConnect::AwaitingApproval(url) => LastFmState::AwaitingApproval { url },
            }
        };
        let view = SettingsView {
            page: self.settings_page,
            start_with_windows: engine.settings.start_with_windows,
            auto_mix: engine.auto_mix,
            close_to_tray: engine.settings.close_to_tray,
            invite_popups: engine.settings.invite_popups,
            lastfm,
            one_click: built_in_keys().is_some(),
            error: engine.settings_error.as_deref(),
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
            SettingsAction::Close => self.settings_open = false,
            SettingsAction::SetAutoMix(enabled) => self.set_auto_mix(enabled),
            SettingsAction::SetInvitePopups(enabled) => self.engine.set_invite_popups(enabled),
            SettingsAction::SetCloseToTray(enabled) => {
                self.settings.close_to_tray = enabled;
                self.save_settings();
            }
            SettingsAction::ClearHistory => self.engine.clear_history(),
            SettingsAction::PickGenres => self.pick_genres(),
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
                // The user's own pair if typed in, else MusiKPL's.
                self.lastfm_key_draft = self.lastfm_key_draft.trim().to_owned();
                self.lastfm_secret_draft = self.lastfm_secret_draft.trim().to_owned();
                self.engine
                    .connect_lastfm(&self.lastfm_key_draft, &self.lastfm_secret_draft);
            }
            SettingsAction::LastFmCancel => self.engine.cancel_lastfm_connect(),
            SettingsAction::LastFmDisconnect => self.engine.disconnect_lastfm(),
            SettingsAction::OpenUrl(url) => {
                if let Err(error) = system::open_url(&url) {
                    self.settings_error = Some(error);
                }
            }
        }
    }
}
