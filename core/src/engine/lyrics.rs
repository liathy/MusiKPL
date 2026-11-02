//! Lyrics of the song playing, looked up only while the shell shows them.

use std::{
    sync::{
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
};

use crate::{
    domain::song_info,
    lyrics::{self, Lyrics},
};

use super::Engine;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum LyricsStatus {
    #[default]
    Idle,
    Loading,
    Found(Arc<Lyrics>),
    /// The song has no lyrics we could find.
    Missing,
    Failed(String),
}

#[derive(Default)]
pub struct LyricsState {
    /// The shell shows lyrics: keep them for the song playing.
    pub wanted: bool,
    /// The song `status` belongs to.
    pub id: String,
    pub status: LyricsStatus,
    rx: Option<Receiver<Result<Option<Lyrics>, String>>>,
}

impl Engine {
    pub fn show_lyrics(&mut self, shown: bool) {
        self.lyrics.wanted = shown;
    }

    /// The line being sung now, for synced lyrics.
    pub fn lyrics_line(&self) -> Option<usize> {
        match &self.lyrics.status {
            LyricsStatus::Found(lyrics) => lyrics.line_at(self.player.position_seconds() + 0.15),
            _ => None,
        }
    }

    /// Look up again after a failure (the Retry button).
    pub fn retry_lyrics(&mut self) {
        self.lyrics.id.clear();
    }

    pub fn tick_lyrics(&mut self) {
        if let Some(rx) = &self.lyrics.rx {
            match rx.try_recv() {
                Ok(result) => {
                    self.lyrics.rx = None;
                    // A lookup that could not finish (offline, service busy) is
                    // reported as not found, so the sheet never waits forever.
                    self.lyrics.status = match result {
                        Ok(Some(found)) => LyricsStatus::Found(Arc::new(found)),
                        Ok(None) | Err(_) => LyricsStatus::Missing,
                    };
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.lyrics.rx = None;
                    self.lyrics.status = LyricsStatus::Missing;
                }
            }
        }
        if !self.lyrics.wanted {
            return;
        }
        let Some(track) = self.shown_track().cloned() else {
            self.lyrics.id.clear();
            self.lyrics.status = LyricsStatus::Idle;
            return;
        };
        if track.id == self.lyrics.id {
            return;
        }
        self.lyrics.id = track.id.clone();
        // Saved from before: no lookup.
        if let Some(saved) = lyrics::cached(&track.id) {
            self.lyrics.rx = None;
            self.lyrics.status = match saved {
                Some(found) => LyricsStatus::Found(Arc::new(found)),
                None => LyricsStatus::Missing,
            };
            return;
        }
        self.lyrics.status = LyricsStatus::Loading;
        let (sender, receiver) = mpsc::channel();
        let waker = self.waker.clone();
        let song = song_info(&track);
        thread::spawn(move || {
            let result = lyrics::fetch(&song, track.duration);
            if let Ok(found) = &result {
                lyrics::remember(&track.id, found);
            }
            let _ = sender.send(result);
            waker();
        });
        self.lyrics.rx = Some(receiver);
    }
}
