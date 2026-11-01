//! The Jam section of the menu: what it shows, and its buttons. The Jam
//! itself (hosting, joining, staying in sync) is the engine's.

use musikpl_core::{
    engine::{track_from, AppError, NOTICE_DURATION},
    jam::{DiscoveryStatus, NearbyJam, Progress, Role},
};

use crate::ui::jam::{
    self as jam_ui, DiscoveryView, InternetView, JamAction, JamView, NearbyView, RequestView,
};

use super::YtMusicApp;

impl YtMusicApp {
    /// Ask Windows (with its permission prompt) to let MusiKPL through the
    /// firewall, without blocking the interface while the prompt is open.
    fn allow_through_firewall(&mut self) {
        if self.firewall_rx.is_some() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.firewall_rx = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(crate::infrastructure::system::allow_through_firewall());
        });
    }

    pub(super) fn poll_firewall(&mut self) {
        let Some(receiver) = &self.firewall_rx else {
            return;
        };
        let result = match receiver.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("The firewall change stopped unexpectedly".into())
            }
        };
        self.firewall_rx = None;
        self.set_jam_notice(match result {
            Ok(()) => "MusiKPL is allowed through Windows Firewall. Do the same on the other PC if its Jams still don't appear.".into(),
            Err(error) => error,
        });
    }

    /// Snapshot what the Jam section of the menu shows this frame.
    pub fn jam_panel_data(&mut self) -> JamPanelData {
        if self
            .jam_notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() > NOTICE_DURATION)
        {
            self.jam_notice = None;
        }
        let nearby_jams = self.jam.nearby();
        let nearby = nearby_jams
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
        let requests = self
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
        JamPanelData {
            internet: self.jam.internet_status(),
            nearby_jams,
            nearby,
            discovery,
            requests,
        }
    }

    pub fn jam_view<'a>(&'a self, data: &'a JamPanelData) -> JamView<'a> {
        let info = &self.jam.info;
        let hosting = info.role == Some(Role::Host);
        JamView {
            state: jam_ui::state(info.role, info.connecting, info.peers),
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
            undiscoverable: hosting && !info.discoverable,
            waiting_for_guests: hosting && self.jam_hold.is_some(),
            firewall_pending: self.firewall_rx.is_some(),
            internet: match &data.internet {
                None => InternetView::Off,
                Some(Progress::Preparing) => InternetView::Preparing,
                Some(Progress::Ready(invite)) => InternetView::Ready(invite),
                Some(Progress::Failed(reason)) => InternetView::Failed(reason),
            },
            discovery: &data.discovery,
            nearby: &data.nearby,
            requests: &data.requests,
        }
    }

    pub fn apply_jam_action(&mut self, action: JamAction, data: &JamPanelData) {
        match action {
            JamAction::Start => {
                self.jam_notice = None;
                if let Err(error) = self.jam.host() {
                    self.error = Some(AppError::Jam(error));
                }
            }
            JamAction::JoinNearby(session_id) => {
                if let Some(jam) = data
                    .nearby_jams
                    .iter()
                    .find(|jam| jam.session_id == session_id)
                {
                    self.jam_notice = None;
                    self.error = None;
                    self.jam.join_nearby(jam);
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
                    self.add_to_queue(track_from(&track));
                }
            }
            JamAction::RejectRequest(id) => self.jam.reject_request(id),
            JamAction::AllowFirewall => self.allow_through_firewall(),
            JamAction::ShareOverInternet => self.jam.share_over_internet(),
            JamAction::JoinInvite => {
                match self.engine.jam.join_invite(&self.engine.jam_invite_input) {
                    Ok(()) => {
                        self.jam_invite_input.clear();
                        self.jam_notice = None;
                        self.error = None;
                    }
                    Err(error) => self.set_jam_notice(error),
                }
            }
            JamAction::RemoveRequest(id) => {
                if let Some(track) = self.jam.remove_request(id) {
                    self.queue.remove_id(&track.video_id);
                }
            }
        }
    }
}

/// Owned data behind one frame of the Jam section.
pub struct JamPanelData {
    internet: Option<Progress<String>>,
    nearby_jams: Vec<NearbyJam>,
    nearby: Vec<NearbyView>,
    discovery: DiscoveryView,
    requests: Vec<RequestView>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::Track,
        infrastructure::mpv::MpvOutput,
        media::{Tools, YtDlp},
        ui::footer::FooterAction,
    };
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
        thread::sleep,
        time::{Duration, Instant},
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
        let mut app = YtMusicApp::new(musikpl_core::platform::quiet_waker());
        app.engine.player.set_output(Box::new(MpvOutput::new(
            mpv.to_path_buf(),
            app.registry.clone(),
        )));
        // Resolution is replaced below; make sure nothing reaches the network.
        let tools = Tools {
            yt_dlp: PathBuf::from("missing-yt-dlp-for-tests"),
            ..app.tools.clone()
        };
        app.engine.extractor = Arc::new(YtDlp::new(tools, app.registry.clone()));
        app.engine.player.volume = 0;
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
            .play_stream_at(&first, TONE, 40.0, false)
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
            .play_stream_at(&second, TONE, 0.0, true)
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
