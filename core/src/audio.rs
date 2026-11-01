//! Playing one stream at a time: mpv on desktop, ExoPlayer on Android.
//!
//! [`crate::playback::Player`] keeps the state the app shows and syncs (the
//! position, pause, speed for Jams); an [`AudioOutput`] only does what it's
//! told and reports where it is.

use std::time::Instant;

/// The player's own position at a known instant.
#[derive(Clone, Copy, Debug)]
pub struct PositionSample {
    pub position: f64,
    pub taken: Instant,
}

/// How a song fades into the next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FadeCurve {
    /// Straight, evenly.
    Linear,
    /// Slow at both ends, quick in the middle.
    #[default]
    Smooth,
    /// Keeps the loudness even through the mix (DJ style).
    EqualPower,
    /// The new song comes in late and strong.
    Exponential,
}

impl FadeCurve {
    pub const ALL: [FadeCurve; 4] = [
        FadeCurve::Smooth,
        FadeCurve::EqualPower,
        FadeCurve::Linear,
        FadeCurve::Exponential,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::Smooth => "Smooth",
            Self::EqualPower => "Equal power",
            Self::Exponential => "Late",
        }
    }

    /// The incoming song's loudness of the way through (0 to 1); the
    /// outgoing one follows `gain(1 - t)`.
    pub fn gain(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Smooth => (1.0 - (std::f64::consts::PI * t).cos()) / 2.0,
            Self::EqualPower => (std::f64::consts::FRAC_PI_2 * t).sin(),
            Self::Exponential => t * t,
        }
    }
}

/// A crossfade: how long, and how.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub seconds: f64,
    pub curve: FadeCurve,
}

pub trait AudioOutput: Send {
    /// Start `url` at `start_secs`, optionally paused, at `volume` (0–100).
    fn play(&mut self, url: &str, volume: u8, start_secs: f64, paused: bool) -> Result<(), String>;
    fn set_paused(&mut self, paused: bool);
    fn seek(&mut self, position: f64);
    /// Jam guests nudge the speed to absorb small drift.
    fn set_speed(&mut self, speed: f64);
    fn set_volume(&mut self, volume: u8);
    fn stop(&mut self);
    /// The latest position the player reported, if it reports one.
    fn latest_position(&self) -> Option<PositionSample>;
    /// Whether positions are reported at all; without them, time is counted
    /// from the start.
    fn reports_position(&self) -> bool;
    /// The stream ended (or the player quit).
    fn is_finished(&mut self) -> bool;
    /// Start `url` while the song playing fades out and this one fades in.
    /// Outputs that can't play two streams at once just switch.
    fn crossfade(
        &mut self,
        url: &str,
        volume: u8,
        start_secs: f64,
        fade: Fade,
    ) -> Result<(), String> {
        let _ = fade;
        self.play(url, volume, start_secs, false)
    }
}

/// Plays nothing: tests, and the moment before the platform's output exists.
#[derive(Default)]
pub struct SilentOutput {
    playing: bool,
}

impl AudioOutput for SilentOutput {
    fn play(&mut self, _url: &str, _volume: u8, _start: f64, _paused: bool) -> Result<(), String> {
        self.playing = true;
        Ok(())
    }
    fn set_paused(&mut self, _paused: bool) {}
    fn seek(&mut self, _position: f64) {}
    fn set_speed(&mut self, _speed: f64) {}
    fn set_volume(&mut self, _volume: u8) {}
    fn stop(&mut self) {
        self.playing = false;
    }
    fn latest_position(&self) -> Option<PositionSample> {
        None
    }
    fn reports_position(&self) -> bool {
        false
    }
    fn is_finished(&mut self) -> bool {
        false
    }
}
