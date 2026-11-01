//! Shuffle, Smart Shuffle and Repeat.
//!
//! - **Shuffle** plays the queue (a playlist, the songs the user added) in
//!   a random order; turning it off puts the old order back.
//! - **Smart Shuffle** shuffles too, and mixes in recommended songs that fit
//!   what's playing: about one in every four, found from YouTube Mixes of
//!   the songs being shuffled. They leave the queue when it's turned off.
//! - **Repeat all** starts the playlist over when it ends (or, outside a
//!   playlist, the songs played since the user picked one), instead of
//!   Auto Mix adding new songs. **Repeat one** plays the same song again.

use std::{
    collections::HashSet,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use crate::{
    domain::Track,
    recommendations::{mix_sources, rng::Rng, select_mix_items},
    settings::{RepeatMode, ShuffleMode},
};

use super::Engine;

/// Songs remembered as "what's being listened to" outside a playlist.
const CONTEXT_LIMIT: usize = 200;
/// Smart Shuffle keeps this many recommended songs waiting in the queue.
const SMART_AHEAD: usize = 2;
/// One recommended song after this many of the user's.
const SMART_EVERY: usize = 3;
/// After a recommendation search found nothing, wait before another.
const SMART_RETRY: Duration = Duration::from_secs(60);

#[derive(Default)]
pub struct PlayOrder {
    /// What's being listened to, for Repeat all: the playlist being played,
    /// or the songs played since the user picked one.
    pub context: Vec<Track>,
    /// `context` is a playlist (else it grows as songs play).
    pub from_playlist: bool,
    /// The queue's order before shuffling (song ids), to go back to.
    unshuffled: Option<Vec<String>>,
    /// Songs Smart Shuffle added to the queue.
    pub smart_picks: HashSet<String>,
    smart_rx: Option<Receiver<Result<Vec<Track>, String>>>,
    smart_retry_at: Option<Instant>,
}

impl PlayOrder {
    pub fn busy(&self) -> bool {
        self.smart_rx.is_some()
    }
}

impl Engine {
    pub fn shuffle(&self) -> ShuffleMode {
        self.settings.shuffle
    }

    pub fn repeat(&self) -> RepeatMode {
        self.settings.repeat
    }

    /// The shuffle button: off, on, smart, off.
    pub fn cycle_shuffle(&mut self) {
        self.set_shuffle(self.settings.shuffle.next());
    }

    /// The repeat button: off, all, one, off.
    pub fn cycle_repeat(&mut self) {
        self.set_repeat(self.settings.repeat.next());
    }

    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.settings.repeat = mode;
        self.library.settings_changed = true;
    }

    pub fn set_shuffle(&mut self, mode: ShuffleMode) {
        let was = self.settings.shuffle;
        self.settings.shuffle = mode;
        self.library.settings_changed = true;
        if was == ShuffleMode::Off && mode != ShuffleMode::Off {
            self.shuffle_queue();
        }
        if mode != ShuffleMode::Smart {
            self.drop_smart_picks();
        }
        if mode == ShuffleMode::Off {
            self.unshuffle_queue();
        }
    }

    /// Shuffle the songs waiting (the user's; Auto Mix's picks stay after
    /// them), remembering the order to go back to.
    fn shuffle_queue(&mut self) {
        self.order.unshuffled = Some(self.queue.iter().map(|track| track.id.clone()).collect());
        let picks = self.automix.picks.clone();
        let (mut mine, mixed): (Vec<Track>, Vec<Track>) = self
            .queue
            .iter()
            .cloned()
            .partition(|track| !picks.contains(&track.id));
        Rng::from_time().shuffle(&mut mine);
        self.queue.retain(|_| false);
        for track in mine.into_iter().chain(mixed) {
            self.queue.push_back(track);
        }
    }

    /// Put the queue back in its order from before shuffling; songs added
    /// since stay after those.
    fn unshuffle_queue(&mut self) {
        let Some(order) = self.order.unshuffled.take() else {
            return;
        };
        let mut tracks: Vec<Track> = self.queue.iter().cloned().collect();
        tracks.sort_by_key(|track| {
            order
                .iter()
                .position(|id| *id == track.id)
                .unwrap_or(usize::MAX)
        });
        self.queue.retain(|_| false);
        for track in tracks {
            self.queue.push_back(track);
        }
    }

    fn drop_smart_picks(&mut self) {
        let picks = std::mem::take(&mut self.order.smart_picks);
        self.queue.retain(|track| !picks.contains(&track.id));
        self.order.smart_rx = None;
        self.order.smart_retry_at = None;
    }

    /// A playlist starts playing from `first`: it's what Repeat all repeats,
    /// and with shuffle on, the rest of it comes in a random order.
    pub fn start_playlist_context(&mut self, playlist: Vec<Track>) {
        self.order = PlayOrder {
            context: playlist,
            from_playlist: true,
            ..PlayOrder::default()
        };
    }

    /// A song picked outside a playlist: listening starts over from it.
    pub fn start_song_context(&mut self, track: &Track) {
        self.order = PlayOrder {
            context: vec![track.clone()],
            ..PlayOrder::default()
        };
    }

    /// A song started: outside a playlist, it joins what Repeat all repeats.
    pub fn note_played(&mut self, track: &Track) {
        if self.order.from_playlist || self.order.context.iter().any(|t| t.id == track.id) {
            return;
        }
        self.order.context.push(track.clone());
        if self.order.context.len() > CONTEXT_LIMIT {
            self.order.context.remove(0);
        }
    }

    /// Repeat all, at the end of the queue: queue what's being listened to
    /// again (shuffled when shuffle is on). False when there's nothing.
    pub fn repeat_context(&mut self) -> bool {
        if self.settings.repeat != RepeatMode::All || self.order.context.is_empty() {
            return false;
        }
        let mut tracks = self.order.context.clone();
        if self.settings.shuffle != ShuffleMode::Off {
            Rng::from_time().shuffle(&mut tracks);
        }
        for track in tracks {
            self.queue.enqueue_unique(track);
        }
        !self.queue.is_empty()
    }

    /// Every tick, with Smart Shuffle on: keep a couple of recommended songs
    /// mixed into what's coming up.
    pub fn keep_smart_shuffle(&mut self) {
        self.poll_smart();
        if self.settings.shuffle != ShuffleMode::Smart
            || self.is_guest()
            || self.order.smart_rx.is_some()
            || self.order.context.is_empty()
            || self.queue.is_empty()
            || self
                .order
                .smart_retry_at
                .is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        let waiting = self
            .queue
            .iter()
            .filter(|track| self.order.smart_picks.contains(&track.id))
            .count();
        if waiting >= SMART_AHEAD {
            return;
        }
        let Some(seed) = Rng::from_time().pick(&self.order.context).cloned() else {
            return;
        };
        let sources: Vec<String> = mix_sources(&seed).into_iter().take(2).collect();
        let extractor = self.extractor.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut last = Err("no recommendations were found".to_string());
            for source in sources {
                match extractor.list(&source, Some(25)) {
                    Ok(tracks) if !tracks.is_empty() => {
                        last = Ok(tracks);
                        break;
                    }
                    Ok(_) => {}
                    Err(error) => last = Err(error),
                }
            }
            let _ = sender.send(last);
        });
        self.order.smart_rx = Some(receiver);
    }

    fn poll_smart(&mut self) {
        let Some(receiver) = &self.order.smart_rx else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("stopped".into()),
        };
        self.order.smart_rx = None;
        if self.settings.shuffle != ShuffleMode::Smart {
            return;
        }
        let added = match result {
            Ok(found) => self.mix_in_smart_picks(found),
            Err(_) => 0,
        };
        if added == 0 {
            self.order.smart_retry_at = Some(Instant::now() + SMART_RETRY);
        }
    }

    /// Spread recommended songs through the queue: one after every few of
    /// the user's songs. Returns how many went in.
    pub fn mix_in_smart_picks(&mut self, found: Vec<Track>) -> usize {
        let mut exclusions = self.mix_exclusions();
        for track in &self.order.context {
            exclusions.add(track);
        }
        let mut found = found;
        Rng::from_time().shuffle(&mut found);
        let picks = select_mix_items(found, &mut exclusions, SMART_AHEAD);
        let mut added = 0;
        for track in picks {
            // After the last recommendation (or the start), skip a few songs.
            let last_pick = self
                .queue
                .iter()
                .enumerate()
                .filter(|(_, queued)| self.order.smart_picks.contains(&queued.id))
                .map(|(index, _)| index + 1)
                .last()
                .unwrap_or(0);
            let at = last_pick + SMART_EVERY;
            self.order.smart_picks.insert(track.id.clone());
            if self.queue.insert_unique(at, track) {
                added += 1;
            }
        }
        added
    }

    /// A recommended song Smart Shuffle added.
    pub fn is_smart_pick(&self, id: &str) -> bool {
        self.order.smart_picks.contains(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: format!("Song {id}"),
            id: id.into(),
            duration: Some(200.0),
            channel: format!("Artist {id}"),
        }
    }

    fn ids(engine: &Engine) -> Vec<String> {
        engine.queue.iter().map(|t| t.id.clone()).collect()
    }

    #[test]
    fn shuffle_keeps_the_songs_and_off_restores_their_order() {
        let mut engine = Engine::default();
        let songs: Vec<String> = (0..12).map(|i| format!("s{i}")).collect();
        for id in &songs {
            engine.queue.push_back(track(id));
        }
        engine.set_shuffle(ShuffleMode::On);
        let mut shuffled = ids(&engine);
        assert_ne!(shuffled, songs, "12 songs come out in another order");
        shuffled.sort();
        let mut sorted = songs.clone();
        sorted.sort();
        assert_eq!(shuffled, sorted);
        engine.set_shuffle(ShuffleMode::Off);
        assert_eq!(ids(&engine), songs);
    }

    #[test]
    fn repeat_all_starts_the_playlist_over() {
        let mut engine = Engine::default();
        engine.start_playlist_context(vec![track("a"), track("b")]);
        assert!(!engine.repeat_context(), "repeat is off");
        engine.set_repeat(RepeatMode::All);
        assert!(engine.repeat_context());
        assert_eq!(ids(&engine), ["a", "b"]);
    }

    #[test]
    fn songs_played_outside_a_playlist_are_what_repeats() {
        let mut engine = Engine::default();
        engine.start_song_context(&track("a"));
        engine.note_played(&track("b"));
        engine.note_played(&track("a"));
        assert_eq!(engine.order.context.len(), 2);
        engine.start_playlist_context(vec![track("p")]);
        engine.note_played(&track("x"));
        assert_eq!(engine.order.context.len(), 1, "a playlist doesn't grow");
    }

    #[test]
    fn smart_picks_are_spread_out_and_leave_with_smart_shuffle() {
        let mut engine = Engine::default();
        for i in 0..8 {
            engine.queue.push_back(track(&format!("mine{i}")));
        }
        engine.start_song_context(&track("seed"));
        engine.set_shuffle(ShuffleMode::Smart);
        let added = engine.mix_in_smart_picks(vec![track("rec1"), track("rec2"), track("rec3")]);
        assert_eq!(added, SMART_AHEAD);
        let positions: Vec<usize> = ids(&engine)
            .iter()
            .enumerate()
            .filter(|(_, id)| id.starts_with("rec"))
            .map(|(index, _)| index)
            .collect();
        assert_eq!(positions, [3, 7]);
        engine.set_shuffle(ShuffleMode::On);
        assert!(ids(&engine).iter().all(|id| !id.starts_with("rec")));
        assert_eq!(engine.queue.len(), 8);
    }
}
