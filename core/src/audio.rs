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
