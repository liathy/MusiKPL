use std::{
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    domain::Track,
    infrastructure::{mpv::MpvBackend, process::ProcessRegistry},
};

/// A position MPV reported this soon after a local pause or seek may still
/// describe the state before that command took effect.
const SAMPLE_GUARD: Duration = Duration::from_millis(150);
/// If MPV never reports a position, fall back to timing from the launch
/// rather than appearing stuck forever.
const START_FALLBACK: Duration = Duration::from_secs(10);

/// Application-facing playback state and operations.
///
/// MPV process and IPC details stay behind `MpvBackend`; this type owns the
/// state that the application presents and synchronizes with the player.
pub(crate) struct Player {
    backend: MpvBackend,
    position_anchor: Option<Instant>,
    position: f64,
    pub(crate) paused: bool,
    pub(crate) volume: u8,
    speed: f64,
    /// False while MPV is still opening the stream and nothing is audible.
    started: bool,
    launched: Option<Instant>,
    /// When pause/seek state was last changed locally.
    local_change: Instant,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            backend: MpvBackend::default(),
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
}

impl Player {
    pub(crate) fn set_registry(&mut self, registry: ProcessRegistry) {
        self.backend.set_registry(registry);
    }

    pub(crate) fn send(&mut self, command: &str) {
        self.backend.send(command);
    }

    pub(crate) fn stop(&mut self) {
        self.backend.stop();
        self.position_anchor = None;
        self.paused = false;
        self.position = 0.0;
        self.speed = 1.0;
        self.started = false;
        self.launched = None;
    }

    pub(crate) fn position_seconds(&self) -> f64 {
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

    /// Whether MPV has loaded the stream, so the position is real.
    pub(crate) fn has_started(&self) -> bool {
        self.started
    }

    /// Re-anchor the position on what MPV last reported. Call once per frame.
    pub(crate) fn refresh_position(&mut self) {
        match self.backend.latest_position() {
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

    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.position = self.position_seconds();
        self.paused = paused;
        self.position_anchor = (self.started && !paused).then(Instant::now);
        self.local_change = Instant::now();
        self.send(if paused {
            "[\"set_property\",\"pause\",true]"
        } else {
            "[\"set_property\",\"pause\",false]"
        });
    }

    pub(crate) fn seek(&mut self, position: f64) {
        self.position = position.max(0.0);
        self.position_anchor = (self.started && !self.paused).then(Instant::now);
        self.local_change = Instant::now();
        self.send(&format!(
            "[\"set_property\",\"time-pos\",{}]",
            self.position
        ));
    }

    /// Change playback speed; used by Jam guests to absorb small drift.
    pub(crate) fn set_speed(&mut self, speed: f64) {
        if (speed - self.speed).abs() < 0.001 {
            return;
        }
        self.position = self.position_seconds();
        if self.position_anchor.is_some() {
            self.position_anchor = Some(Instant::now());
        }
        self.speed = speed;
        self.send(&format!("[\"set_property\",\"speed\",{speed:.3}]"));
    }

    /// Start a stream at `start_secs`, optionally paused.
    pub(crate) fn play_stream_at(
        &mut self,
        track: &Track,
        stream_url: &str,
        mpv: &Path,
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
        self.backend
            .play_stream(stream_url, mpv, self.volume, self.position, paused)?;
        let now = Instant::now();
        self.local_change = now;
        self.launched = Some(now);
        // Without a position feed, time from the launch as before.
        if !self.backend.reports_position() {
            self.started = true;
            self.position_anchor = (!paused).then_some(now);
        }
        Ok(())
    }

    pub(crate) fn is_finished(&mut self) -> bool {
        self.backend.is_finished()
    }
}
