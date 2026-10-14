//! LAN Jam controller and TCP transport.
//!
//! The wire schema and synchronization rules live in `protocol`, the guest
//! request list in `requests`, and nearby-Jam discovery in `discovery`, so
//! each can be tested independently of sockets and worker threads.
//!
//! Threads: a host runs one hub thread (accepts connections, answers
//! discovery, fans messages out) plus a reader and a writer per guest; a
//! guest runs one connection thread. All of them talk to the controller, and
//! so to the UI thread, only through channels that are polled once per frame.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

mod discovery;
mod protocol;
mod requests;

pub(crate) use discovery::{DiscoveryStatus, NearbyJam};
pub(crate) use protocol::*;

use discovery::{Discovery, DISCOVERY_PORT};
use requests::RequestQueue;

const MAX_LINE: usize = 64 * 1024;
/// Preferred TCP port, so joining by code has a predictable address. Any free
/// port is used when it is taken; discovery advertises the real one.
const JAM_PORT: u16 = 38_721;
const MAX_GUESTS: usize = 16;
/// Wake-up interval of the host hub for new connections and discovery.
const HUB_TICK: Duration = Duration::from_millis(25);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// Guests ping every second; this much silence means the guest is gone.
const GUEST_SILENCE_TIMEOUT: Duration = Duration::from_secs(10);
/// The host sends a snapshot every second; this much silence means it is gone.
const HOST_SILENCE_TIMEOUT: Duration = Duration::from_secs(6);
const GUEST_POLL: Duration = Duration::from_millis(50);
const PING_INTERVAL: Duration = Duration::from_secs(1);
/// Faster pings right after connecting, to settle the clock offset quickly.
const PING_BURST_INTERVAL: Duration = Duration::from_millis(150);
const PING_BURST_COUNT: u32 = 5;
const RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY: Duration = Duration::from_secs(2);
/// Forbidden or malformed messages tolerated before a guest is dropped.
const MAX_STRIKES: u32 = 3;
/// Delay before refusing a wrong code, to make guessing impractical.
const WRONG_CODE_DELAY: Duration = Duration::from_millis(500);

pub(crate) struct JamController {
    pub(crate) info: JamInfo,
    wire: Receiver<Wire>,
    hub: Option<Sender<HubMsg>>,
    guest: Option<Sender<GuestCmd>>,
    clock_origin: Option<Instant>,
    revision: u64,
    gate: RevisionGate,
    latest_snapshot: Option<PlaybackSnapshot>,
    clock_offset_ms: i64,
    requests: RequestQueue,
    guest_requests: Vec<RequestEntry>,
    /// Host: what each connected guest last reported about loading a track.
    guest_loads: HashMap<u64, (String, LoadStatus)>,
    participant_id: Option<u64>,
    discovery: Option<Discovery>,
}

/// Controller and guest-connection messages to the host hub.
enum HubMsg {
    Publish(PlaybackSnapshot),
    Requests(Vec<RequestEntry>),
    SendTo {
        peer: u64,
        message: Message,
    },
    End,
    PeerJoined {
        id: u64,
        name: String,
        outgoing: Sender<Message>,
    },
    PeerLeft {
        id: u64,
    },
    PeerRequest {
        id: u64,
        track: TrackRef,
    },
    PeerWantsSnapshot {
        id: u64,
    },
    PeerLoad {
        id: u64,
        video_id: String,
        status: LoadStatus,
    },
}

enum GuestCmd {
    Request(TrackRef),
    LoadStatus {
        revision: u64,
        video_id: String,
        status: LoadStatus,
    },
    End,
}

/// Worker-thread reports to the controller.
enum Wire {
    Peers(usize),
    GuestRequest {
        peer: u64,
        name: String,
        track: TrackRef,
    },
    Notice(String),
    GuestLoad {
        peer: u64,
        video_id: String,
        status: LoadStatus,
    },
    GuestGone(u64),
    Welcome {
        participant_id: u64,
        host_name: String,
    },
    Snapshot(PlaybackSnapshot),
    Requests(Vec<RequestEntry>),
    ClockOffset(i64),
    Reconnecting,
    Closed(String),
}

impl Default for JamController {
    fn default() -> Self {
        let (_tx, wire) = mpsc::channel();
        Self {
            info: JamInfo::default(),
            wire,
            hub: None,
            guest: None,
            clock_origin: None,
            revision: 0,
            gate: RevisionGate::default(),
            latest_snapshot: None,
            clock_offset_ms: 0,
            requests: RequestQueue::default(),
            guest_requests: Vec::new(),
            guest_loads: HashMap::new(),
            participant_id: None,
            discovery: None,
        }
    }
}

impl Drop for JamController {
    fn drop(&mut self) {
        self.leave();
    }
}

impl JamController {
    /// Begin looking for nearby Jams in the background.
    pub(crate) fn start_discovery(&mut self) {
        if self.discovery.is_none() {
            self.discovery = Some(Discovery::start(DISCOVERY_PORT));
        }
    }

    pub(crate) fn nearby(&self) -> Vec<NearbyJam> {
        self.discovery
            .as_ref()
            .map(Discovery::nearby)
            .unwrap_or_default()
    }

    pub(crate) fn discovery_status(&self) -> DiscoveryStatus {
        self.discovery
            .as_ref()
            .map_or(DiscoveryStatus::Searching, Discovery::status)
    }

    pub(crate) fn host(&mut self) -> Result<(), String> {
        self.host_with(Ipv4Addr::UNSPECIFIED, JAM_PORT, Some(DISCOVERY_PORT))
    }

    fn host_with(
        &mut self,
        bind: Ipv4Addr,
        port: u16,
        discovery_port: Option<u16>,
    ) -> Result<(), String> {
        self.leave();
        let listener = TcpListener::bind((bind, port))
            .or_else(|_| TcpListener::bind((bind, 0)))
            .map_err(|e| format!("Could not start a Jam on this device: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("Could not configure the Jam listener: {e}"))?;
        let port = listener
            .local_addr()
            .map_err(|e| format!("Could not read the Jam port: {e}"))?
            .port();
        // Hosting still works without discovery; guests then join by code.
        let responder = discovery_port.and_then(|port| discovery::bind_responder(bind, port).ok());
        let discoverable = responder.is_some();
        let token = make_token();
        let host_name = device_name();
        let origin = Instant::now();
        let (wire_tx, wire_rx) = mpsc::channel();
        let (hub_tx, hub_rx) = mpsc::channel();
        let session = HostSession {
            listener,
            responder,
            port,
            session_id: rand_seed(),
            token: token.clone(),
            host_name: host_name.clone(),
            origin,
            hub: hub_tx.clone(),
            inbox: hub_rx,
            wire: wire_tx,
        };
        thread::spawn(move || session.run());

        self.wire = wire_rx;
        self.hub = Some(hub_tx);
        self.clock_origin = Some(origin);
        let ip = discovery::local_ipv4()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "this device's IP address".into());
        self.info = JamInfo {
            role: Some(Role::Host),
            token,
            status: "Waiting for listeners".into(),
            host_name,
            address: format!("{ip}:{port}"),
            discoverable,
            ..JamInfo::default()
        };
        self.set_browsing(false);
        Ok(())
    }

    /// Join a Jam found by discovery; no address or code is needed.
    pub(crate) fn join_nearby(&mut self, jam: &NearbyJam) {
        self.connect(jam.address, jam.token.clone(), jam.host_name.clone());
    }

    /// Join by typing the host's address and six-digit code.
    pub(crate) fn join(&mut self, address: &str, token: &str) -> Result<(), String> {
        let address = parse_host_address(address)
            .ok_or("Enter the host's address like 192.168.1.20 (shown on the host's screen)")?;
        let token = normalize_code(token);
        if !valid_token(&token) {
            return Err("Enter the six-digit code shown on the host's screen".into());
        }
        self.connect(address, token, String::new());
        Ok(())
    }

    fn connect(&mut self, address: SocketAddr, token: String, host_name: String) {
        self.leave();
        let origin = Instant::now();
        let (wire_tx, wire_rx) = mpsc::channel();
        let (command_tx, command_rx) = mpsc::channel();
        let name = device_name();
        // Connecting can take seconds, so it never runs on the UI thread.
        thread::spawn(move || {
            guest_session(address, &token, &name, origin, &wire_tx, &command_rx);
        });
        self.wire = wire_rx;
        self.guest = Some(command_tx);
        self.clock_origin = Some(origin);
        self.info = JamInfo {
            role: Some(Role::Guest),
            status: "Connecting…".into(),
            connecting: true,
            host_name,
            ..JamInfo::default()
        };
        self.set_browsing(false);
    }

    /// Leave or end the current session and release its network resources.
    pub(crate) fn leave(&mut self) {
        self.close(None);
    }

    fn close(&mut self, notice: Option<String>) {
        if let Some(hub) = self.hub.take() {
            let _ = hub.send(HubMsg::End);
        }
        if let Some(guest) = self.guest.take() {
            let _ = guest.send(GuestCmd::End);
        }
        // Dropping the receiver also stops anything a worker still sends.
        let (_tx, wire) = mpsc::channel();
        self.wire = wire;
        self.info = JamInfo {
            notice,
            ..JamInfo::default()
        };
        self.clock_origin = None;
        self.revision = 0;
        self.gate = RevisionGate::default();
        self.latest_snapshot = None;
        self.clock_offset_ms = 0;
        self.requests = RequestQueue::default();
        self.guest_requests.clear();
        self.guest_loads.clear();
        self.participant_id = None;
        self.set_browsing(true);
    }

    fn set_browsing(&self, enabled: bool) {
        if let Some(discovery) = &self.discovery {
            discovery.set_enabled(enabled);
        }
    }

    /// Apply everything the worker threads reported since the last frame.
    pub(crate) fn poll(&mut self) -> Vec<JamEvent> {
        let mut events = Vec::new();
        let mut requests_changed = false;
        while let Ok(wire) = self.wire.try_recv() {
            match wire {
                Wire::Peers(count) => {
                    self.info.peers = count;
                    self.info.status = if count == 0 {
                        "Waiting for listeners".into()
                    } else {
                        "Jam is live".into()
                    };
                }
                Wire::GuestRequest { peer, name, track } => {
                    let now = self.clock_ms();
                    match self.requests.submit(peer, &name, &track, now) {
                        Ok(_) => requests_changed = true,
                        Err(error) => self.send_hub(HubMsg::SendTo {
                            peer,
                            message: Message::Error {
                                code: ERROR_REQUEST.into(),
                                message: error.message().into(),
                            },
                        }),
                    }
                }
                Wire::Notice(text) => events.push(JamEvent::Notice(text)),
                Wire::GuestLoad {
                    peer,
                    video_id,
                    status,
                } => {
                    self.guest_loads.insert(peer, (video_id, status));
                }
                Wire::GuestGone(peer) => {
                    self.guest_loads.remove(&peer);
                }
                Wire::Welcome {
                    participant_id,
                    host_name,
                } => {
                    self.participant_id = Some(participant_id);
                    self.info.connecting = false;
                    self.info.host_name = sanitize_text(&host_name, MAX_NAME_CHARS);
                    self.info.status = "Connected".into();
                }
                Wire::Snapshot(snapshot) => {
                    if let Some(snapshot) = checked_snapshot(snapshot) {
                        if self.gate.accept(snapshot.revision) {
                            self.latest_snapshot = Some(snapshot.clone());
                            events.push(JamEvent::Snapshot(snapshot));
                        }
                    }
                }
                Wire::Requests(entries) => self.guest_requests = checked_requests(entries),
                Wire::ClockOffset(offset) => self.clock_offset_ms = offset,
                Wire::Reconnecting => {
                    self.info.connecting = true;
                    self.info.status = "Connection lost. Reconnecting…".into();
                }
                Wire::Closed(reason) => {
                    self.close(Some(reason));
                    events.push(JamEvent::SessionClosed);
                    break;
                }
            }
        }
        if requests_changed {
            self.share_requests();
        }
        events
    }

    /// Host: offer the current playback state. A snapshot is sent only when
    /// something changed or the periodic interval elapsed.
    pub(crate) fn publish(&mut self, state: PlaybackState) {
        if self.info.role != Some(Role::Host) {
            return;
        }
        let now = self.clock_ms();
        let Some(reason) = classify_change(self.latest_snapshot.as_ref(), &state, now) else {
            return;
        };
        self.revision += 1;
        let snapshot = PlaybackSnapshot {
            revision: self.revision,
            reason,
            track: state.track,
            position_secs: if state.position_secs.is_finite() {
                state.position_secs.max(0.0)
            } else {
                0.0
            },
            playing: state.playing,
            speed: state.speed,
            host_clock_ms: now,
            scheduled_start_host_ms: None,
        };
        self.latest_snapshot = Some(snapshot.clone());
        self.send_hub(HubMsg::Publish(snapshot));
    }

    pub(crate) fn latest_snapshot(&self) -> Option<&PlaybackSnapshot> {
        self.latest_snapshot.as_ref()
    }

    pub(crate) fn clock_ms(&self) -> u64 {
        self.clock_origin
            .map(|start| start.elapsed().as_millis() as u64)
            .unwrap_or(0)
    }

    /// Guest: the host's clock right now, from the measured offset.
    pub(crate) fn host_now_ms(&self) -> u64 {
        let local = self.clock_ms() as i128 + self.clock_offset_ms as i128;
        local.max(0) as u64
    }

    /// The request list: authoritative on the host, a copy on guests.
    pub(crate) fn requests(&self) -> &[RequestEntry] {
        if self.info.role == Some(Role::Host) {
            self.requests.entries()
        } else {
            &self.guest_requests
        }
    }

    /// Guest: this device's ID in the session, to mark its own requests.
    pub(crate) fn participant_id(&self) -> Option<u64> {
        self.participant_id
    }

    /// Guest: ask the host to consider a track.
    pub(crate) fn request_track(&self, track: TrackRef) -> Result<(), String> {
        match &self.guest {
            Some(guest) if !self.info.connecting => guest
                .send(GuestCmd::Request(track))
                .map_err(|_| "The Jam connection is closed".to_string()),
            _ => Err("Wait until you're connected to the Jam".into()),
        }
    }

    /// Guest: tell the host whether its current track could be loaded here.
    pub(crate) fn report_load(&self, revision: u64, video_id: &str, status: LoadStatus) {
        if let Some(guest) = &self.guest {
            let _ = guest.send(GuestCmd::LoadStatus {
                revision,
                video_id: video_id.to_owned(),
                status,
            });
        }
    }

    /// Host: whether every connected guest has finished loading `video_id`,
    /// successfully or not. True with no guests.
    pub(crate) fn guests_ready(&self, video_id: &str) -> bool {
        let done = self
            .guest_loads
            .values()
            .filter(|(id, status)| id == video_id && *status != LoadStatus::Loading)
            .count();
        done >= self.info.peers
    }

    /// Host: accept a pending request; returns the track to enqueue.
    pub(crate) fn accept_request(&mut self, id: u64) -> Option<TrackRef> {
        if self.info.role != Some(Role::Host) {
            return None;
        }
        let track = self.requests.accept(id)?;
        self.share_requests();
        Some(track)
    }

    /// Host: decline a pending request.
    pub(crate) fn reject_request(&mut self, id: u64) {
        if self.info.role == Some(Role::Host) && self.requests.reject(id) {
            self.share_requests();
        }
    }

    /// Host: withdraw a request; returns the track if it must also leave the
    /// playback queue.
    pub(crate) fn remove_request(&mut self, id: u64) -> Option<TrackRef> {
        if self.info.role != Some(Role::Host) {
            return None;
        }
        let queued = self.requests.remove(id)?;
        self.share_requests();
        queued
    }

    /// Host: update accepted requests from the real playback queue.
    pub(crate) fn reconcile_requests(
        &mut self,
        current: Option<&str>,
        in_queue: impl Fn(&str) -> bool,
    ) {
        if self.info.role == Some(Role::Host) && self.requests.reconcile(current, in_queue) {
            self.share_requests();
        }
    }

    /// Host on loopback only, so tests need no firewall rule or free port.
    #[cfg(test)]
    pub(crate) fn host_on_loopback(&mut self) -> SocketAddr {
        self.host_with(Ipv4Addr::LOCALHOST, 0, None)
            .expect("loopback host starts");
        let port = self.info.address.rsplit(':').next().unwrap_or_default();
        SocketAddr::from((Ipv4Addr::LOCALHOST, port.parse().expect("port")))
    }

    #[cfg(test)]
    pub(crate) fn join_for_test(&mut self, address: SocketAddr, token: &str) {
        self.connect(address, token.to_owned(), String::new());
    }

    fn share_requests(&self) {
        self.send_hub(HubMsg::Requests(self.requests.entries().to_vec()));
    }

    fn send_hub(&self, message: HubMsg) {
        if let Some(hub) = &self.hub {
            let _ = hub.send(message);
        }
    }
}

struct Peer {
    name: String,
    outgoing: Sender<Message>,
}

struct HostSession {
    listener: TcpListener,
    responder: Option<UdpSocket>,
    port: u16,
    session_id: u64,
    token: String,
    host_name: String,
    origin: Instant,
    hub: Sender<HubMsg>,
    inbox: Receiver<HubMsg>,
    wire: Sender<Wire>,
}

impl HostSession {
    fn run(self) {
        let mut peers: HashMap<u64, Peer> = HashMap::new();
        let mut connections = 0usize;
        let mut next_id = 1u64;
        let mut snapshot: Option<PlaybackSnapshot> = None;
        let mut requests: Vec<RequestEntry> = Vec::new();
        let broadcast = |peers: &HashMap<u64, Peer>, message: Message| {
            for peer in peers.values() {
                let _ = peer.outgoing.send(message.clone());
            }
        };

        loop {
            match self.listener.accept() {
                Ok((stream, _)) if connections >= MAX_GUESTS => refuse_full(stream),
                Ok((stream, _)) => {
                    connections += 1;
                    let id = next_id;
                    next_id += 1;
                    let context = PeerContext {
                        token: self.token.clone(),
                        host_name: self.host_name.clone(),
                        origin: self.origin,
                        hub: self.hub.clone(),
                    };
                    thread::spawn(move || serve_peer(stream, id, context));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::ConnectionAborted
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => {
                    broadcast(&peers, Message::SessionEnded);
                    let _ = self.wire.send(Wire::Closed(format!(
                        "The Jam stopped because this device can no longer accept listeners ({error})."
                    )));
                    return;
                }
            }
            if let Some(responder) = &self.responder {
                discovery::answer_queries(
                    responder,
                    &self.announcement(snapshot.as_ref(), peers.len()),
                );
            }

            match self.inbox.recv_timeout(HUB_TICK) {
                Ok(HubMsg::Publish(next)) => {
                    broadcast(&peers, Message::Snapshot(next.clone()));
                    snapshot = Some(next);
                }
                Ok(HubMsg::Requests(next)) => {
                    broadcast(&peers, Message::Requests(next.clone()));
                    requests = next;
                }
                Ok(HubMsg::SendTo { peer, message }) => {
                    if let Some(peer) = peers.get(&peer) {
                        let _ = peer.outgoing.send(message);
                    }
                }
                Ok(HubMsg::PeerJoined { id, name, outgoing }) => {
                    // A newcomer gets the authoritative state immediately.
                    if let Some(snapshot) = &snapshot {
                        let _ = outgoing.send(Message::Snapshot(snapshot.clone()));
                    }
                    let _ = outgoing.send(Message::Requests(requests.clone()));
                    peers.insert(id, Peer { name, outgoing });
                    let _ = self.wire.send(Wire::Peers(peers.len()));
                }
                Ok(HubMsg::PeerLeft { id }) => {
                    connections = connections.saturating_sub(1);
                    if peers.remove(&id).is_some() {
                        let _ = self.wire.send(Wire::GuestGone(id));
                        let _ = self.wire.send(Wire::Peers(peers.len()));
                    }
                }
                Ok(HubMsg::PeerRequest { id, track }) => {
                    if let Some(peer) = peers.get(&id) {
                        let _ = self.wire.send(Wire::GuestRequest {
                            peer: id,
                            name: peer.name.clone(),
                            track,
                        });
                    }
                }
                Ok(HubMsg::PeerWantsSnapshot { id }) => {
                    if let (Some(peer), Some(snapshot)) = (peers.get(&id), &snapshot) {
                        let _ = peer.outgoing.send(Message::Snapshot(snapshot.clone()));
                    }
                }
                Ok(HubMsg::PeerLoad {
                    id,
                    video_id,
                    status,
                }) => {
                    if let Some(peer) = peers.get(&id) {
                        if status == LoadStatus::Failed {
                            let _ = self.wire.send(Wire::Notice(format!(
                                "{} couldn't load the current track",
                                peer.name
                            )));
                        }
                        let _ = self.wire.send(Wire::GuestLoad {
                            peer: id,
                            video_id,
                            status,
                        });
                    }
                }
                Ok(HubMsg::End) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    broadcast(&peers, Message::SessionEnded);
                    return;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn announcement(
        &self,
        snapshot: Option<&PlaybackSnapshot>,
        listeners: usize,
    ) -> DiscoveryPacket {
        DiscoveryPacket::Announce {
            app: DISCOVERY_APP.into(),
            version: PROTOCOL_VERSION,
            session_id: self.session_id,
            host_name: self.host_name.clone(),
            port: self.port,
            token: self.token.clone(),
            track: snapshot
                .and_then(|snapshot| snapshot.track.as_ref())
                .map(|track| sanitize_text(&track.title, 80)),
            listeners: listeners as u32,
        }
    }
}

struct PeerContext {
    token: String,
    host_name: String,
    origin: Instant,
    hub: Sender<HubMsg>,
}

fn refuse_full(mut stream: TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let _ = write_message(
        &mut stream,
        &Message::Error {
            code: "full".into(),
            message: "This Jam is full".into(),
        },
    );
}

fn serve_peer(stream: TcpStream, id: u64, context: PeerContext) {
    peer_session(&stream, id, &context);
    // The hub drops this guest's sender; its writer thread then flushes what
    // is still queued (such as a final refusal) and closes the socket.
    let _ = context.hub.send(HubMsg::PeerLeft { id });
}

/// One guest connection on the host: handshake, then a read loop that only
/// lets guest-permitted messages through.
fn peer_session(stream: &TcpStream, id: u64, context: &PeerContext) {
    // Accepted sockets inherit the listener's non-blocking mode on Windows.
    if stream.set_nonblocking(false).is_err() {
        return;
    }
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let (Ok(read_half), Ok(mut write_half)) = (stream.try_clone(), stream.try_clone()) else {
        return;
    };
    let mut reader = LineReader::new(read_half);

    let name = match reader.read_message() {
        Ok(Message::Hello {
            version,
            token,
            name,
        }) => {
            if version != PROTOCOL_VERSION {
                let _ = write_message(
                    &mut write_half,
                    &Message::Error {
                        code: "version".into(),
                        message: "This Jam uses a different MusiKPL version. Update both apps and try again.".into(),
                    },
                );
                return;
            }
            if token != context.token {
                thread::sleep(WRONG_CODE_DELAY);
                let _ = write_message(
                    &mut write_half,
                    &Message::Error {
                        code: "unauthorized".into(),
                        message: "That Jam code isn't right. Check the code on the host's screen."
                            .into(),
                    },
                );
                return;
            }
            let name = sanitize_text(&name, MAX_NAME_CHARS);
            if name.is_empty() {
                "Guest".to_owned()
            } else {
                name
            }
        }
        _ => return,
    };
    if write_message(
        &mut write_half,
        &Message::Welcome {
            version: PROTOCOL_VERSION,
            participant_id: id,
            host_name: context.host_name.clone(),
        },
    )
    .is_err()
    {
        return;
    }

    let (outgoing, queue) = mpsc::channel::<Message>();
    thread::spawn(move || {
        for message in queue {
            let last = matches!(message, Message::SessionEnded);
            if write_message(&mut write_half, &message).is_err() || last {
                break;
            }
        }
        // Unblocks the reader so both halves end together.
        let _ = write_half.shutdown(Shutdown::Both);
    });
    let _ = context.hub.send(HubMsg::PeerJoined {
        id,
        name,
        outgoing: outgoing.clone(),
    });
    let _ = stream.set_read_timeout(Some(GUEST_SILENCE_TIMEOUT));

    let mut strikes = 0u32;
    loop {
        let message = match reader.read_message() {
            Ok(message) => message,
            Err(ReadError::Invalid) => {
                strikes += 1;
                if strikes >= MAX_STRIKES {
                    return;
                }
                continue;
            }
            Err(ReadError::Timeout | ReadError::Closed | ReadError::TooLong) => return,
        };
        // Authorization: a guest connection can never act as the host.
        if !guest_may_send(&message) {
            let _ = outgoing.send(Message::Error {
                code: ERROR_FORBIDDEN.into(),
                message: "Only the host can control the Jam".into(),
            });
            strikes += 1;
            if strikes >= MAX_STRIKES {
                return;
            }
            continue;
        }
        match message {
            Message::Ping { client_clock_ms } => {
                let _ = outgoing.send(Message::Pong {
                    client_clock_ms,
                    host_clock_ms: context.origin.elapsed().as_millis() as u64,
                });
            }
            Message::Leave => return,
            Message::RequestSnapshot => {
                let _ = context.hub.send(HubMsg::PeerWantsSnapshot { id });
            }
            Message::TrackRequest { track } => {
                let _ = context.hub.send(HubMsg::PeerRequest { id, track });
            }
            Message::TrackLoadStatus {
                video_id, status, ..
            } if video_id.len() <= 64 => {
                let _ = context.hub.send(HubMsg::PeerLoad {
                    id,
                    video_id,
                    status,
                });
            }
            _ => {}
        }
    }
}

enum GuestEnd {
    /// The user left; nothing to report.
    Left,
    /// The session is over for a reason the user should see.
    Closed(String),
    /// The connection dropped; worth retrying.
    Lost,
}

fn guest_session(
    address: SocketAddr,
    token: &str,
    name: &str,
    origin: Instant,
    wire: &Sender<Wire>,
    commands: &Receiver<GuestCmd>,
) {
    let mut connected_once = false;
    // Consecutive failed reconnects; a successful welcome resets it.
    let mut attempts = 1u32;
    loop {
        let end = guest_connection(address, token, name, origin, wire, commands, &mut attempts);
        connected_once |= attempts == 0;
        match end {
            GuestEnd::Left => return,
            GuestEnd::Closed(reason) => {
                let _ = wire.send(Wire::Closed(reason));
                return;
            }
            GuestEnd::Lost if connected_once && attempts < RECONNECT_ATTEMPTS => {
                attempts += 1;
                let _ = wire.send(Wire::Reconnecting);
                // Stay responsive to "Leave" while waiting to retry.
                match commands.recv_timeout(RECONNECT_DELAY) {
                    Ok(GuestCmd::End) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            GuestEnd::Lost => {
                let reason = if connected_once {
                    "The connection to the Jam was lost."
                } else {
                    "Couldn't reach that Jam. Make sure both devices are on the same Wi-Fi or network and that the host's firewall allows MusiKPL."
                };
                let _ = wire.send(Wire::Closed(reason.into()));
                return;
            }
        }
    }
}

fn guest_connection(
    address: SocketAddr,
    token: &str,
    name: &str,
    origin: Instant,
    wire: &Sender<Wire>,
    commands: &Receiver<GuestCmd>,
    failed_attempts: &mut u32,
) -> GuestEnd {
    let Ok(mut stream) = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) else {
        return GuestEnd::Lost;
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(GUEST_POLL));
    let _ = stream.set_write_timeout(Some(WRITE_TIMEOUT));
    let Ok(read_half) = stream.try_clone() else {
        return GuestEnd::Lost;
    };
    let mut reader = LineReader::new(read_half);
    if write_message(
        &mut stream,
        &Message::Hello {
            version: PROTOCOL_VERSION,
            token: token.to_owned(),
            name: name.to_owned(),
        },
    )
    .is_err()
    {
        return GuestEnd::Lost;
    }

    let clock = |instant: Instant| instant.duration_since(origin).as_millis() as u64;
    let mut sync = ClockSync::default();
    let mut welcomed = false;
    let mut last_heard = Instant::now();
    let mut last_ping: Option<Instant> = None;
    let mut pings = 0u32;

    let end = loop {
        match commands.try_recv() {
            Ok(GuestCmd::End) | Err(mpsc::TryRecvError::Disconnected) => {
                let _ = write_message(&mut stream, &Message::Leave);
                break GuestEnd::Left;
            }
            Ok(GuestCmd::Request(track)) => {
                let _ = write_message(&mut stream, &Message::TrackRequest { track });
            }
            Ok(GuestCmd::LoadStatus {
                revision,
                video_id,
                status,
            }) => {
                let _ = write_message(
                    &mut stream,
                    &Message::TrackLoadStatus {
                        revision,
                        video_id,
                        status,
                    },
                );
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }

        let interval = if pings < PING_BURST_COUNT {
            PING_BURST_INTERVAL
        } else {
            PING_INTERVAL
        };
        if welcomed && !last_ping.is_some_and(|sent| sent.elapsed() < interval) {
            let now = Instant::now();
            if write_message(
                &mut stream,
                &Message::Ping {
                    client_clock_ms: clock(now),
                },
            )
            .is_err()
            {
                break GuestEnd::Lost;
            }
            last_ping = Some(now);
            pings += 1;
        }

        let message = match reader.read_message() {
            Ok(message) => message,
            Err(ReadError::Timeout) => {
                let limit = if welcomed {
                    HOST_SILENCE_TIMEOUT
                } else {
                    HANDSHAKE_TIMEOUT
                };
                if last_heard.elapsed() > limit {
                    break GuestEnd::Lost;
                }
                continue;
            }
            Err(ReadError::Invalid) => continue,
            Err(ReadError::Closed | ReadError::TooLong) => break GuestEnd::Lost,
        };
        last_heard = Instant::now();
        match message {
            Message::Welcome {
                participant_id,
                host_name,
                ..
            } => {
                welcomed = true;
                *failed_attempts = 0;
                let _ = wire.send(Wire::Welcome {
                    participant_id,
                    host_name,
                });
            }
            Message::Snapshot(snapshot) => {
                let _ = wire.send(Wire::Snapshot(snapshot));
            }
            Message::Requests(entries) => {
                let _ = wire.send(Wire::Requests(entries));
            }
            Message::Pong {
                client_clock_ms,
                host_clock_ms,
            } => {
                let offset = sync.record(client_clock_ms, host_clock_ms, clock(Instant::now()));
                let _ = wire.send(Wire::ClockOffset(offset));
            }
            Message::SessionEnded => break GuestEnd::Closed("The host ended the Jam.".into()),
            Message::Error { code, message } => {
                let message = sanitize_text(&message, MAX_TEXT_CHARS);
                if code == ERROR_REQUEST || code == ERROR_FORBIDDEN {
                    let _ = wire.send(Wire::Notice(message));
                } else {
                    break GuestEnd::Closed(message);
                }
            }
            _ => {}
        }
    };
    let _ = stream.shutdown(Shutdown::Both);
    end
}

/// Re-validate a request list received from the host.
fn checked_requests(entries: Vec<RequestEntry>) -> Vec<RequestEntry> {
    entries
        .into_iter()
        .take(64)
        .filter_map(|entry| {
            Some(RequestEntry {
                track: checked_track(&entry.track)?,
                requester: sanitize_text(&entry.requester, MAX_NAME_CHARS),
                ..entry
            })
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
enum ReadError {
    Timeout,
    Closed,
    /// A complete line that is not a valid message; the stream is still usable.
    Invalid,
    TooLong,
}

/// Newline-delimited JSON reader that keeps partial lines across read
/// timeouts and never buffers more than `MAX_LINE` bytes.
struct LineReader<R> {
    inner: R,
    buffer: Vec<u8>,
}

impl<R: Read> LineReader<R> {
    fn new(inner: R) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
        }
    }

    fn read_message(&mut self) -> Result<Message, ReadError> {
        loop {
            if let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=end).collect();
                return serde_json::from_slice(&line).map_err(|_| ReadError::Invalid);
            }
            if self.buffer.len() > MAX_LINE {
                return Err(ReadError::TooLong);
            }
            let mut chunk = [0u8; 4096];
            match self.inner.read(&mut chunk) {
                Ok(0) => return Err(ReadError::Closed),
                Ok(read) => self.buffer.extend_from_slice(&chunk[..read]),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
                {
                    return Err(ReadError::Timeout)
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => return Err(ReadError::Closed),
            }
        }
    }
}

fn write_message(stream: &mut impl Write, message: &Message) -> Result<(), String> {
    let mut line = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    if line.len() > MAX_LINE {
        return Err("Jam message is too large".into());
    }
    line.push(b'\n');
    stream.write_all(&line).map_err(|e| e.to_string())
}

fn make_token() -> String {
    format!("{:06}", rand_seed() % 1_000_000)
}

fn rand_seed() -> u64 {
    use std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    time ^ (std::process::id() as u64).rotate_left(17) ^ COUNTER.fetch_add(1, Ordering::Relaxed)
}

fn normalize_code(code: &str) -> String {
    code.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

/// Parse a typed host address. Only literal IPs are accepted so joining never
/// performs a blocking name lookup; the port is optional.
fn parse_host_address(address: &str) -> Option<SocketAddr> {
    let address = address.trim();
    address.parse::<SocketAddr>().ok().or_else(|| {
        address
            .parse::<Ipv4Addr>()
            .ok()
            .map(|ip| SocketAddr::from((ip, JAM_PORT)))
    })
}

/// The name other devices see for this one: the computer name only.
fn device_name() -> String {
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let name = sanitize_text(&name, MAX_NAME_CHARS);
    if name.is_empty() {
        "MusiKPL".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};

    const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

    fn track(id: &str) -> TrackRef {
        track_ref(id, "Song", "Artist", Some(300.0)).unwrap()
    }

    fn playing(id: &str, position: f64, playing: bool) -> PlaybackState {
        PlaybackState {
            track: Some(track(id)),
            position_secs: position,
            playing,
            speed: 1.0,
        }
    }

    fn host() -> (JamController, SocketAddr) {
        let mut host = JamController::default();
        host.host_with(LOOPBACK, 0, None).expect("host starts");
        let port: u16 = host
            .info
            .address
            .rsplit(':')
            .next()
            .unwrap()
            .parse()
            .unwrap();
        (host, SocketAddr::from((LOOPBACK, port)))
    }

    fn joined_guest(host: &mut JamController, address: SocketAddr) -> JamController {
        let mut guest = JamController::default();
        guest.connect(address, host.info.token.clone(), String::new());
        wait_for(&mut [host, &mut guest], |all| {
            !all[1].info.connecting && all[0].info.peers == 1
        });
        guest
    }

    /// Poll every controller until `done`, collecting their events.
    fn wait_for(
        controllers: &mut [&mut JamController],
        done: impl Fn(&[&mut JamController]) -> bool,
    ) -> Vec<JamEvent> {
        let started = Instant::now();
        let mut events = Vec::new();
        loop {
            for controller in controllers.iter_mut() {
                events.extend(controller.poll());
            }
            if done(controllers) {
                return events;
            }
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "timed out waiting for the Jam to reach the expected state"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn raw_client(address: SocketAddr, token: &str) -> (TcpStream, BufReader<TcpStream>) {
        let mut stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write_message(
            &mut stream,
            &Message::Hello {
                version: PROTOCOL_VERSION,
                token: token.into(),
                name: "raw".into(),
            },
        )
        .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        (stream, reader)
    }

    fn next_message(reader: &mut BufReader<TcpStream>) -> Option<Message> {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(read) if read > 0 => serde_json::from_str(&line).ok(),
            _ => None,
        }
    }

    #[test]
    fn host_codes_are_six_digits() {
        let code = make_token();
        assert!(valid_token(&code));
    }

    #[test]
    fn joining_accepts_spaced_codes() {
        assert_eq!(normalize_code("483 921"), "483921");
    }

    #[test]
    fn typed_addresses_need_no_port_and_never_resolve_names() {
        assert_eq!(
            parse_host_address(" 192.168.1.20 "),
            Some("192.168.1.20:38721".parse().unwrap())
        );
        assert_eq!(
            parse_host_address("192.168.1.20:4000"),
            Some("192.168.1.20:4000".parse().unwrap())
        );
        assert_eq!(parse_host_address("my-laptop"), None);

        let mut guest = JamController::default();
        assert!(guest.join("my-laptop", "123456").is_err());
        assert!(guest.join("192.168.1.20", "12").is_err());
        assert_eq!(guest.info.role, None);
    }

    #[test]
    fn line_reader_keeps_partial_lines_across_timeouts() {
        struct Chunks(Vec<Option<Vec<u8>>>);
        impl Read for Chunks {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                match self.0.pop() {
                    Some(Some(bytes)) => {
                        buffer[..bytes.len()].copy_from_slice(&bytes);
                        Ok(bytes.len())
                    }
                    Some(None) => Err(io::ErrorKind::TimedOut.into()),
                    None => Ok(0),
                }
            }
        }

        let mut line = serde_json::to_vec(&Message::Leave).unwrap();
        line.push(b'\n');
        let (first, second) = line.split_at(3);
        let mut chunks = vec![
            Some(first.to_vec()),
            None,
            Some([second, b"not json\n", &line].concat()),
        ];
        chunks.reverse();
        let mut reader = LineReader::new(Chunks(chunks));

        assert_eq!(reader.read_message(), Err(ReadError::Timeout));
        assert_eq!(reader.read_message(), Ok(Message::Leave));
        assert_eq!(reader.read_message(), Err(ReadError::Invalid));
        assert_eq!(reader.read_message(), Ok(Message::Leave));
        assert_eq!(reader.read_message(), Err(ReadError::Closed));

        let mut flood = LineReader::new(io::repeat(b'x'));
        assert_eq!(flood.read_message(), Err(ReadError::TooLong));
    }

    #[test]
    fn guest_joining_mid_track_receives_the_hosts_live_position() {
        let (mut host, address) = host();
        host.publish(playing("song-a", 40.0, true));
        // The guest arrives after playback has been running for a while.
        thread::sleep(Duration::from_millis(300));
        let host_position = |host: &JamController| {
            expected_position(host.latest_snapshot().unwrap(), host.clock_ms())
        };

        let mut guest = JamController::default();
        guest.connect(address, host.info.token.clone(), String::new());
        let events = wait_for(&mut [&mut host, &mut guest], |all| {
            all[1].latest_snapshot().is_some() && all[1].clock_offset_ms != 0
        });
        assert!(events
            .iter()
            .any(|event| matches!(event, JamEvent::Snapshot(_))));

        let snapshot = guest.latest_snapshot().unwrap().clone();
        assert_eq!(snapshot.reason, SyncReason::TrackChanged);
        assert_eq!(snapshot.track.as_ref().unwrap().video_id, "song-a");
        assert!(snapshot.playing);
        // Not "start from zero": the guest projects where the host is now.
        let guest_view = expected_position(&snapshot, guest.host_now_ms());
        assert!(guest_view > 40.25, "guest projected {guest_view}");
        assert!(
            (guest_view - host_position(&host)).abs() < 0.1,
            "guest {guest_view} vs host {}",
            host_position(&host)
        );
        assert_eq!(guest.info.host_name, host.info.host_name);
    }

    #[test]
    fn pause_seek_and_track_changes_reach_guests_in_order() {
        let (mut host, address) = host();
        host.publish(playing("song-a", 10.0, true));
        let mut guest = joined_guest(&mut host, address);
        let mut reasons = Vec::new();
        let mut collect = |host: &mut JamController, guest: &mut JamController, revision: u64| {
            let events = wait_for(&mut [host, guest], |all| {
                all[1]
                    .latest_snapshot()
                    .is_some_and(|snapshot| snapshot.revision >= revision)
            });
            for event in events {
                if let JamEvent::Snapshot(snapshot) = event {
                    reasons.push((snapshot.revision, snapshot.reason));
                }
            }
        };

        // Joining delivered the state that was current at that moment.
        collect(&mut host, &mut guest, 1);
        assert_eq!(
            guest.latest_snapshot().unwrap().reason,
            SyncReason::TrackChanged
        );
        host.publish(playing("song-a", 10.1, false));
        collect(&mut host, &mut guest, 2);
        assert!(!guest.latest_snapshot().unwrap().playing);

        host.publish(playing("song-a", 120.0, false));
        collect(&mut host, &mut guest, 3);
        assert_eq!(guest.latest_snapshot().unwrap().position_secs, 120.0);

        host.publish(playing("song-a", 120.0, true));
        collect(&mut host, &mut guest, 4);
        host.publish(playing("song-b", 0.0, true));
        collect(&mut host, &mut guest, 5);
        assert_eq!(
            guest
                .latest_snapshot()
                .unwrap()
                .track
                .as_ref()
                .unwrap()
                .video_id,
            "song-b"
        );

        assert_eq!(
            reasons,
            vec![
                (2, SyncReason::Paused),
                (3, SyncReason::Seeked),
                (4, SyncReason::Started),
                (5, SyncReason::TrackChanged),
            ]
        );

        // An unchanged state right away is not re-sent.
        host.publish(playing("song-b", 0.0, true));
        assert_eq!(host.latest_snapshot().unwrap().revision, 5);
    }

    #[test]
    fn guest_requests_need_the_hosts_approval_to_reach_playback() {
        let (mut host, address) = host();
        let mut guest = joined_guest(&mut host, address);

        guest.request_track(track("wanted")).unwrap();
        wait_for(&mut [&mut host, &mut guest], |all| {
            all[0].requests().len() == 1 && all[1].requests().len() == 1
        });
        let request = host.requests()[0].clone();
        assert_eq!(request.status, RequestStatus::Pending);
        assert_eq!(Some(request.requester_id), guest.participant_id());
        // Nothing was published or changed on the host's playback.
        assert!(host.latest_snapshot().is_none());

        // A guest controller has no authority over the request list.
        assert_eq!(guest.accept_request(request.id), None);
        assert_eq!(guest.remove_request(request.id), None);

        // A duplicate is declined with an explanation, not silently dropped.
        thread::sleep(Duration::from_millis(1_600));
        guest.request_track(track("wanted")).unwrap();
        let mut events = Vec::new();
        let started = Instant::now();
        while !events
            .iter()
            .any(|event| matches!(event, JamEvent::Notice(_)))
        {
            assert!(started.elapsed() < Duration::from_secs(5));
            events.extend(host.poll());
            events.extend(guest.poll());
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(host.requests().len(), 1);

        assert_eq!(host.accept_request(request.id), Some(track("wanted")));
        wait_for(&mut [&mut host, &mut guest], |all| {
            all[1].requests()[0].status == RequestStatus::Accepted
        });

        host.reconcile_requests(Some("wanted"), |_| false);
        wait_for(&mut [&mut host, &mut guest], |all| {
            all[1].requests()[0].status == RequestStatus::Played
        });
    }

    #[test]
    fn host_refuses_wrong_codes_and_host_only_messages_from_guests() {
        let (mut host, address) = host();
        host.publish(playing("song-a", 10.0, false));

        let (_stream, mut reader) = raw_client(address, "000000x");
        assert!(matches!(
            next_message(&mut reader),
            Some(Message::Error { code, .. }) if code == "unauthorized"
        ));
        assert_eq!(next_message(&mut reader), None);

        let token = host.info.token.clone();
        let (mut stream, mut reader) = raw_client(address, &token);
        assert!(matches!(
            next_message(&mut reader),
            Some(Message::Welcome { .. })
        ));
        wait_for(&mut [&mut host], |all| all[0].info.peers == 1);

        // A rogue guest tries to take over playback and end the session.
        let mut forged = host.latest_snapshot().unwrap().clone();
        forged.revision = 9_999;
        forged.playing = true;
        forged.position_secs = 200.0;
        for message in [
            Message::Snapshot(forged),
            Message::SessionEnded,
            Message::Requests(vec![]),
        ] {
            write_message(&mut stream, &message).unwrap();
        }
        let mut refusals = 0;
        while let Some(message) = next_message(&mut reader) {
            if matches!(&message, Message::Error { code, .. } if code == ERROR_FORBIDDEN) {
                refusals += 1;
            }
        }
        assert_eq!(refusals, 3);

        // The host's state is untouched and the offender was disconnected.
        wait_for(&mut [&mut host], |all| all[0].info.peers == 0);
        let snapshot = host.latest_snapshot().unwrap();
        assert_eq!(snapshot.revision, 1);
        assert!(!snapshot.playing);
        assert_eq!(host.info.role, Some(Role::Host));
    }

    #[test]
    fn host_knows_when_every_guest_has_loaded_the_track() {
        let (mut host, address) = host();
        assert!(
            host.guests_ready("song-a"),
            "no guests means nothing to wait for"
        );

        let mut first = joined_guest(&mut host, address);
        let mut second = JamController::default();
        second.connect(address, host.info.token.clone(), String::new());
        wait_for(&mut [&mut host, &mut first, &mut second], |all| {
            all[0].info.peers == 2 && !all[2].info.connecting
        });
        assert!(!host.guests_ready("song-a"));

        first.report_load(1, "song-a", LoadStatus::Loading);
        second.report_load(1, "song-a", LoadStatus::Ready);
        wait_for(&mut [&mut host, &mut first, &mut second], |all| {
            all[0].guest_loads.len() == 2
        });
        assert!(!host.guests_ready("song-a"), "one guest is still loading");

        first.report_load(1, "song-a", LoadStatus::Ready);
        wait_for(&mut [&mut host, &mut first, &mut second], |all| {
            all[0].guests_ready("song-a")
        });
        // Readiness is per track.
        assert!(!host.guests_ready("song-b"));

        // A guest that fails or leaves must not hold everyone else up.
        first.report_load(2, "song-b", LoadStatus::Failed);
        second.leave();
        wait_for(&mut [&mut host, &mut first], |all| {
            all[0].info.peers == 1 && all[0].guests_ready("song-b")
        });
    }

    #[test]
    fn leaving_and_ending_clean_up_both_sides() {
        let (mut host, address) = host();
        let mut first = joined_guest(&mut host, address);
        let mut second = JamController::default();
        second.connect(address, host.info.token.clone(), String::new());
        wait_for(&mut [&mut host, &mut first, &mut second], |all| {
            all[0].info.peers == 2
        });

        first.leave();
        assert_eq!(first.info.role, None);
        wait_for(&mut [&mut host, &mut second], |all| all[0].info.peers == 1);

        host.leave();
        assert_eq!(host.info.role, None);
        let events = wait_for(&mut [&mut second], |all| all[0].info.role.is_none());
        assert!(events.contains(&JamEvent::SessionClosed));
        assert_eq!(
            second.info.notice.as_deref(),
            Some("The host ended the Jam.")
        );
        assert!(second.latest_snapshot().is_none());
        // The port is released: nothing is listening any more.
        thread::sleep(Duration::from_millis(100));
        assert!(TcpStream::connect_timeout(&address, Duration::from_millis(500)).is_err());
    }

    #[test]
    fn unreachable_host_reports_an_actionable_error_without_blocking() {
        // A port nothing listens on.
        let free = TcpListener::bind((LOOPBACK, 0)).unwrap();
        let address = free.local_addr().unwrap();
        drop(free);

        let mut guest = JamController::default();
        let started = Instant::now();
        guest.connect(address, "123456".into(), "Desk".into());
        // Connecting happens off the calling thread.
        assert!(started.elapsed() < Duration::from_millis(200));
        assert!(guest.info.connecting);

        wait_for(&mut [&mut guest], |all| all[0].info.role.is_none());
        assert!(guest
            .info
            .notice
            .as_deref()
            .is_some_and(|notice| notice.contains("same Wi-Fi")));
    }

    #[test]
    fn nearby_jams_are_discovered_joined_and_expire() {
        let probe = UdpSocket::bind((LOOPBACK, 0)).unwrap();
        let discovery_port = probe.local_addr().unwrap().port();
        drop(probe);

        let mut host = JamController::default();
        host.host_with(LOOPBACK, 0, Some(discovery_port))
            .expect("host starts");
        assert!(host.info.discoverable);
        host.publish(playing("song-a", 0.0, true));

        let browser = discovery::start_for_test(SocketAddr::from((LOOPBACK, discovery_port)));
        let started = Instant::now();
        let found = loop {
            if let Some(jam) = browser.nearby().into_iter().next() {
                break jam;
            }
            assert!(started.elapsed() < Duration::from_secs(10), "no Jam found");
            thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(found.host_name, host.info.host_name);
        assert_eq!(found.track.as_deref(), Some("Song"));
        assert_eq!(found.listeners, 0);

        // One click: everything needed to connect came from discovery.
        let mut guest = JamController::default();
        guest.join_nearby(&found);
        wait_for(&mut [&mut host, &mut guest], |all| {
            !all[1].info.connecting && all[0].info.peers == 1
        });

        host.leave();
        let started = Instant::now();
        while !browser.nearby().is_empty() {
            assert!(
                started.elapsed() < Duration::from_secs(15),
                "ended Jam never expired"
            );
            thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(browser.status(), DiscoveryStatus::Ready);
    }
}
