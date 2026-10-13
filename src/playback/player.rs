use std::{path::Path, time::Instant};

use crate::{
    domain::Track,
    infrastructure::{mpv::MpvBackend, process::ProcessRegistry},
};

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
}

impl Default for Player {
    fn default() -> Self {
        Self {
            backend: MpvBackend::default(),
            position_anchor: None,
            position: 0.0,
            paused: false,
            volume: 100,
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
    }

    pub(crate) fn position_seconds(&self) -> f64 {
        if self.paused {
            self.position
        } else {
            self.position
                + self
                    .position_anchor
                    .map(|anchor| anchor.elapsed().as_secs_f64())
                    .unwrap_or(0.0)
        }
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.position = self.position_seconds();
        self.paused = paused;
        self.position_anchor = if paused { None } else { Some(Instant::now()) };
        self.send(if paused {
            "[\"set_property\",\"pause\",true]"
        } else {
            "[\"set_property\",\"pause\",false]"
        });
    }

    pub(crate) fn seek(&mut self, position: f64) {
        self.position = position.max(0.0);
        self.position_anchor = if self.paused {
            None
        } else {
            Some(Instant::now())
        };
        self.send(&format!(
            "[\"set_property\",\"time-pos\",{}]",
            self.position
        ));
    }

    pub(crate) fn play_stream(
        &mut self,
        track: &Track,
        stream_url: &str,
        mpv: &Path,
    ) -> Result<(), String> {
        let _ = track;
        self.position_anchor = None;
        self.position = 0.0;
        self.paused = false;
        self.backend.play_stream(stream_url, mpv, self.volume)?;
        self.position_anchor = Some(Instant::now());
        Ok(())
    }

    pub(crate) fn is_finished(&mut self) -> bool {
        self.backend.is_finished()
    }
}
