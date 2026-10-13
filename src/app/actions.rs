use crate::ui::footer::FooterAction;

use super::YtMusicApp;

/// Translate presentation-layer footer events into application operations.
pub(super) fn apply_footer_action(app: &mut YtMusicApp, action: FooterAction) {
    match action {
        FooterAction::Previous => app.previous(),
        FooterAction::TogglePause => app.player.set_paused(!app.player.paused),
        FooterAction::Next => app.next(),
        FooterAction::Seek(position) => app.player.seek(position),
        FooterAction::SetVolume(volume) => {
            app.player.volume = volume;
            app.player
                .send(&format!("[\"set_property\",\"volume\",{}]", volume));
        }
    }
}
