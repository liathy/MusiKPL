//! LAN Jam protocol, timeline math, and lightweight TCP transport.
//!
//! Audio is never sent over the network. Participants exchange stable YouTube
//! IDs and playback snapshots, then resolve and play streams locally.

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

pub(crate) const PROTOCOL_VERSION: u16 = 1;
const MAX_LINE: usize = 64 * 1024;
const DEFAULT_PORT: u16 = 38_721;
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
    Hello { version: u16, token: String, name: String },
    Welcome { version: u16, participant_id: String },
    Ping { client_clock_ms: u64 },
    Pong { client_clock_ms: u64, host_clock_ms: u64 },
    RequestSnapshot,
    Snapshot(PlaybackSnapshot),
    TrackLoadStatus { revision: u64, video_id: String, status: LoadStatus },
    Leave,
    SessionEnded,
    Error { code: String, message: String },
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

pub(crate) struct JamController {
    pub(crate) info: JamInfo,
    events: Receiver<JamEvent>,
    commands: Option<Sender<Command>>,
    session_started: Option<Instant>,
    revision: u64,
    latest_snapshot: Option<PlaybackSnapshot>,
    clock_offset_ms: i64,
}

enum Command {
    Broadcast(PlaybackSnapshot),
    End,
}

impl Default for JamController {
    fn default() -> Self {
        let (_tx, events) = mpsc::channel();
        Self {
            info: JamInfo::default(),
            events,
            commands: None,
            session_started: None,
            revision: 0,
            latest_snapshot: None,
            clock_offset_ms: 0,
        }
    }
}

impl JamController {
    pub(crate) fn host(&mut self, port: u16) -> Result<(), String> {
        self.leave();
        let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|e| format!("Could not host Jam on port {port}: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Could not configure Jam listener: {e}"))?;
        let token = make_token();
        let address = local_lan_address(port);
        let (events_tx, events_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let token_for_thread = token.clone();
        thread::spawn(move || host_loop(listener, token_for_thread, events_tx, command_rx));
        self.events = events_rx;
        self.commands = Some(command_tx);
        self.session_started = Some(Instant::now());
        self.info = JamInfo {
            role: Some(Role::Host),
            address,
            token,
            status: "Hosting · waiting for guests".into(),
            peers: 0,
        };
        Ok(())
    }

    pub(crate) fn join(&mut self, address: &str, token: &str) -> Result<(), String> {
        self.leave();
        let stream = TcpStream::connect(address).map_err(|e| format!("Could not join Jam: {e}"))?;
        stream
            .set_nodelay(true)
            .map_err(|e| format!("Could not configure Jam connection: {e}"))?;
        let (events_tx, events_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let target = address.to_owned();
        let token = token.to_owned();
        thread::spawn(move || guest_loop(stream, token, events_tx, command_rx));
        self.events = events_rx;
        self.commands = Some(command_tx);
        self.session_started = Some(Instant::now());
        self.info = JamInfo {
            role: Some(Role::Guest),
            address: target,
            token: String::new(),
            status: "Connecting…".into(),
            peers: 0,
        };
        Ok(())
    }

    pub(crate) fn leave(&mut self) {
        if let Some(commands) = self.commands.take() {
            let _ = commands.send(Command::End);
        }
        self.info = JamInfo::default();
        self.session_started = None;
        self.latest_snapshot = None;
    }

    pub(crate) fn poll(&mut self) -> Vec<JamEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            if let JamEvent::Snapshot(snapshot) = &event {
                if snapshot.revision >= self.revision {
                    self.revision = snapshot.revision;
                    self.latest_snapshot = Some(snapshot.clone());
                }
            }
            match &event {
                JamEvent::Connected { role, address } => {
                    self.info.status = format!("Connected as {:?} · {address}", role);
                }
                JamEvent::PeerCount(count) => self.info.peers = *count,
                JamEvent::Status(status) => self.info.status = status.clone(),
                JamEvent::Disconnected => self.info.status = "Disconnected".into(),
                JamEvent::Error(error) => self.info.status = error.clone(),
                JamEvent::ClockOffset(offset) => self.clock_offset_ms = *offset,
                JamEvent::Snapshot(_) => {}
            }
            events.push(event);
        }
        events
    }

    pub(crate) fn broadcast(&self, snapshot: PlaybackSnapshot) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(Command::Broadcast(snapshot));
        }
    }

    pub(crate) fn clock_ms(&self) -> u64 {
        self.session_started
            .map(|start| start.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    pub(crate) fn host_now_ms(&self) -> u64 {
        let local = self.clock_ms() as i128 + self.clock_offset_ms as i128;
        local.max(0) as u64
    }
}

pub(crate) fn track_ref(video_id: &str, title: &str, artist: &str, duration_secs: Option<f64>) -> Option<TrackRef> {
    if !valid_video_id(video_id) || duration_secs.is_some_and(|value| !value.is_finite() || value < 0.0) {
        return None;
    }
    Some(TrackRef { video_id: video_id.into(), title: title.into(), artist: artist.into(), duration_secs })
}

pub(crate) fn expected_position(snapshot: &PlaybackSnapshot, host_now_ms: u64) -> f64 {
    let mut position = snapshot.position_secs;
    if snapshot.playing {
        let start = snapshot.scheduled_start_host_ms.unwrap_or(snapshot.host_clock_ms);
        if host_now_ms > start {
            position += (host_now_ms - start) as f64 / 1000.0;
        }
    }
    snapshot.track.as_ref().and_then(|track| track.duration_secs).map_or(position.max(0.0), |duration| position.clamp(0.0, duration))
}

pub(crate) fn clock_offset_ms(client_before: u64, host_received: u64, host_sent: u64, client_after: u64) -> i64 {
    let client_mid = (client_before as i128 + client_after as i128) / 2;
    let host_mid = (host_received as i128 + host_sent as i128) / 2;
    (host_mid - client_mid) as i64
}

pub(crate) fn drift_action(local: f64, expected: f64) -> DriftAction {
    let drift = expected - local;
    if drift.abs() < DRIFT_DEADBAND_SECS { DriftAction::None }
    else if drift.abs() < DRIFT_SEEK_SECS { DriftAction::Nudge(drift) }
    else { DriftAction::Seek(expected.max(0.0)) }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum DriftAction { None, Nudge(f64), Seek(f64) }

fn host_loop(listener: TcpListener, token: String, events: Sender<JamEvent>, commands: Receiver<Command>) {
    let mut peers: HashMap<usize, Sender<Message>> = HashMap::new();
    let mut next_id = 1usize;
    let started = Instant::now();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let (tx, rx) = mpsc::channel();
                let id = next_id; next_id += 1;
                let token_clone = token.clone(); let events_clone = events.clone();
                thread::spawn(move || host_peer(stream, token_clone, rx, events_clone, started));
                peers.insert(id, tx);
                let _ = events.send(JamEvent::PeerCount(peers.len()));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(error) => { let _ = events.send(JamEvent::Error(format!("Jam listener stopped: {error}"))); break; }
        }
        match commands.recv_timeout(Duration::from_millis(25)) {
            Ok(Command::Broadcast(snapshot)) => {
                for peer in peers.values() { let _ = peer.send(Message::Snapshot(snapshot.clone())); }
            }
            Ok(Command::End) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                for peer in peers.values() { let _ = peer.send(Message::SessionEnded); }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn host_peer(mut stream: TcpStream, token: String, outgoing: Receiver<Message>, events: Sender<JamEvent>, started: Instant) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let cloned = match stream.try_clone() { Ok(value) => value, Err(_) => return };
    let mut reader = BufReader::new(cloned);
    let hello = match read_message(&mut reader) { Ok(Message::Hello { version, token: supplied, .. }) if version == PROTOCOL_VERSION && supplied == token => Message::Welcome { version: PROTOCOL_VERSION, participant_id: format!("guest-{}", std::process::id()) }, _ => Message::Error { code: "unauthorized".into(), message: "Invalid Jam credentials or protocol version".into() } };
    if let Err(error) = write_message(&mut stream, &hello) {
        let _ = events.send(JamEvent::Error(format!("Jam handshake write failed: {error}")));
        return;
    }
    if matches!(hello, Message::Error { .. }) { return; }
    let _ = events.send(JamEvent::Connected { role: Role::Host, address: "guest".into() });
    loop {
        while let Ok(message) = outgoing.try_recv() {
            if let Err(error) = write_message(&mut stream, &message) {
                let _ = events.send(JamEvent::Error(format!("Jam snapshot write failed: {error}")));
                return;
            }
        }
        match read_message(&mut reader) {
            Ok(Message::Ping { client_clock_ms }) => { let now = started.elapsed().as_millis() as u64; let _ = write_message(&mut stream, &Message::Pong { client_clock_ms, host_clock_ms: now }); }
            Ok(Message::Leave) => return,
            Err(ReadError::Timeout) => continue,
            Err(_) => return,
            Ok(Message::RequestSnapshot) | Ok(Message::TrackLoadStatus { .. }) => {}
            Ok(_) => {}
        }
    }
}

fn guest_loop(mut stream: TcpStream, token: String, events: Sender<JamEvent>, commands: Receiver<Command>) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    if let Err(error) = write_message(&mut stream, &Message::Hello { version: PROTOCOL_VERSION, token, name: "MusiKPL guest".into() }) {
        let _ = events.send(JamEvent::Error(format!("Jam connect write failed: {error}")));
        return;
    }
    let cloned = match stream.try_clone() { Ok(value) => value, Err(error) => { let _ = events.send(JamEvent::Error(error.to_string())); return; } };
    let mut reader = BufReader::new(cloned);
    let started = Instant::now();
    let mut last_ping = Instant::now() - Duration::from_secs(1);
    let mut last_ping_clock = 0u64;
    loop {
        while let Ok(Command::End) = commands.try_recv() { let _ = write_message(&mut stream, &Message::Leave); let _ = events.send(JamEvent::Disconnected); return; }
        if last_ping.elapsed() >= Duration::from_millis(500) {
            last_ping_clock = started.elapsed().as_millis() as u64;
            let _ = write_message(&mut stream, &Message::Ping { client_clock_ms: last_ping_clock });
            last_ping = Instant::now();
        }
        match read_message(&mut reader) {
            Ok(Message::Welcome { .. }) => { let _ = events.send(JamEvent::Connected { role: Role::Guest, address: "host".into() }); }
            Ok(Message::Snapshot(snapshot)) => { let _ = events.send(JamEvent::Snapshot(snapshot)); }
            Ok(Message::Pong { client_clock_ms, host_clock_ms }) => {
                let client_after = started.elapsed().as_millis() as u64;
                let before = if client_clock_ms == 0 { last_ping_clock } else { client_clock_ms };
                let _ = events.send(JamEvent::ClockOffset(clock_offset_ms(before, host_clock_ms, host_clock_ms, client_after)));
            }
            Ok(Message::SessionEnded) => { let _ = events.send(JamEvent::Status("Host ended the Jam".into())); return; }
            Ok(Message::Error { message, .. }) => { let _ = events.send(JamEvent::Error(message)); return; }
            Ok(_) => {}
            Err(ReadError::Timeout) => continue,
            Err(ReadError::Closed) => { let _ = events.send(JamEvent::Disconnected); return; }
            Err(ReadError::Invalid(error)) => { let _ = events.send(JamEvent::Error(format!("Jam message error: {error}"))); return; }
        }
    }
}

enum ReadError {
    Timeout,
    Closed,
    Invalid(String),
}

fn read_message(reader: &mut BufReader<TcpStream>) -> Result<Message, ReadError> {
    let mut line = String::new();
    let size = reader.read_line(&mut line).map_err(|error| {
        if matches!(error.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) {
            ReadError::Timeout
        } else {
            ReadError::Invalid(error.to_string())
        }
    })?;
    if size == 0 { return Err(ReadError::Closed); }
    if size > MAX_LINE { return Err(ReadError::Invalid("Invalid or oversized Jam message".into())); }
    serde_json::from_str(&line).map_err(|error| ReadError::Invalid(error.to_string()))
}

fn write_message(stream: &mut TcpStream, message: &Message) -> Result<(), String> {
    let line = serde_json::to_string(message).map_err(|e| e.to_string())?;
    if line.len() > MAX_LINE { return Err("Jam message is too large".into()); }
    stream.write_all(line.as_bytes()).and_then(|_| stream.write_all(b"\n")).map_err(|e| e.to_string())
}

fn valid_video_id(id: &str) -> bool { id.len() <= 64 && !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') }

fn make_token() -> String { format!("{:016x}{:016x}", rand_seed(), rand_seed().rotate_left(17)) }

fn rand_seed() -> u64 { let now = Instant::now(); now.elapsed().as_nanos() as u64 ^ std::process::id() as u64 }

fn local_lan_address(port: u16) -> String {
    let address = std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into());
    format!("{address}:{port}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(playing: bool, position: f64) -> PlaybackSnapshot { PlaybackSnapshot { revision: 1, track: track_ref("abc123", "Song", "Artist", Some(300.0)), position_secs: position, playing, host_clock_ms: 1_000, scheduled_start_host_ms: None } }

    #[test] fn playing_position_advances() { assert!((expected_position(&snapshot(true, 10.0), 2_500) - 11.5).abs() < 0.001); }
    #[test] fn paused_position_is_fixed() { assert_eq!(expected_position(&snapshot(false, 10.0), 2_500), 10.0); }
    #[test] fn scheduled_start_does_not_advance_early() { let mut value = snapshot(true, 10.0); value.scheduled_start_host_ms = Some(2_000); assert_eq!(expected_position(&value, 1_500), 10.0); assert_eq!(expected_position(&value, 2_500), 10.5); }
    #[test] fn clock_offset_has_documented_sign() { assert_eq!(clock_offset_ms(1_000, 1_120, 1_125, 1_005), 120); }
    #[test] fn invalid_ids_and_numbers_are_rejected() { assert!(track_ref("bad id", "", "", Some(1.0)).is_none()); assert!(track_ref("ok", "", "", Some(f64::NAN)).is_none()); }
    #[test] fn drift_has_deadband_and_seek_threshold() { assert_eq!(drift_action(1.0, 1.02), DriftAction::None); assert!(matches!(drift_action(1.0, 1.1), DriftAction::Nudge(_))); assert_eq!(drift_action(1.0, 1.5), DriftAction::Seek(1.5)); }
}
