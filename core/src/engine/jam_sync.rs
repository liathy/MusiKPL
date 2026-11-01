//! Application-side Jam coordination: publishing the host's playback,
//! following it as a guest, and translating Jam panel actions.

use std::time::{Duration, Instant};

use crate::{
    domain::Track,
    jam::{
        self, DriftAction, JamEvent, LoadStatus, PlaybackSnapshot, PlaybackState, RequestStatus,
        Role, SyncReason, TrackRef, SHARED_QUEUE,
    },
};

use super::{errors::AppError, Engine};

/// How far ahead of the host a guest preloads a track. The stream is opened
/// paused at that point and released when the host reaches it, so loading
/// time does not turn into an offset.
const PRELOAD_LEAD_SECS: f64 = 2.5;
/// A preloaded start is released this close to its target.
const START_TOLERANCE_SECS: f64 = 0.02;
/// Time a stream seek needs to rebuffer before drift is judged again.
const SEEK_SETTLE: Duration = Duration::from_secs(2);
/// A second seek within this window is treated as a follow-up to the first.
const SEEK_FOLLOW_UP: Duration = Duration::from_secs(10);
const MAX_SEEK_LEAD_SECS: f64 = 3.0;
/// Seeks in a row that may fail to bring a guest into range before it stops
/// trying; a stream that cannot be positioned must not be seeked forever.
const MAX_SEEK_ATTEMPTS: u32 = 3;
const LOAD_RETRY: Duration = Duration::from_secs(8);
pub const NOTICE_DURATION: Duration = Duration::from_secs(8);
/// Longest a host waits for guests to load a new track before starting
/// anyway; resolving and opening a stream normally takes a few seconds.
const HOLD_TIMEOUT: Duration = Duration::from_secs(12);

/// Guest-side synchronization state for the current session.
#[derive(Default)]
pub struct GuestSync {
    /// Video being resolved for the host's current track.
    pub loading_id: Option<String>,
    /// Position a preloaded, paused stream is waiting to start from.
    pub armed_start: Option<f64>,
    pub last_seek: Option<Instant>,
    /// Consecutive seeks that have not yet brought playback into range.
    pub seek_attempts: u32,
    /// Learned head start that offsets this connection's rebuffer time.
    pub seek_lead: f64,
    pub failed: Option<(String, Instant)>,
    pub applied_revision: u64,
    /// Track this guest already told the host it has loaded.
    pub ready_reported: Option<String>,
    pub status: String,
}

pub fn track_from(track: &TrackRef) -> Track {
    Track {
        title: track.title.clone(),
        id: track.video_id.clone(),
        duration: track.duration_secs,
        channel: track.artist.clone(),
    }
}

fn track_ref(track: &Track) -> Option<TrackRef> {
    jam::track_ref(&track.id, &track.title, &track.channel, track.duration)
}

impl Engine {
    pub fn is_guest(&self) -> bool {
        self.jam.info.role == Some(Role::Guest)
    }

    pub fn poll_jam(&mut self) {
        for event in self.jam.poll() {
            match event {
                JamEvent::Snapshot(_) => {}
                JamEvent::SessionClosed => self.release_guest_playback(),
                JamEvent::Notice(text) => self.set_jam_notice(text),
            }
        }
        match self.jam.info.role {
            Some(Role::Host) => {
                self.publish_host_playback();
                self.share_host_queue();
            }
            Some(Role::Guest) => self.follow_host(),
            None => {}
        }
    }

    pub fn set_jam_notice(&mut self, text: String) {
        self.jam_notice = Some((text, Instant::now()));
    }

    /// Hand playback back to the local user when a guest session ends.
    pub fn release_guest_playback(&mut self) {
        self.player.set_speed(1.0);
        if self.jam_sync.armed_start.is_some() && self.current.is_some() {
            self.player.set_paused(false);
        }
        self.jam_sync = GuestSync::default();
    }

    /// Host: describe actual playback. The controller turns changes into
    /// snapshots, so every control path is covered without touching each one.
    fn publish_host_playback(&mut self) {
        self.release_hold_when_guests_are_ready();
        let state = match self.current.as_ref().and_then(track_ref) {
            // While the next stream resolves nothing is audible on the host,
            // but guests are told what is coming so they load it in parallel.
            _ if self.loading => PlaybackState {
                track: self.pending_track.as_ref().and_then(track_ref),
                position_secs: 0.0,
                playing: false,
                speed: 1.0,
            },
            Some(track) => {
                let finished = self.player.is_finished();
                let position = self.player.position_seconds();
                PlaybackState {
                    position_secs: track
                        .duration_secs
                        .map_or(position, |duration| position.min(duration)),
                    playing: !self.player.paused && self.player.has_started() && !finished,
                    speed: 1.0,
                    track: Some(track),
                }
            }
            None => PlaybackState {
                track: None,
                position_secs: 0.0,
                playing: false,
                speed: 1.0,
            },
        };
        self.jam.publish(state);

        // A request popped for loading is briefly in neither place.
        if !self.loading {
            let current = self.current.as_ref().map(|track| track.id.as_str());
            let queue = &self.queue;
            self.jam
                .reconcile_requests(current, |id| queue.contains_id(id));
        }
    }

    /// Host: whether a track about to start should wait for guests.
    pub fn should_hold_for_guests(&self) -> bool {
        self.jam.info.role == Some(Role::Host) && self.jam.info.peers > 0
    }

    /// Host: start a held track once every guest has it loaded, or the wait
    /// has gone on long enough.
    fn release_hold_when_guests_are_ready(&mut self) {
        let Some(since) = self.jam_hold else {
            return;
        };
        let ready = self
            .current
            .as_ref()
            .is_some_and(|track| self.jam.guests_ready(&track.id));
        if self.player.has_started() && (ready || since.elapsed() > HOLD_TIMEOUT) {
            self.jam_hold = None;
            self.player.set_paused(false);
        }
    }

    /// Guest: load, start, and keep the host's track aligned.
    fn follow_host(&mut self) {
        if self.jam.info.connecting {
            return;
        }
        let Some(snapshot) = self.jam.latest_snapshot().cloned() else {
            self.jam_sync.status = "Waiting for the host…".into();
            return;
        };
        let Some(track) = snapshot.track.clone() else {
            if self.current.is_some() || self.loading {
                self.player.stop();
                self.current = None;
                self.play_rx = None;
                self.loading = false;
            }
            self.jam_sync.loading_id = None;
            self.jam_sync.armed_start = None;
            self.jam_sync.status = "The host isn't playing anything right now".into();
            return;
        };

        let id = track.video_id.as_str();
        if self.loading && self.jam_sync.loading_id.as_deref() == Some(id) {
            self.jam_sync.status = "Loading the host's track…".into();
            return;
        }
        let have_track =
            !self.loading && self.current.as_ref().map(|current| current.id.as_str()) == Some(id);
        if !have_track {
            self.load_host_track(&snapshot, &track);
            return;
        }
        if !self.player.has_started() {
            self.jam_sync.status = "Buffering…".into();
            return;
        }
        // The stream is open and positioned: the host may be waiting on this.
        if self.jam_sync.ready_reported.as_deref() != Some(id) {
            self.jam
                .report_load(snapshot.revision, id, LoadStatus::Ready);
            self.jam_sync.ready_reported = Some(id.to_owned());
        }
        self.align_with_host(&snapshot, &track);
    }

    fn load_host_track(&mut self, snapshot: &PlaybackSnapshot, track: &TrackRef) {
        if let Some((failed_id, at)) = &self.jam_sync.failed {
            if *failed_id == track.video_id && at.elapsed() < LOAD_RETRY {
                self.jam_sync.status = "Couldn't load this track here. Retrying…".into();
                return;
            }
        }
        self.jam_sync.loading_id = Some(track.video_id.clone());
        self.jam_sync.armed_start = None;
        self.jam_sync.last_seek = None;
        self.jam_sync.seek_attempts = 0;
        // Never keep showing or playing a different track as if it were synced.
        self.current = None;
        self.start_track(track_from(track), false);
        self.jam
            .report_load(snapshot.revision, &track.video_id, LoadStatus::Loading);
        self.jam_sync.status = "Loading the host's track…".into();
    }

    /// Guest: the stream URL for the host's track resolved; open it paused
    /// just ahead of the host so it can start exactly on time.
    pub fn finish_guest_load(&mut self, track: Track, url: &str) {
        self.jam_sync.loading_id = None;
        let Some(snapshot) = self.jam.latest_snapshot().cloned() else {
            return;
        };
        let still_current = snapshot
            .track
            .as_ref()
            .is_some_and(|host_track| host_track.video_id == track.id);
        if !still_current {
            // The host moved on while this resolved; the next frame follows.
            return;
        }
        let expected = jam::expected_position(&snapshot, self.jam.host_now_ms());
        let start = if snapshot.playing {
            expected + PRELOAD_LEAD_SECS
        } else {
            expected
        };
        match self.player.play_stream_at(&track, url, start, true) {
            Ok(()) => {
                self.current = Some(track);
                self.jam_sync.armed_start = snapshot.playing.then_some(start);
                self.jam_sync.failed = None;
                self.error = None;
                self.finished_handled = false;
            }
            Err(error) => self.fail_guest_load(Some(track.id), error),
        }
    }

    /// Guest: the host's track could not be resolved or played here.
    pub fn fail_guest_load(&mut self, id: Option<String>, error: String) {
        let Some(id) = id.or_else(|| self.jam_sync.loading_id.take()) else {
            return;
        };
        self.jam_sync.loading_id = None;
        self.current = None;
        let revision = self
            .jam
            .latest_snapshot()
            .map_or(0, |snapshot| snapshot.revision);
        self.jam.report_load(revision, &id, LoadStatus::Failed);
        self.jam_sync.failed = Some((id, Instant::now()));
        self.error = Some(AppError::Jam(format!(
            "this device couldn't load the host's current track ({error}). It will retry, and rejoin at the next track."
        )));
    }

    fn align_with_host(&mut self, snapshot: &PlaybackSnapshot, track: &TrackRef) {
        let expected = jam::expected_position(snapshot, self.jam.host_now_ms());
        let fresh = snapshot.revision != self.jam_sync.applied_revision;
        self.jam_sync.applied_revision = snapshot.revision;
        let settled = !self
            .jam_sync
            .last_seek
            .is_some_and(|at| at.elapsed() < SEEK_SETTLE);
        let jumped = fresh
            && matches!(
                snapshot.reason,
                SyncReason::Seeked | SyncReason::TrackChanged
            );

        if !snapshot.playing {
            self.jam_sync.armed_start = None;
            self.player.set_speed(1.0);
            if !self.player.paused {
                self.player.set_paused(true);
            }
            // Pausing a few milliseconds apart leaves a small offset that the
            // speed nudge removes after resuming; only a real gap, such as a
            // seek made while paused, is worth a seek of its own.
            let far = matches!(
                jam::drift_action(self.player.position_seconds(), expected),
                DriftAction::Seek(_)
            );
            if far && (jumped || (settled && self.may_seek())) {
                self.seek_to_host(expected, jumped);
            }
            self.jam_sync.status = if snapshot.reason == SyncReason::TrackChanged {
                "Ready. Starting together with the host…".into()
            } else {
                "Paused by the host".into()
            };
            return;
        }

        let ending = track
            .duration_secs
            .is_some_and(|duration| expected >= duration - 1.0);
        if self.player.is_finished() {
            if !ending {
                // The local player stopped early; load the track again.
                self.fail_guest_load(Some(track.video_id.clone()), "playback stopped".into());
            }
            return;
        }

        if let Some(start) = self.jam_sync.armed_start {
            let wait = start - expected;
            // Still ahead of the host: hold the preloaded stream. A much
            // longer wait means the host jumped back, so fall through.
            if wait > START_TOLERANCE_SECS && wait < PRELOAD_LEAD_SECS + 1.0 {
                self.jam_sync.status = "Starting in sync…".into();
                return;
            }
            self.jam_sync.armed_start = None;
        }
        if self.player.paused {
            self.player.set_paused(false);
        }
        if ending {
            self.player.set_speed(1.0);
            self.jam_sync.status = "In sync".into();
            return;
        }

        let local = self.player.position_seconds();
        match jam::drift_action(local, expected) {
            DriftAction::None => {
                self.jam_sync.seek_attempts = 0;
                self.player.set_speed(1.0);
                self.jam_sync.status = "In sync".into();
            }
            DriftAction::Nudge(drift) => {
                self.jam_sync.seek_attempts = 0;
                if settled {
                    self.player.set_speed(jam::nudge_speed(drift));
                }
                self.jam_sync.status = format!("Syncing ({:+.0} ms)", drift * 1000.0);
            }
            DriftAction::Seek(_) if !jumped && !self.may_seek() => {
                self.player.set_speed(1.0);
                self.jam_sync.status =
                    "Can't line this stream up exactly. It will resync at the next track.".into();
            }
            DriftAction::Seek(target) => {
                if settled || jumped {
                    let follow_up = self
                        .jam_sync
                        .last_seek
                        .is_some_and(|at| at.elapsed() < SEEK_FOLLOW_UP);
                    if follow_up && !jumped {
                        // The last seek landed late by the rebuffer time;
                        // aim that much further ahead from now on.
                        self.jam_sync.seek_lead = (self.jam_sync.seek_lead + (expected - local))
                            .clamp(0.0, MAX_SEEK_LEAD_SECS);
                    }
                    self.player.set_speed(1.0);
                    self.seek_to_host(target + self.jam_sync.seek_lead, jumped);
                }
                self.jam_sync.status = "Catching up with the host…".into();
            }
        }
    }

    fn may_seek(&self) -> bool {
        self.jam_sync.seek_attempts < MAX_SEEK_ATTEMPTS
    }

    /// Seek the guest's player; a jump on the host starts a fresh run of
    /// attempts, a correction counts against the current one.
    fn seek_to_host(&mut self, position: f64, jumped: bool) {
        self.jam_sync.seek_attempts = if jumped {
            1
        } else {
            self.jam_sync.seek_attempts + 1
        };
        self.player.seek(position);
        self.jam_sync.last_seek = Some(Instant::now());
    }
    /// Host: listeners add songs straight to the queue (their requests are
    /// accepted as they arrive), and everyone sees the same queue.
    fn share_host_queue(&mut self) {
        let pending: Vec<u64> = self
            .jam
            .requests()
            .iter()
            .filter(|entry| entry.status == RequestStatus::Pending)
            .map(|entry| entry.id)
            .collect();
        for id in pending {
            if let Some(track) = self.jam.accept_request(id) {
                self.add_to_queue(track_from(&track));
            }
        }
        let queue = self
            .queue
            .iter()
            .take(SHARED_QUEUE)
            .filter_map(track_ref)
            .collect();
        self.jam.share_queue(queue);
    }

    /// Guest: the host's queue, as the host sees it.
    pub fn jam_queue(&self) -> Vec<Track> {
        self.jam.queue().iter().map(track_from).collect()
    }

    /// Guest: add a track to the host's queue instead of playing it locally.
    pub fn request_from_host(&mut self, track: &Track) {
        let result = track_ref(track)
            .ok_or_else(|| "That track can't be added".to_string())
            .and_then(|track| self.jam.request_track(track));
        self.set_jam_notice(match result {
            Ok(()) => format!("Added \"{}\" to the Jam queue", track.title),
            Err(error) => error,
        });
    }
}
