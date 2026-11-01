use crate::ui::footer::FooterAction;

use super::YtMusicApp;

/// Translate presentation-layer footer events into application operations.
pub(super) fn apply_footer_action(app: &mut YtMusicApp, action: FooterAction) {
    match action {
        FooterAction::Previous => app.previous(),
        // Jam guests follow the host; their footer cannot drive playback.
        FooterAction::TogglePause if app.is_guest() => {}
        FooterAction::Seek(_) if app.is_guest() => {}
        FooterAction::TogglePause => app.toggle_pause(),
        FooterAction::Next => app.next(),
        FooterAction::Seek(position) => app.seek(position),
        FooterAction::SetVolume(volume) => app.set_volume(volume),
        FooterAction::ToggleMute => app.toggle_mute(),
        FooterAction::CycleShuffle => app.cycle_shuffle(),
        FooterAction::CycleRepeat => app.cycle_repeat(),
        FooterAction::OpenArtist => {
            if let Some(track) = app.shown_track().cloned() {
                app.open_track_artist(&track);
            }
        }
        FooterAction::ToggleQueue => app.queue_open = !app.queue_open,
        FooterAction::ToggleLike => {
            if let Some(track) = app.shown_track().cloned() {
                app.toggle_like(&track);
            }
        }
        FooterAction::AddToPlaylist(target) => {
            if let Some(track) = app.shown_track().cloned() {
                app.add_to_playlist(track, target);
            }
        }
    }
}
