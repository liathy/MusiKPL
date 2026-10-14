//! LAN Jam wire types, roles, permissions, and deterministic synchronization rules.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// Version 2 added discovery, snapshot reasons/speed, and guest requests.
pub(crate) const PROTOCOL_VERSION: u16 = 2;
pub(crate) const DISCOVERY_APP: &str = "musikpl-jam";
pub(crate) const MAX_NAME_CHARS: usize = 32;
pub(crate) const MAX_TEXT_CHARS: usize = 200;
/// Largest discovery datagram accepted; stays under a typical LAN MTU.
pub(crate) const MAX_DISCOVERY_BYTES: usize = 1_200;

/// Below this the ear cannot tell and corrections would only add jitter.
const DRIFT_DEADBAND_SECS: f64 = 0.050;
/// A stream seek costs a rebuffer, so only drift a speed nudge cannot remove
/// within a few seconds is worth one.
const DRIFT_SEEK_SECS: f64 = 0.500;
/// Largest speed change used to absorb drift; mpv keeps pitch constant.
const MAX_NUDGE: f64 = 0.08;
/// Position jump on the host, relative to its own last snapshot, that is
/// treated as a seek rather than timing noise.
const SEEK_DETECT_SECS: f64 = 0.75;
/// Idle interval between periodic snapshots; also the guests' keep-alive.
pub(crate) const PERIODIC_SNAPSHOT_MS: u64 = 1_000;
const CLOCK_SAMPLES: usize = 8;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct TrackRef {
    pub(crate) video_id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) duration_secs: Option<f64>,
}

/// Why the host published a snapshot.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum SyncReason {
    TrackChanged,
    Started,
    Paused,
    Seeked,
    Periodic,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct PlaybackSnapshot {
    pub(crate) revision: u64,
    pub(crate) reason: SyncReason,
    pub(crate) track: Option<TrackRef>,
    pub(crate) position_secs: f64,
    pub(crate) playing: bool,
    pub(crate) speed: f64,
    pub(crate) host_clock_ms: u64,
    pub(crate) scheduled_start_host_ms: Option<u64>,
}

/// The host's playback as observed locally, before it becomes a snapshot.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaybackState {
    pub(crate) track: Option<TrackRef>,
    pub(crate) position_secs: f64,
    pub(crate) playing: bool,
    pub(crate) speed: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum RequestStatus {
    Pending,
    Accepted,
    Played,
    Rejected,
    Removed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct RequestEntry {
    pub(crate) id: u64,
    pub(crate) track: TrackRef,
    pub(crate) requester_id: u64,
    pub(crate) requester: String,
    pub(crate) status: RequestStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) enum Message {
    Hello {
        version: u16,
        token: String,
        name: String,
    },
    Welcome {
        version: u16,
        participant_id: u64,
        host_name: String,
    },
    Ping {
        client_clock_ms: u64,
    },
    Pong {
        client_clock_ms: u64,
        host_clock_ms: u64,
    },
    RequestSnapshot,
    Snapshot(PlaybackSnapshot),
    TrackLoadStatus {
        revision: u64,
        video_id: String,
        status: LoadStatus,
    },
    /// Guest asks the host to consider a track. Never a playback command.
    TrackRequest {
        track: TrackRef,
    },
    /// Host-authoritative request list, sent to every guest when it changes.
    Requests(Vec<RequestEntry>),
    Leave,
    SessionEnded,
    Error {
        code: String,
        message: String,
    },
}

/// Error code for a declined track request; the session stays open.
pub(crate) const ERROR_REQUEST: &str = "request";
/// Error code for a message the sender's role may not send.
pub(crate) const ERROR_FORBIDDEN: &str = "forbidden";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum LoadStatus {
    Loading,
    Ready,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Host,
    Guest,
}

/// Session changes the application coordinator has to react to.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JamEvent {
    /// A newer authoritative snapshot was accepted (guest).
    Snapshot(PlaybackSnapshot),
    /// The guest session is over; the controller is already inactive.
    SessionClosed,
    /// Something the user should see, such as a declined request.
    Notice(String),
}

#[derive(Clone, Debug)]
pub(crate) struct JamInfo {
    pub(crate) role: Option<Role>,
    pub(crate) token: String,
    pub(crate) status: String,
    pub(crate) peers: usize,
    /// Guest: true until the host's welcome arrives.
    pub(crate) connecting: bool,
    /// Guest: the host's name. Host: this device's name.
    pub(crate) host_name: String,
    /// Host: `ip:port` for joining by code when discovery is unavailable.
    pub(crate) address: String,
    /// Host: whether nearby devices can find this Jam automatically.
    pub(crate) discoverable: bool,
    /// Last session outcome, shown while inactive.
    pub(crate) notice: Option<String>,
}

impl Default for JamInfo {
    fn default() -> Self {
        Self {
            role: None,
            token: String::new(),
            status: "Jam inactive".into(),
            peers: 0,
            connecting: false,
            host_name: String::new(),
            address: String::new(),
            discoverable: false,
            notice: None,
        }
    }
}

/// UDP discovery datagrams. Guests broadcast `Query`; hosts answer the sender
/// directly with `Announce`, so only hosts need an inbound firewall rule.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) enum DiscoveryPacket {
    Query {
        app: String,
        version: u16,
    },
    Announce {
        app: String,
        version: u16,
        session_id: u64,
        host_name: String,
        port: u16,
        token: String,
        track: Option<String>,
        listeners: u32,
    },
}

pub(crate) fn discovery_query() -> DiscoveryPacket {
    DiscoveryPacket::Query {
        app: DISCOVERY_APP.into(),
        version: PROTOCOL_VERSION,
    }
}

/// Parse an untrusted discovery datagram, keeping only compatible, sane ones.
pub(crate) fn decode_discovery(bytes: &[u8]) -> Option<DiscoveryPacket> {
    if bytes.len() > MAX_DISCOVERY_BYTES {
        return None;
    }
    match serde_json::from_slice(bytes).ok()? {
        DiscoveryPacket::Query { app, version }
            if app == DISCOVERY_APP && version == PROTOCOL_VERSION =>
        {
            Some(DiscoveryPacket::Query { app, version })
        }
        DiscoveryPacket::Announce {
            app,
            version,
            session_id,
            host_name,
            port,
            token,
            track,
            listeners,
        } if app == DISCOVERY_APP
            && version == PROTOCOL_VERSION
            && port != 0
            && valid_token(&token) =>
        {
            Some(DiscoveryPacket::Announce {
                app,
                version,
                session_id,
                host_name: sanitize_text(&host_name, MAX_NAME_CHARS),
                port,
                token,
                track: track
                    .map(|title| sanitize_text(&title, MAX_TEXT_CHARS))
                    .filter(|title| !title.is_empty()),
                listeners,
            })
        }
        _ => None,
    }
}

pub(crate) fn valid_token(token: &str) -> bool {
    token.len() == 6 && token.bytes().all(|byte| byte.is_ascii_digit())
}

/// Strip control characters and cap the length of text received from a peer.
pub(crate) fn sanitize_text(text: &str, max_chars: usize) -> String {
    text.chars()
        .filter(|character| !character.is_control())
        .take(max_chars)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Messages a guest connection is allowed to send. Everything else, notably
/// snapshots and session control, is host-only and must be refused.
pub(crate) fn guest_may_send(message: &Message) -> bool {
    matches!(
        message,
        Message::Hello { .. }
            | Message::Ping { .. }
            | Message::RequestSnapshot
            | Message::TrackLoadStatus { .. }
            | Message::TrackRequest { .. }
            | Message::Leave
    )
}

fn valid_video_id(id: &str) -> bool {
    id.len() <= 64
        && !id.is_empty()
        && id.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

pub(crate) fn track_ref(
    video_id: &str,
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> Option<TrackRef> {
    if !valid_video_id(video_id)
        || duration_secs.is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return None;
    }
    Some(TrackRef {
        video_id: video_id.into(),
        title: sanitize_text(title, MAX_TEXT_CHARS),
        artist: sanitize_text(artist, MAX_TEXT_CHARS),
        duration_secs,
    })
}

/// Re-validate a track received from the network. The video ID is the track
/// identity; titles are display-only and are sanitized, never trusted.
pub(crate) fn checked_track(track: &TrackRef) -> Option<TrackRef> {
    track_ref(
        &track.video_id,
        &track.title,
        &track.artist,
        track.duration_secs,
    )
}

/// Validate a snapshot received from the network.
pub(crate) fn checked_snapshot(snapshot: PlaybackSnapshot) -> Option<PlaybackSnapshot> {
    if !snapshot.position_secs.is_finite()
        || snapshot.position_secs < 0.0
        || !snapshot.speed.is_finite()
        || !(0.25..=4.0).contains(&snapshot.speed)
    {
        return None;
    }
    let track = match &snapshot.track {
        Some(track) => Some(checked_track(track)?),
        None => None,
    };
    Some(PlaybackSnapshot { track, ..snapshot })
}

pub(crate) fn expected_position(snapshot: &PlaybackSnapshot, host_now_ms: u64) -> f64 {
    let mut position = snapshot.position_secs;
    if snapshot.playing {
        let start = snapshot
            .scheduled_start_host_ms
            .unwrap_or(snapshot.host_clock_ms);
        if host_now_ms > start {
            position += (host_now_ms - start) as f64 / 1000.0 * snapshot.speed;
        }
    }
    snapshot
        .track
        .as_ref()
        .and_then(|track| track.duration_secs)
        .map_or(position.max(0.0), |duration| position.clamp(0.0, duration))
}

/// Decide whether the host's current state must be published, and why.
///
/// Track, play/pause, and seek changes are published immediately; otherwise a
/// periodic snapshot goes out once per `PERIODIC_SNAPSHOT_MS`.
pub(crate) fn classify_change(
    previous: Option<&PlaybackSnapshot>,
    next: &PlaybackState,
    host_now_ms: u64,
) -> Option<SyncReason> {
    let Some(previous) = previous else {
        return Some(SyncReason::TrackChanged);
    };
    let previous_id = previous.track.as_ref().map(|track| track.video_id.as_str());
    let next_id = next.track.as_ref().map(|track| track.video_id.as_str());
    if previous_id != next_id {
        return Some(SyncReason::TrackChanged);
    }
    if previous.playing != next.playing {
        return Some(if next.playing {
            SyncReason::Started
        } else {
            SyncReason::Paused
        });
    }
    let expected = expected_position(previous, host_now_ms);
    if (next.position_secs - expected).abs() > SEEK_DETECT_SECS
        || (next.speed - previous.speed).abs() > 0.001
    {
        return Some(SyncReason::Seeked);
    }
    (host_now_ms.saturating_sub(previous.host_clock_ms) >= PERIODIC_SNAPSHOT_MS)
        .then_some(SyncReason::Periodic)
}

/// Accepts only strictly newer revisions, so reordered or replayed snapshots
/// can never roll a guest back.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct RevisionGate {
    latest: u64,
}

impl RevisionGate {
    pub(crate) fn accept(&mut self, revision: u64) -> bool {
        if revision > self.latest {
            self.latest = revision;
            true
        } else {
            false
        }
    }
}

pub(crate) fn clock_offset_ms(
    client_before: u64,
    host_received: u64,
    host_sent: u64,
    client_after: u64,
) -> i64 {
    let client_mid = (client_before as i128 + client_after as i128) / 2;
    let host_mid = (host_received as i128 + host_sent as i128) / 2;
    (host_mid - client_mid) as i64
}

/// Host-clock offset estimate from ping round trips.
///
/// The sample with the shortest round trip had the least asymmetric delay, so
/// its offset is the most trustworthy of the recent window.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClockSync {
    samples: VecDeque<(u64, i64)>,
}

impl ClockSync {
    pub(crate) fn record(&mut self, client_before: u64, host_clock: u64, client_after: u64) -> i64 {
        let round_trip = client_after.saturating_sub(client_before);
        let offset = clock_offset_ms(client_before, host_clock, host_clock, client_after);
        if self.samples.len() == CLOCK_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((round_trip, offset));
        self.offset_ms()
    }

    pub(crate) fn offset_ms(&self) -> i64 {
        self.samples
            .iter()
            .min_by_key(|(round_trip, _)| *round_trip)
            .map_or(0, |(_, offset)| *offset)
    }
}

pub(crate) fn drift_action(local: f64, expected: f64) -> DriftAction {
    let drift = expected - local;
    if drift.abs() < DRIFT_DEADBAND_SECS {
        DriftAction::None
    } else if drift.abs() < DRIFT_SEEK_SECS {
        DriftAction::Nudge(drift)
    } else {
        DriftAction::Seek(expected.max(0.0))
    }
}

/// Playback speed that removes `drift` seconds over roughly four seconds,
/// bounded so the change stays unobtrusive.
pub(crate) fn nudge_speed(drift: f64) -> f64 {
    1.0 + (drift * 0.25).clamp(-MAX_NUDGE, MAX_NUDGE)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DriftAction {
    None,
    Nudge(f64),
    Seek(f64),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(playing: bool, position: f64) -> PlaybackSnapshot {
        PlaybackSnapshot {
            revision: 1,
            reason: SyncReason::Periodic,
            track: track_ref("abc123", "Song", "Artist", Some(300.0)),
            position_secs: position,
            playing,
            speed: 1.0,
            host_clock_ms: 1_000,
            scheduled_start_host_ms: None,
        }
    }

    fn state(id: &str, playing: bool, position: f64) -> PlaybackState {
        PlaybackState {
            track: track_ref(id, "Song", "Artist", Some(300.0)),
            position_secs: position,
            playing,
            speed: 1.0,
        }
    }

    #[test]
    fn playing_position_advances() {
        assert!((expected_position(&snapshot(true, 10.0), 2_500) - 11.5).abs() < 0.001);
    }
    #[test]
    fn paused_position_is_fixed() {
        assert_eq!(expected_position(&snapshot(false, 10.0), 2_500), 10.0);
    }
    #[test]
    fn scheduled_start_does_not_advance_early() {
        let mut value = snapshot(true, 10.0);
        value.scheduled_start_host_ms = Some(2_000);
        assert_eq!(expected_position(&value, 1_500), 10.0);
        assert_eq!(expected_position(&value, 2_500), 10.5);
    }
    #[test]
    fn projection_honours_speed_and_duration() {
        let mut value = snapshot(true, 10.0);
        value.speed = 2.0;
        assert!((expected_position(&value, 2_000) - 12.0).abs() < 0.001);
        assert_eq!(expected_position(&snapshot(true, 299.5), 9_000), 300.0);
    }
    #[test]
    fn clock_offset_has_documented_sign() {
        assert_eq!(clock_offset_ms(1_000, 1_120, 1_125, 1_005), 120);
    }
    #[test]
    fn clock_sync_prefers_the_shortest_round_trip() {
        let mut sync = ClockSync::default();
        // 40 ms round trip, true offset 500.
        assert_eq!(sync.record(1_000, 1_520, 1_040), 500);
        // A slow, asymmetric sample must not replace the better estimate.
        assert_eq!(sync.record(2_000, 2_900, 2_400), 500);
        // A tighter sample does.
        assert_eq!(sync.record(3_000, 3_503, 3_004), 501);
    }
    #[test]
    fn invalid_ids_and_numbers_are_rejected() {
        assert!(track_ref("bad id", "", "", Some(1.0)).is_none());
        assert!(track_ref("ok", "", "", Some(f64::NAN)).is_none());
        assert!(track_ref("../../etc/passwd", "", "", None).is_none());
        assert!(track_ref("C:\\music\\a.mp3", "", "", None).is_none());
    }
    #[test]
    fn drift_has_deadband_and_seek_threshold() {
        assert_eq!(drift_action(1.0, 1.02), DriftAction::None);
        assert!(matches!(drift_action(1.0, 1.1), DriftAction::Nudge(_)));
        assert_eq!(drift_action(1.0, 1.5), DriftAction::Seek(1.5));
    }
    #[test]
    fn nudge_speed_is_bounded_and_signed() {
        assert!(nudge_speed(0.2) > 1.0);
        assert!(nudge_speed(-0.2) < 1.0);
        assert!((nudge_speed(10.0) - (1.0 + MAX_NUDGE)).abs() < 1e-9);
        assert!((nudge_speed(-10.0) - (1.0 - MAX_NUDGE)).abs() < 1e-9);
    }

    #[test]
    fn host_changes_are_classified_by_kind() {
        let previous = snapshot(true, 10.0);
        // No previous snapshot: guests need the full state.
        assert_eq!(
            classify_change(None, &state("abc123", true, 0.0), 0),
            Some(SyncReason::TrackChanged)
        );
        assert_eq!(
            classify_change(Some(&previous), &state("other", true, 0.0), 1_100),
            Some(SyncReason::TrackChanged)
        );
        assert_eq!(
            classify_change(Some(&previous), &state("abc123", false, 10.1), 1_100),
            Some(SyncReason::Paused)
        );
        assert_eq!(
            classify_change(
                Some(&snapshot(false, 10.0)),
                &state("abc123", true, 10.0),
                1_100
            ),
            Some(SyncReason::Started)
        );
        assert_eq!(
            classify_change(Some(&previous), &state("abc123", true, 95.0), 1_100),
            Some(SyncReason::Seeked)
        );
    }

    #[test]
    fn steady_playback_only_publishes_periodically() {
        let previous = snapshot(true, 10.0);
        // 100 ms later, position where expected: nothing to send.
        assert_eq!(
            classify_change(Some(&previous), &state("abc123", true, 10.1), 1_100),
            None
        );
        assert_eq!(
            classify_change(Some(&previous), &state("abc123", true, 11.0), 2_000),
            Some(SyncReason::Periodic)
        );
    }

    #[test]
    fn stale_and_repeated_revisions_are_rejected() {
        let mut gate = RevisionGate::default();
        assert!(gate.accept(1));
        assert!(gate.accept(5));
        assert!(!gate.accept(5));
        assert!(!gate.accept(3));
        assert!(gate.accept(6));
    }

    #[test]
    fn guests_may_not_send_host_only_messages() {
        let track = track_ref("abc123", "Song", "Artist", None).unwrap();
        for allowed in [
            Message::Ping { client_clock_ms: 1 },
            Message::RequestSnapshot,
            Message::TrackRequest {
                track: track.clone(),
            },
            Message::Leave,
        ] {
            assert!(guest_may_send(&allowed), "{allowed:?}");
        }
        for forbidden in [
            Message::Snapshot(snapshot(false, 0.0)),
            Message::SessionEnded,
            Message::Requests(vec![]),
            Message::Welcome {
                version: PROTOCOL_VERSION,
                participant_id: 1,
                host_name: "x".into(),
            },
            Message::Pong {
                client_clock_ms: 1,
                host_clock_ms: 1,
            },
        ] {
            assert!(!guest_may_send(&forbidden), "{forbidden:?}");
        }
    }

    #[test]
    fn snapshots_round_trip_and_are_validated() {
        let original = snapshot(true, 42.5);
        let line = serde_json::to_string(&Message::Snapshot(original.clone())).unwrap();
        let Message::Snapshot(parsed) = serde_json::from_str(&line).unwrap() else {
            panic!("expected a snapshot");
        };
        assert_eq!(checked_snapshot(parsed), Some(original));

        let mut negative = snapshot(true, -1.0);
        assert!(checked_snapshot(negative.clone()).is_none());
        negative.position_secs = 1.0;
        negative.speed = 0.0;
        assert!(checked_snapshot(negative).is_none());

        let mut bad_track = snapshot(true, 1.0);
        bad_track.track.as_mut().unwrap().video_id = "not a valid id".into();
        assert!(checked_snapshot(bad_track).is_none());
    }

    #[test]
    fn discovery_packets_are_filtered_and_sanitized() {
        let announce = |version: u16, port: u16, token: &str| {
            serde_json::to_vec(&DiscoveryPacket::Announce {
                app: DISCOVERY_APP.into(),
                version,
                session_id: 7,
                host_name: "Living\u{7} room PC".into(),
                port,
                token: token.into(),
                track: Some("  Song  ".into()),
                listeners: 2,
            })
            .unwrap()
        };

        let Some(DiscoveryPacket::Announce {
            host_name, track, ..
        }) = decode_discovery(&announce(PROTOCOL_VERSION, 4_000, "123456"))
        else {
            panic!("a valid announce must decode");
        };
        assert_eq!(host_name, "Living room PC");
        assert_eq!(track.as_deref(), Some("Song"));

        assert!(decode_discovery(&announce(PROTOCOL_VERSION + 1, 4_000, "123456")).is_none());
        assert!(decode_discovery(&announce(PROTOCOL_VERSION, 0, "123456")).is_none());
        assert!(decode_discovery(&announce(PROTOCOL_VERSION, 4_000, "12ab")).is_none());
        assert!(decode_discovery(b"not json").is_none());
        assert!(decode_discovery(&vec![b' '; MAX_DISCOVERY_BYTES + 1]).is_none());
        assert_eq!(
            decode_discovery(&serde_json::to_vec(&discovery_query()).unwrap()),
            Some(discovery_query())
        );
    }
}
