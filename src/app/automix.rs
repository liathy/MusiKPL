//! Auto Mix: keeps the queue topped up with songs like the ones playing.
//!
//! The queue is refilled in the background while a song still plays, so the
//! next one is ready when it ends. A refill that fails tries another source
//! and then retries later; when the queue runs dry the player waits for the
//! mix instead of stopping. Songs the user added are never removed by it.

use std::{
    collections::HashSet,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Instant,
};

use crate::{
    domain::Track,
    media::query_ytdlp,
    recommendations::{
        mix_sources, retry_delay, select_mix_items, Exclusions, MIX_BATCH, MIX_LOW_WATER,
    },
};

use super::{errors::AppError, library::unix_now, YtMusicApp};

type MixResult = Result<Vec<Track>, String>;

#[derive(Default)]
pub(super) struct AutoMix {
    /// The refill being searched for, if any.
    pub(super) rx: Option<Receiver<MixResult>>,
    /// Queued songs that Auto Mix picked, as opposed to the user.
    pub(super) picks: HashSet<String>,
    /// The queue ran dry: play the first song the mix delivers.
    pub(super) waiting: bool,
    /// After a refill found nothing, don't try again before this.
    pub(super) retry_at: Option<Instant>,
    /// Refills in a row that found nothing.
    pub(super) failures: u32,
}

impl AutoMix {
    pub(super) fn busy(&self) -> bool {
        self.rx.is_some()
    }
}

impl YtMusicApp {
    /// Everything that must not be picked again: what plays, what is queued,
    /// and what was played.
    fn mix_exclusions(&self) -> Exclusions {
        let mut exclusions = Exclusions::default();
        let tracks = self
            .current
            .iter()
            .chain(self.pending_track.iter())
            .chain(self.queue.iter())
            .chain(self.history.iter());
        for track in tracks {
            exclusions.add(track);
        }
        exclusions
    }

    /// What the next refill is based on: the newest Auto Mix pick, so each
    /// refill moves the mix along instead of asking for the same Mix again;
    /// otherwise the song loading or playing.
    fn mix_seed(&self) -> Option<Track> {
        self.queue
            .iter()
            .filter(|track| self.automix.picks.contains(&track.id))
            .last()
            .or(self.pending_track.as_ref())
            .or(self.current.as_ref())
            .cloned()
    }

    fn request_mix(&mut self, seed: &Track) {
        let sources = mix_sources(seed);
        let mut exclusions = self.mix_exclusions();
        // Lean toward the user's taste: skipped songs out, favorite
        // artists up, the Mix's own order otherwise.
        let preferences = self.library.taste.preferences(unix_now());
        let tools = self.tools.clone();
        let registry = self.registry.clone();
        let (sender, receiver) = mpsc::channel();
        self.automix.rx = Some(receiver);
        std::thread::spawn(move || {
            let mut last_error = None;
            for source in sources {
                match query_ytdlp(&tools, &source, true, &registry) {
                    Ok(items) => {
                        let items = preferences.rank(items);
                        let picked = select_mix_items(items, &mut exclusions, MIX_BATCH);
                        if !picked.is_empty() {
                            let _ = sender.send(Ok(picked));
                            return;
                        }
                    }
                    Err(error) => last_error = Some(error),
                }
            }
            let _ = sender.send(Err(
                last_error.unwrap_or_else(|| "no new songs were found".into())
            ));
        });
    }

    /// Called every frame: start a refill when the queue runs low.
    pub(super) fn keep_mix_filled(&mut self) {
        if self.is_guest() || !self.auto_mix || self.automix.busy() {
            return;
        }
        if self.automix.retry_at.is_some_and(|at| Instant::now() < at) {
            return;
        }
        let listening = self.current.is_some() || self.loading || self.automix.waiting;
        if !listening || self.queue.len() >= MIX_LOW_WATER {
            return;
        }
        if let Some(seed) = self.mix_seed() {
            self.request_mix(&seed);
        }
    }

    /// Queue a refill's songs, filtered again because the queue may have
    /// changed while yt-dlp searched. Returns how many were added.
    pub(super) fn queue_mix_picks(&mut self, items: Vec<Track>) -> usize {
        let mut exclusions = self.mix_exclusions();
        let mut added = 0;
        for track in select_mix_items(items, &mut exclusions, MIX_BATCH) {
            self.automix.picks.insert(track.id.clone());
            if self.queue.enqueue_unique(track) {
                added += 1;
            }
        }
        added
    }

    pub(super) fn poll_mix(&mut self) {
        let Some(receiver) = &self.automix.rx else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the mix search stopped unexpectedly".into()),
        };
        self.automix.rx = None;
        let (added, reason) = match result {
            Ok(items) => (self.queue_mix_picks(items), None),
            Err(error) => (0, Some(error)),
        };
        if added > 0 {
            self.automix.failures = 0;
            self.automix.retry_at = None;
            if self.automix.waiting {
                self.next();
            }
            return;
        }
        self.automix.failures += 1;
        self.automix.retry_at = Some(Instant::now() + retry_delay(self.automix.failures));
        // Only interrupt when the music stopped because of it, and only once.
        if self.automix.waiting && self.automix.failures == 1 {
            let reason = reason.unwrap_or_else(|| "no new songs were found".into());
            self.error = Some(AppError::Mix(format!(
                "Auto Mix couldn't find the next song ({reason}). It will keep trying."
            )));
        }
    }

    /// A new song was chosen from search: start a fresh mix around it, but
    /// keep the songs the user queued.
    pub(super) fn reset_mix(&mut self) {
        let picks = std::mem::take(&mut self.automix.picks);
        self.queue.retain(|track| !picks.contains(&track.id));
        self.automix = Default::default();
    }

    pub(super) fn set_auto_mix(&mut self, enabled: bool) {
        self.auto_mix = enabled;
        self.settings.auto_mix = enabled;
        self.library.settings_changed = true;
        if enabled {
            self.automix.failures = 0;
            self.automix.retry_at = None;
        } else {
            self.automix.rx = None;
            if std::mem::take(&mut self.automix.waiting) {
                // Nothing will come to continue with.
                self.current = None;
            }
        }
    }

    /// A song the user chose to add: it plays right away when nothing is
    /// playing, otherwise it is queued ahead of Auto Mix's picks.
    pub(super) fn add_to_queue(&mut self, track: Track) {
        if self.current.is_none() && !self.loading && !self.automix.waiting {
            self.play_queue_track(track);
            return;
        }
        let picks = &self.automix.picks;
        let index = self
            .queue
            .position(|queued| picks.contains(&queued.id))
            .unwrap_or(self.queue.len());
        self.queue.insert_unique(index, track);
    }
}
