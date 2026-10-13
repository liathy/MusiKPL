//! LAN Jam wire types, roles, and deterministic synchronization rules.

use serde::{Deserialize, Serialize};

pub(crate) const PROTOCOL_VERSION: u16 = 1;
const DRIFT_DEADBAND_SECS: f64 = 0.050;
const DRIFT_SEEK_SECS: f64 = 0.250;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct TrackRef {
    pub(crate) video_id: String,
    pub(crate) title: String,
    pub(crate) artist: String,
    pub(crate) duration_secs: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct PlaybackSnapshot {
    pub(crate) revision: u64,
    pub(crate) track: Option<TrackRef>,
    pub(crate) position_secs: f64,
    pub(crate) playing: bool,
    pub(crate) host_clock_ms: u64,
    pub(crate) scheduled_start_host_ms: Option<u64>,
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
        participant_id: String,
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
    Leave,
    SessionEnded,
    Error {
        code: String,
        message: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JamEvent {
    Connected { role: Role, address: String },
    Snapshot(PlaybackSnapshot),
    PeerCount(usize),
    Status(String),
    Disconnected,
    Error(String),
    ClockOffset(i64),
}

#[derive(Clone, Debug)]
pub(crate) struct JamInfo {
    pub(crate) role: Option<Role>,
    pub(crate) address: String,
    pub(crate) token: String,
    pub(crate) status: String,
    pub(crate) peers: usize,
}

impl Default for JamInfo {
    fn default() -> Self {
        Self {
            role: None,
            address: String::new(),
            token: String::new(),
            status: "Jam inactive".into(),
            peers: 0,
        }
    }
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
        title: title.into(),
        artist: artist.into(),
        duration_secs,
    })
}

pub(crate) fn expected_position(snapshot: &PlaybackSnapshot, host_now_ms: u64) -> f64 {
    let mut position = snapshot.position_secs;
    if snapshot.playing {
        let start = snapshot
            .scheduled_start_host_ms
            .unwrap_or(snapshot.host_clock_ms);
        if host_now_ms > start {
            position += (host_now_ms - start) as f64 / 1000.0;
        }
    }
    snapshot
        .track
        .as_ref()
        .and_then(|track| track.duration_secs)
        .map_or(position.max(0.0), |duration| position.clamp(0.0, duration))
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
            track: track_ref("abc123", "Song", "Artist", Some(300.0)),
            position_secs: position,
            playing,
            host_clock_ms: 1_000,
            scheduled_start_host_ms: None,
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
    fn clock_offset_has_documented_sign() {
        assert_eq!(clock_offset_ms(1_000, 1_120, 1_125, 1_005), 120);
    }
    #[test]
    fn invalid_ids_and_numbers_are_rejected() {
        assert!(track_ref("bad id", "", "", Some(1.0)).is_none());
        assert!(track_ref("ok", "", "", Some(f64::NAN)).is_none());
    }
    #[test]
    fn drift_has_deadband_and_seek_threshold() {
        assert_eq!(drift_action(1.0, 1.02), DriftAction::None);
        assert!(matches!(drift_action(1.0, 1.1), DriftAction::Nudge(_)));
        assert_eq!(drift_action(1.0, 1.5), DriftAction::Seek(1.5));
    }
}
