//! When to tell Last.fm about the current song.
//!
//! Last.fm's rule: a track longer than 30 seconds is scrobbled once it has
//! been listened to for half its length or four minutes, whichever comes
//! first. Listening time only grows while audio plays; seeking forward does
//! not count.

/// Longest listen that counts toward a scrobble.
const MAX_REQUIRED_SECS: f64 = 240.0;
const MIN_TRACK_SECS: f64 = 30.0;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ScrobbleStep {
    Nothing,
    /// The song just started playing.
    NowPlaying,
    /// The song has been listened to long enough. `started_at` is when it
    /// started, in Unix seconds.
    Scrobble {
        started_at: u64,
    },
}

#[derive(Default)]
pub(crate) struct ScrobbleTracker {
    video_id: Option<String>,
    started_at: u64,
    listened: f64,
    /// Position and wall-clock time of the previous update.
    last: Option<(f64, f64)>,
    announced: bool,
    scrobbled: bool,
}

impl ScrobbleTracker {
    /// Call every frame with the player's state. `now` is Unix time in
    /// seconds.
    pub(crate) fn update(
        &mut self,
        video_id: Option<&str>,
        position: f64,
        playing: bool,
        duration: Option<f64>,
        now: f64,
    ) -> ScrobbleStep {
        if video_id != self.video_id.as_deref() {
            *self = Self {
                video_id: video_id.map(str::to_owned),
                ..Self::default()
            };
        }
        if video_id.is_none() {
            return ScrobbleStep::Nothing;
        }
        if !playing {
            self.last = None;
            return ScrobbleStep::Nothing;
        }

        if let Some((last_position, last_time)) = self.last {
            let advanced = position - last_position;
            let elapsed = (now - last_time).max(0.0);
            // A jump larger than the time that passed is a seek.
            if advanced > 0.0 && advanced <= elapsed * 1.5 + 1.0 {
                self.listened += advanced;
            }
        }
        self.last = Some((position, now));

        if !self.announced {
            self.announced = true;
            self.started_at = (now - position).max(0.0) as u64;
            return ScrobbleStep::NowPlaying;
        }
        let long_enough = duration.is_none_or(|duration| duration > MIN_TRACK_SECS);
        let required = duration.map_or(MAX_REQUIRED_SECS, |duration| {
            (duration / 2.0).min(MAX_REQUIRED_SECS)
        });
        if !self.scrobbled && long_enough && self.listened >= required {
            self.scrobbled = true;
            return ScrobbleStep::Scrobble {
                started_at: self.started_at,
            };
        }
        ScrobbleStep::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Play `seconds` of audio in one-second frames, returning every step
    /// that was not `Nothing`.
    fn play(
        tracker: &mut ScrobbleTracker,
        id: &str,
        from: f64,
        seconds: u32,
        duration: Option<f64>,
        clock: &mut f64,
    ) -> Vec<ScrobbleStep> {
        let mut steps = Vec::new();
        for second in 0..=seconds {
            let step = tracker.update(Some(id), from + second as f64, true, duration, *clock);
            if step != ScrobbleStep::Nothing {
                steps.push(step);
            }
            *clock += 1.0;
        }
        *clock -= 1.0;
        steps
    }

    #[test]
    fn announces_then_scrobbles_once_at_half_length() {
        let mut tracker = ScrobbleTracker::default();
        let mut clock = 1_000.0;
        let steps = play(&mut tracker, "a", 0.0, 60, Some(180.0), &mut clock);
        assert_eq!(steps, vec![ScrobbleStep::NowPlaying]);

        // 90 seconds in, half of the 180-second track has been heard.
        let steps = play(&mut tracker, "a", 60.0, 80, Some(180.0), &mut clock);
        assert_eq!(steps, vec![ScrobbleStep::Scrobble { started_at: 1_000 }]);
    }

    #[test]
    fn long_tracks_need_four_minutes() {
        let mut tracker = ScrobbleTracker::default();
        let mut clock = 0.0;
        let steps = play(&mut tracker, "a", 0.0, 239, Some(3_600.0), &mut clock);
        assert_eq!(steps, vec![ScrobbleStep::NowPlaying]);
        let steps = play(&mut tracker, "a", 239.0, 2, Some(3_600.0), &mut clock);
        assert_eq!(steps, vec![ScrobbleStep::Scrobble { started_at: 0 }]);
    }

    #[test]
    fn seeking_forward_does_not_count_as_listening() {
        let mut tracker = ScrobbleTracker::default();
        let mut clock = 0.0;
        play(&mut tracker, "a", 0.0, 10, Some(200.0), &mut clock);
        // Jump to near the end: only real listening counts.
        let steps = play(&mut tracker, "a", 190.0, 9, Some(200.0), &mut clock);
        assert!(steps.is_empty());
    }

    #[test]
    fn pauses_and_short_tracks_never_scrobble() {
        let mut tracker = ScrobbleTracker::default();
        for second in 0..100 {
            let step = tracker.update(Some("a"), 5.0, false, Some(100.0), second as f64);
            assert_eq!(step, ScrobbleStep::Nothing);
        }

        let mut clock = 0.0;
        let steps = play(&mut tracker, "short", 0.0, 25, Some(25.0), &mut clock);
        assert_eq!(steps, vec![ScrobbleStep::NowPlaying]);
    }

    #[test]
    fn a_new_track_starts_over() {
        let mut tracker = ScrobbleTracker::default();
        let mut clock = 0.0;
        play(&mut tracker, "a", 0.0, 120, Some(200.0), &mut clock);
        let steps = play(&mut tracker, "b", 0.0, 5, Some(200.0), &mut clock);
        assert_eq!(steps, vec![ScrobbleStep::NowPlaying]);
    }
}
