//! Application-side Jam coordination: publishing the host's playback,
//! following it as a guest, and translating Jam panel actions.

use std::time::{Duration, Instant};

use eframe::egui;

use crate::{
    domain::Track,
    jam::{
        self, DiscoveryStatus, DriftAction, JamEvent, LoadStatus, PlaybackSnapshot, PlaybackState,
        Role, SyncReason, TrackRef,
    },
    ui::{
        jam::{self as jam_ui, DiscoveryView, JamAction, JamView, NearbyView, RequestView},
        responsive::ResponsiveUi,
    },
};

use super::{errors::AppError, YtMusicApp};

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
const NOTICE_DURATION: Duration = Duration::from_secs(8);
/// Longest a host waits for guests to load a new track before starting
/// anyway; resolving and opening a stream normally takes a few seconds.
const HOLD_TIMEOUT: Duration = Duration::from_secs(12);

/// Guest-side synchronization state for the current session.
#[derive(Default)]
pub(super) struct GuestSync {
    /// Video being resolved for the host's current track.
    loading_id: Option<String>,
    /// Position a preloaded, paused stream is waiting to start from.
    armed_start: Option<f64>,
    last_seek: Option<Instant>,
    /// Consecutive seeks that have not yet brought playback into range.
    seek_attempts: u32,
    /// Learned head start that offsets this connection's rebuffer time.
    seek_lead: f64,
    failed: Option<(String, Instant)>,
    applied_revision: u64,
    /// Track this guest already told the host it has loaded.
    ready_reported: Option<String>,
    status: String,
}

fn track_from(track: &TrackRef) -> Track {
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

impl YtMusicApp {
    pub(super) fn is_guest(&self) -> bool {
        self.jam.info.role == Some(Role::Guest)
    }

    pub(super) fn poll_jam(&mut self) {
        for event in self.jam.poll() {
            match event {
                JamEvent::Snapshot(_) => {}
                JamEvent::SessionClosed => self.release_guest_playback(),
                JamEvent::Notice(text) => self.set_jam_notice(text),
            }
        }
        match self.jam.info.role {
            Some(Role::Host) => self.publish_host_playback(),
            Some(Role::Guest) => self.follow_host(),
            None => {}
        }
    }

    fn set_jam_notice(&mut self, text: String) {
        self.jam_notice = Some((text, Instant::now()));
    }

    /// Hand playback back to the local user when a guest session ends.
    fn release_guest_playback(&mut self) {
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
    pub(super) fn should_hold_for_guests(&self) -> bool {
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
        self.start_track(track_from(track), false, false);
        self.jam
            .report_load(snapshot.revision, &track.video_id, LoadStatus::Loading);
        self.jam_sync.status = "Loading the host's track…".into();
    }

    /// Guest: the stream URL for the host's track resolved; open it paused
    /// just ahead of the host so it can start exactly on time.
    pub(super) fn finish_guest_load(&mut self, track: Track, url: &str) {
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
        match self
            .player
            .play_stream_at(&track, url, &self.tools.mpv, start, true)
        {
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
    pub(super) fn fail_guest_load(&mut self, id: Option<String>, error: String) {
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
    /// Guest: suggest a track to the host instead of playing it locally.
    pub(super) fn request_from_host(&mut self, track: &Track) {
        let result = track_ref(track)
            .ok_or_else(|| "That track can't be requested".to_string())
            .and_then(|track| self.jam.request_track(track));
        self.set_jam_notice(match result {
            Ok(()) => format!("Requested \"{}\"", track.title),
            Err(error) => error,
        });
    }

    pub(super) fn show_jam_panel(&mut self, ui: &mut egui::Ui, responsive: &ResponsiveUi) {
        if self
            .jam_notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > NOTICE_DURATION)
        {
            self.jam_notice = None;
        }
        let nearby_jams = self.jam.nearby();
        let nearby: Vec<NearbyView> = nearby_jams
            .iter()
            .map(|jam| NearbyView {
                session_id: jam.session_id,
                host: jam.host_name.clone(),
                track: jam.track.clone(),
                listeners: jam.listeners,
            })
            .collect();
        let discovery = match self.jam.discovery_status() {
            DiscoveryStatus::Searching => DiscoveryView::Searching,
            DiscoveryStatus::Ready => DiscoveryView::Ready,
            DiscoveryStatus::Unavailable(reason) => DiscoveryView::Unavailable(reason),
        };
        let me = self.jam.participant_id();
        let requests: Vec<RequestView> = self
            .jam
            .requests()
            .iter()
            .rev()
            .map(|entry| RequestView {
                id: entry.id,
                title: entry.track.title.clone(),
                requester: entry.requester.clone(),
                status: entry.status,
                mine: self.is_guest() && me == Some(entry.requester_id),
            })
            .collect();
        let info = &self.jam.info;
        let hosting = info.role == Some(Role::Host);
        let view = JamView {
            state: jam_ui::state(info.role, info.connecting, info.peers),
            host_code: &info.token,
            host_name: &info.host_name,
            status: if self.is_guest() && !info.connecting {
                &self.jam_sync.status
            } else {
                &info.status
            },
            notice: self
                .jam_notice
                .as_ref()
                .map(|(text, _)| text.as_str())
                .or(info.notice.as_deref()),
            manual_address: (hosting && !info.discoverable).then_some(info.address.as_str()),
            waiting_for_guests: hosting && self.jam_hold.is_some(),
            discovery: &discovery,
            nearby: &nearby,
            requests: &requests,
        };
        let actions = jam_ui::show(
            ui,
            &view,
            &mut self.jam_join_address,
            &mut self.jam_join_token,
            self.jam_show_code_join,
            responsive,
        );

        for action in actions {
            match action {
                JamAction::Start => {
                    self.jam_notice = None;
                    if let Err(error) = self.jam.host() {
                        self.error = Some(AppError::Jam(error));
                    }
                }
                JamAction::JoinNearby(session_id) => {
                    if let Some(jam) = nearby_jams.iter().find(|jam| jam.session_id == session_id) {
                        self.jam_notice = None;
                        self.error = None;
                        self.jam.join_nearby(jam);
                    }
                }
                JamAction::ToggleCodeJoin => {
                    self.jam_show_code_join = !self.jam_show_code_join;
                }
                JamAction::Join => {
                    self.jam_notice = None;
                    match self.jam.join(&self.jam_join_address, &self.jam_join_token) {
                        Ok(()) => self.error = None,
                        Err(error) => self.set_jam_notice(error),
                    }
                }
                JamAction::End => {
                    if self.is_guest() {
                        self.release_guest_playback();
                    }
                    self.jam_notice = None;
                    self.jam.leave();
                }
                // Request decisions change the host's own playback queue;
                // the controller refuses them unless this device is the host.
                JamAction::AcceptRequest(id) => {
                    if let Some(track) = self.jam.accept_request(id) {
                        self.queue.enqueue_unique(track_from(&track));
                    }
                }
                JamAction::RejectRequest(id) => self.jam.reject_request(id),
                JamAction::RemoveRequest(id) => {
                    if let Some(track) = self.jam.remove_request(id) {
                        self.queue.remove_id(&track.video_id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::footer::FooterAction;
    use std::{
        path::{Path, PathBuf},
        thread::sleep,
    };

    /// A generated tone: no network, no yt-dlp, any length.
    const TONE: &str = "av://lavfi:sine=frequency=440:duration=600";

    fn track(id: &str) -> Track {
        Track {
            title: format!("Tone {id}"),
            id: id.into(),
            duration: Some(600.0),
            channel: "Test".into(),
        }
    }

    fn app(mpv: &Path) -> YtMusicApp {
        let mut app = YtMusicApp::default();
        app.tools.mpv = mpv.to_path_buf();
        // Resolution is replaced below; make sure nothing reaches the network.
        app.tools.yt_dlp = PathBuf::from("missing-yt-dlp-for-tests");
        app.player.volume = 0;
        app
    }

    /// One frame of both apps. The guest's stream "resolves" instantly to the
    /// same tone the host plays, standing in for yt-dlp.
    fn frame(host: &mut YtMusicApp, guest: &mut YtMusicApp) {
        for app in [&mut *host, &mut *guest] {
            app.player.refresh_position();
            app.poll_jam();
        }
        if guest.loading {
            let id = guest
                .jam_sync
                .loading_id
                .clone()
                .expect("guest loads the host's track");
            guest.play_rx = None;
            guest.loading = false;
            guest.finish_guest_load(track(&id), TONE);
        }
        sleep(Duration::from_millis(20));
    }

    fn run(host: &mut YtMusicApp, guest: &mut YtMusicApp, seconds: f64) {
        let end = Instant::now() + Duration::from_secs_f64(seconds);
        while Instant::now() < end {
            frame(host, guest);
        }
    }

    fn offset(host: &YtMusicApp, guest: &YtMusicApp) -> f64 {
        guest.player.position_seconds() - host.player.position_seconds()
    }

    #[test]
    #[ignore = "plays a muted tone through the real mpv.exe next to Cargo.toml; run with --ignored"]
    fn guest_follows_the_host_through_join_seek_pause_and_track_change() {
        let mpv = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("mpv.exe");
        assert!(mpv.exists(), "mpv.exe is required for this test");

        // The host has been playing for a while before anyone joins.
        let mut host = app(&mpv);
        let first = track("first");
        host.current = Some(first.clone());
        host.player
            .play_stream_at(&first, TONE, &mpv, 40.0, false)
            .unwrap();
        let address = host.jam.host_on_loopback();
        let mut guest = app(&mpv);
        run(&mut host, &mut guest, 2.0);
        assert!(host.player.has_started());

        let token = host.jam.info.token.clone();
        guest.jam.join_for_test(address, &token);
        run(&mut host, &mut guest, 7.0);
        println!(
            "after join: host {:.3}, guest offset {:+.3}, status {:?}",
            host.player.position_seconds(),
            offset(&host, &guest),
            guest.jam_sync.status
        );
        assert_eq!(guest.current.as_ref().map(|t| t.id.as_str()), Some("first"));
        assert!(
            host.player.position_seconds() > 45.0,
            "guest did not restart the host"
        );
        assert!(
            offset(&host, &guest).abs() < 0.1,
            "joined mid-track out of sync"
        );

        // The guest's own controls cannot move playback for anyone.
        let before = guest.player.position_seconds();
        super::super::actions::apply_footer_action(&mut guest, FooterAction::Seek(300.0));
        super::super::actions::apply_footer_action(&mut guest, FooterAction::TogglePause);
        guest.next();
        assert!(!guest.player.paused);
        assert!((guest.player.position_seconds() - before).abs() < 0.2);

        // Host seeks.
        super::super::actions::apply_footer_action(&mut host, FooterAction::Seek(200.0));
        run(&mut host, &mut guest, 6.0);
        println!(
            "after seek: host {:.3}, guest offset {:+.3}",
            host.player.position_seconds(),
            offset(&host, &guest)
        );
        assert!(host.player.position_seconds() > 200.0);
        assert!(offset(&host, &guest).abs() < 0.1, "seek did not propagate");

        // Host pauses, then resumes.
        super::super::actions::apply_footer_action(&mut host, FooterAction::TogglePause);
        run(&mut host, &mut guest, 2.5);
        println!(
            "paused: host {:.3}, guest offset {:+.3}, guest paused {}",
            host.player.position_seconds(),
            offset(&host, &guest),
            guest.player.paused
        );
        assert!(guest.player.paused, "pause did not propagate");
        assert!(offset(&host, &guest).abs() < 0.1);
        super::super::actions::apply_footer_action(&mut host, FooterAction::TogglePause);
        run(&mut host, &mut guest, 5.0);
        println!(
            "resumed: host {:.3}, guest offset {:+.3}",
            host.player.position_seconds(),
            offset(&host, &guest)
        );
        assert!(!guest.player.paused, "resume did not propagate");
        assert!(offset(&host, &guest).abs() < 0.1);

        // Host changes track: it waits at 0:00 until the guest has loaded
        // the track, so both hear it from the very start.
        let second = track("second");
        host.current = Some(second.clone());
        assert!(host.should_hold_for_guests());
        host.player
            .play_stream_at(&second, TONE, &mpv, 0.0, true)
            .unwrap();
        host.jam_hold = Some(Instant::now());
        let waiting = Instant::now();
        while host.player.paused {
            frame(&mut host, &mut guest);
            assert!(
                waiting.elapsed() < Duration::from_secs(10),
                "host never started"
            );
        }
        println!(
            "start together: waited {:?}, host {:.3}, guest {:.3}",
            waiting.elapsed(),
            host.player.position_seconds(),
            guest.player.position_seconds()
        );
        assert_eq!(
            guest.current.as_ref().map(|t| t.id.as_str()),
            Some("second")
        );
        assert!(
            guest.player.has_started(),
            "host started before the guest was ready"
        );
        assert!(
            guest.player.position_seconds() < 0.1,
            "guest missed the start"
        );
        assert!(host.player.position_seconds() < 0.1);
        run(&mut host, &mut guest, 6.0);
        println!(
            "new track: host {:.3}, guest offset {:+.3}",
            host.player.position_seconds(),
            offset(&host, &guest)
        );
        assert!(
            offset(&host, &guest).abs() < 0.1,
            "track change out of sync"
        );
        // Host ends the Jam: the guest is released and keeps its controls.
        host.jam.leave();
        run(&mut host, &mut guest, 1.0);
        assert!(!guest.is_guest());
        assert_eq!(
            guest.jam.info.notice.as_deref(),
            Some("The host ended the Jam.")
        );
        host.player.stop();
        guest.player.stop();
    }
}
