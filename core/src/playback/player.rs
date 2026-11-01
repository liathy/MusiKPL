use std::time::{Duration, Instant};

use crate::{
    audio::{AudioOutput, SilentOutput},
    domain::Track,
};

/// A position the player reported this soon after a local pause or seek may
/// still describe the state before that command took effect.
const SAMPLE_GUARD: Duration = Duration::from_millis(150);
/// If the player never reports a position, fall back to timing from the
/// launch rather than appearing stuck forever.
const START_FALLBACK: Duration = Duration::from_secs(10);

/// Application-facing playback state and operations.
///
/// The platform's player (mpv, ExoPlayer) stays behind [`AudioOutput`]; this
/// type owns the state that the application presents and synchronizes with
/// it.
pub struct Player {
    output: Box<dyn AudioOutput>,
    position_anchor: Option<Instant>,
    position: f64,
    pub paused: bool,
    pub volume: u8,
    speed: f64,
    /// False while the player is still opening the stream and nothing is
    /// audible.
    started: bool,
    launched: Option<Instant>,
    /// When pause/seek state was last changed locally.
    local_change: Instant,
}

impl Default for Player {
    fn default() -> Self {
        Self::new(Box::new(SilentOutput::default()))
    }
}

impl Player {
    pub fn new(output: Box<dyn AudioOutput>) -> Self {
        Self {
            output,
            position_anchor: None,
            position: 0.0,
            paused: false,
            volume: 100,
            speed: 1.0,
            started: false,
            launched: None,
            local_change: Instant::now(),
        }
    }

    /// Swap the platform's player in (the app creates it after the core).
    pub fn set_output(&mut self, output: Box<dyn AudioOutput>) {
        self.output.stop();
        self.output = output;
    }

    pub fn stop(&mut self) {
        self.output.stop();
        self.position_anchor = None;
        self.paused = false;
        self.position = 0.0;
        self.speed = 1.0;
        self.started = false;
        self.launched = None;
    }

    pub fn position_seconds(&self) -> f64 {
        if self.paused {
            self.position
        } else {
            self.position
                + self
                    .position_anchor
                    .map(|anchor| anchor.elapsed().as_secs_f64() * self.speed)
                    .unwrap_or(0.0)
        }
    }

    /// Whether the player has loaded the stream, so the position is real.
    pub fn has_started(&self) -> bool {
        self.started
    }

    /// Re-anchor the position on what the player last reported. Call once
    /// per frame.
    pub fn refresh_position(&mut self) {
        match self.output.latest_position() {
            Some(sample) if sample.taken > self.local_change + SAMPLE_GUARD => {
                self.started = true;
                self.position = sample.position.max(0.0);
                self.position_anchor = (!self.paused).then_some(sample.taken);
            }
            Some(_) => {}
            None => {
                let overdue = self
                    .launched
                    .is_some_and(|launched| launched.elapsed() > START_FALLBACK);
                if !self.started && overdue {
                    self.started = true;
                    self.position_anchor = (!self.paused).then(Instant::now);
                }
            }
        }
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.position = self.position_seconds();
        self.paused = paused;
        self.position_anchor = (self.started && !paused).then(Instant::now);
        self.local_change = Instant::now();
        self.output.set_paused(paused);
    }

    pub fn seek(&mut self, position: f64) {
        self.position = position.max(0.0);
        self.position_anchor = (self.started && !self.paused).then(Instant::now);
        self.local_change = Instant::now();
        self.output.seek(self.position);
    }

    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume;
        self.output.set_volume(volume);
    }

    /// Change playback speed; used by Jam guests to absorb small drift.
    pub fn set_speed(&mut self, speed: f64) {
        if (speed - self.speed).abs() < 0.001 {
            return;
        }
        self.position = self.position_seconds();
        if self.position_anchor.is_some() {
            self.position_anchor = Some(Instant::now());
        }
        self.speed = speed;
        self.output.set_speed(speed);
    }

    /// Show a song as paused at `position` with nothing loaded, like the
    /// last session's song at startup.
    pub fn show_paused_at(&mut self, position: f64) {
        self.stop();
        self.paused = true;
        self.position = position.max(0.0);
    }

    /// Start a stream at `start_secs`, optionally paused.
    pub fn play_stream_at(
        &mut self,
        track: &Track,
        stream_url: &str,
        start_secs: f64,
        paused: bool,
    ) -> Result<(), String> {
        let _ = track;
        self.position_anchor = None;
        self.position = start_secs.max(0.0);
        self.paused = paused;
        self.speed = 1.0;
        self.started = false;
        self.launched = None;
        self.output
            .play(stream_url, self.volume, self.position, paused)?;
        let now = Instant::now();
        self.local_change = now;
        self.launched = Some(now);
        // Without a position feed, time from the launch as before.
        if !self.output.reports_position() {
            self.started = true;
            self.position_anchor = (!paused).then_some(now);
        }
        Ok(())
    }

    pub fn is_finished(&mut self) -> bool {
        self.output.is_finished()
    }
}
