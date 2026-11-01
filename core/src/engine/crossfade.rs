//! Crossfade: the next song starts under the end of this one, like
//! Spotify. Settings choose how long and how (the curve); a playlist can
//! keep the Settings, turn it off, or have its own.
//!
//! A fade starts only when the next song's stream is already looked up, so
//! it begins exactly on time. Not in Jams (everyone plays on the host's
//! clock), and not when repeating one song.

use crate::{
    audio::{Fade, FadeCurve},
    domain::Transition,
    settings::{RepeatMode, MAX_CROSSFADE},
};

use super::Engine;

/// Skipping fades quickly, at most this long.
const SKIP_FADE: f64 = 2.0;
/// Not worth fading when this little is left.
const MIN_LEFT: f64 = 0.5;

impl Engine {
    /// The fade between songs now: the playing playlist's, else Settings'.
    pub fn current_fade(&self) -> Option<Fade> {
        let transition = self
            .order
            .playlist
            .filter(|_| self.order.from_playlist)
            .and_then(|id| self.library.playlists.get(id))
            .map_or(Transition::Default, |playlist| playlist.transition);
        let (seconds, curve) = match transition {
            Transition::Off => return None,
            Transition::Fade { seconds, curve } => (seconds, curve),
            Transition::Default => {
                let crossfade = self.settings.crossfade;
                if !crossfade.enabled {
                    return None;
                }
                (crossfade.seconds, crossfade.curve)
            }
        };
        let seconds = f64::from(seconds.clamp(1, MAX_CROSSFADE));
        Some(Fade { seconds, curve })
    }

    /// Fading is possible now: playing, outside a Jam, the next song ready.
    fn can_fade(&self) -> bool {
        let next_ready = self.queue.iter().next().is_some_and(|next| {
            self.local_file(&next.id).is_some()
                || self
                    .preload
                    .as_ref()
                    .is_some_and(|preload| preload.id == next.id && preload.url.is_some())
        });
        next_ready
            && self.jam.info.role.is_none()
            && !self.loading
            && self.resume_at.is_none()
            && !self.player.paused
            && self.player.has_started()
            && self.settings.repeat != RepeatMode::One
    }

    /// Every tick: near the end of a song, start fading into the next.
    pub fn tick_crossfade(&mut self) {
        if self.pending_fade.is_some() || self.finished_handled {
            return;
        }
        let Some(fade) = self.current_fade() else {
            return;
        };
        let Some(duration) = self.current.as_ref().and_then(|track| track.duration) else {
            return;
        };
        let left = duration - self.player.position_seconds();
        if left > fade.seconds || left < MIN_LEFT || !self.can_fade() {
            return;
        }
        // The fade can't outlast the song.
        let fade = Fade {
            seconds: fade.seconds.min(left),
            ..fade
        };
        self.finished_handled = true;
        self.pending_fade = Some(fade);
        self.next();
    }

    /// The user skipped: a quick fade, when crossfading and allowed.
    pub(super) fn skip_fade(&self) -> Option<Fade> {
        let on_skip = self.settings.crossfade.on_skip;
        if !on_skip || !self.can_fade() {
            return None;
        }
        self.current_fade().map(|fade| Fade {
            seconds: fade.seconds.min(SKIP_FADE),
            ..fade
        })
    }

    pub fn set_crossfade(&mut self, enabled: bool, seconds: u8, curve: FadeCurve, on_skip: bool) {
        self.settings.crossfade = crate::settings::Crossfade {
            enabled,
            seconds: seconds.clamp(1, MAX_CROSSFADE),
            curve,
            on_skip,
        };
        self.library.settings_changed = true;
    }

    pub fn set_playlist_transition(&mut self, id: u64, transition: Transition) {
        let transition = match transition {
            Transition::Fade { seconds, curve } => Transition::Fade {
                seconds: seconds.clamp(1, MAX_CROSSFADE),
                curve,
            },
            other => other,
        };
        if self.library.playlists.set_transition(id, transition) {
            self.library.playlists_changed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Track;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(200.0),
            channel: "Band".into(),
        }
    }

    #[test]
    fn a_playlist_can_keep_turn_off_or_choose_its_fade() {
        let mut app = Engine::default();
        assert_eq!(app.current_fade(), None, "off by default");
        app.set_crossfade(true, 8, FadeCurve::Linear, true);
        assert_eq!(app.current_fade().unwrap().seconds, 8.0);

        let id = app.library.playlists.create();
        app.library.playlists.add(id, track("a"));
        app.order.playlist = Some(id);
        app.order.from_playlist = true;
        app.set_playlist_transition(id, Transition::Off);
        assert_eq!(app.current_fade(), None);
        app.set_playlist_transition(
            id,
            Transition::Fade {
                seconds: 40,
                curve: FadeCurve::EqualPower,
            },
        );
        let fade = app.current_fade().unwrap();
        assert_eq!((fade.seconds, fade.curve), (12.0, FadeCurve::EqualPower));
    }

    #[test]
    fn curves_go_from_silent_to_full() {
        for curve in FadeCurve::ALL {
            assert!(curve.gain(0.0).abs() < 1e-9);
            assert!((curve.gain(1.0) - 1.0).abs() < 1e-9);
            assert!(curve.gain(0.5) > 0.0 && curve.gain(0.5) < 1.0);
        }
    }

    #[test]
    fn no_fade_without_the_next_song_ready() {
        let mut app = Engine::default();
        app.set_crossfade(true, 6, FadeCurve::Smooth, true);
        app.current = Some(track("now"));
        app.queue.push_back(track("next"));
        app.tick_crossfade();
        assert!(app.pending_fade.is_none());
    }
}
