use crate::ui::footer::FooterAction;

use super::YtMusicApp;

/// Translate presentation-layer footer events into application operations.
pub(super) fn apply_footer_action(app: &mut YtMusicApp, action: FooterAction) {
    match action {
        FooterAction::Previous => app.previous(),
        // Jam guests follow the host; their footer cannot drive playback.
        FooterAction::TogglePause if app.is_guest() => {}
        FooterAction::Seek(_) if app.is_guest() => {}
        FooterAction::TogglePause => {
            // Pressing play while waiting for guests starts right away.
            app.jam_hold = None;
            app.player.set_paused(!app.player.paused);
        }
        FooterAction::Next => app.next(),
        FooterAction::Seek(position) => app.player.seek(position),
        FooterAction::SetVolume(volume) => {
            app.player.volume = volume;
            app.player
                .send(&format!("[\"set_property\",\"volume\",{}]", volume));
        }
    }
}
