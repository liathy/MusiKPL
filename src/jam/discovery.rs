//! Nearby-Jam discovery over UDP on the local network.
//!
//! A browsing device broadcasts a small query; every hosting device answers
//! the sender directly. Only hosts bind the well-known port, so browsing needs
//! no inbound firewall rule and several browsers can share one machine.

use std::{
    io,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use super::protocol::{decode_discovery, discovery_query, DiscoveryPacket, MAX_DISCOVERY_BYTES};

pub(crate) const DISCOVERY_PORT: u16 = 38_722;
/// How often browsers ask; a new Jam appears within this long.
const QUERY_INTERVAL: Duration = Duration::from_secs(2);
/// Two missed answers plus margin before a Jam is treated as gone.
const STALE_AFTER: Duration = Duration::from_secs(5);
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_NEARBY: usize = 16;
const MAX_QUERIES_PER_TICK: usize = 16;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NearbyJam {
    pub(crate) session_id: u64,
    pub(crate) host_name: String,
    pub(crate) address: SocketAddr,
    pub(crate) token: String,
    pub(crate) track: Option<String>,
    pub(crate) listeners: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DiscoveryStatus {
    /// The first query is still in flight.
    Searching,
    /// Queries are going out; the list reflects what answered.
    Ready,
    /// The network or firewall will not carry discovery traffic.
    Unavailable(String),
}

struct Shared {
    jams: Vec<(NearbyJam, Instant)>,
    status: DiscoveryStatus,
}

/// Background browser for nearby Jams. Dropping it stops the thread.
pub(crate) struct Discovery {
    shared: Arc<Mutex<Shared>>,
    enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
}

impl Discovery {
    pub(crate) fn start(port: u16) -> Self {
        Self::start_with(move || query_targets(port))
    }

    fn start_with(targets: impl Fn() -> Vec<SocketAddr> + Send + 'static) -> Self {
        let discovery = Self {
            shared: Arc::new(Mutex::new(Shared {
                jams: Vec::new(),
                status: DiscoveryStatus::Searching,
            })),
            enabled: Arc::new(AtomicBool::new(true)),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let (shared, enabled, stop) = (
            discovery.shared.clone(),
            discovery.enabled.clone(),
            discovery.stop.clone(),
        );
        thread::spawn(move || browse(targets, &shared, &enabled, &stop));
        discovery
    }

    /// Pause browsing while this device is itself in a session.
    pub(crate) fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub(crate) fn nearby(&self) -> Vec<NearbyJam> {
        self.shared
            .lock()
            .map(|shared| shared.jams.iter().map(|(jam, _)| jam.clone()).collect())
            .unwrap_or_default()
    }

    pub(crate) fn status(&self) -> DiscoveryStatus {
        self.shared
            .lock()
            .map(|shared| shared.status.clone())
            .unwrap_or(DiscoveryStatus::Searching)
    }
}

impl Drop for Discovery {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn browse(
    targets: impl Fn() -> Vec<SocketAddr>,
    shared: &Mutex<Shared>,
    enabled: &AtomicBool,
    stop: &AtomicBool,
) {
    let set_status = |status: DiscoveryStatus| {
        if let Ok(mut shared) = shared.lock() {
            shared.status = status;
        }
    };
    let socket = match open_browser_socket() {
        Ok(socket) => socket,
        Err(error) => {
            set_status(DiscoveryStatus::Unavailable(format!(
                "Nearby Jams can't be found automatically on this device ({error})."
            )));
            return;
        }
    };
    let query = serde_json::to_vec(&discovery_query()).unwrap_or_default();
    let mut buffer = [0u8; MAX_DISCOVERY_BYTES + 1];

    while !stop.load(Ordering::Relaxed) {
        if !enabled.load(Ordering::Relaxed) {
            if let Ok(mut shared) = shared.lock() {
                shared.jams.clear();
            }
            thread::sleep(RECEIVE_TIMEOUT);
            continue;
        }

        let sent = targets()
            .iter()
            .filter(|target| socket.send_to(&query, target).is_ok())
            .count();
        let cycle = Instant::now();
        while cycle.elapsed() < QUERY_INTERVAL
            && !stop.load(Ordering::Relaxed)
            && enabled.load(Ordering::Relaxed)
        {
            match socket.recv_from(&mut buffer) {
                Ok((length, source)) => {
                    if let Some(jam) = nearby_from(&buffer[..length], source) {
                        remember(shared, jam);
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                // Windows reports an ICMP "port unreachable" from an earlier
                // send here; it is not fatal, but must not become a busy loop.
                Err(_) => thread::sleep(Duration::from_millis(50)),
            }
        }

        if let Ok(mut shared) = shared.lock() {
            shared.jams.retain(|(_, seen)| seen.elapsed() < STALE_AFTER);
            shared.status = if sent == 0 {
                DiscoveryStatus::Unavailable(
                    "This network is blocking automatic Jam discovery.".into(),
                )
            } else {
                DiscoveryStatus::Ready
            };
        }
    }
}

fn open_browser_socket() -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(RECEIVE_TIMEOUT))?;
    Ok(socket)
}

fn nearby_from(datagram: &[u8], source: SocketAddr) -> Option<NearbyJam> {
    match decode_discovery(datagram)? {
        DiscoveryPacket::Announce {
            session_id,
            host_name,
            port,
            token,
            track,
            listeners,
            ..
        } => Some(NearbyJam {
            session_id,
            host_name,
            // The host is reached where its answer came from; it never has
            // to know or advertise its own address.
            address: SocketAddr::new(source.ip(), port),
            token,
            track,
            listeners,
        }),
        DiscoveryPacket::Query { .. } => None,
    }
}

fn remember(shared: &Mutex<Shared>, jam: NearbyJam) {
    let Ok(mut shared) = shared.lock() else {
        return;
    };
    let now = Instant::now();
    if let Some((known, seen)) = shared
        .jams
        .iter_mut()
        .find(|(known, _)| known.session_id == jam.session_id)
    {
        // A host can answer over several routes; keep the first working one.
        let address = known.address;
        *known = NearbyJam { address, ..jam };
        *seen = now;
    } else if shared.jams.len() < MAX_NEARBY {
        shared.jams.push((jam, now));
    }
}

/// Where queries are sent. `std` cannot enumerate interfaces, so this covers
/// the limited broadcast, the primary interface's /24, and this machine.
fn query_targets(port: u16) -> Vec<SocketAddr> {
    let mut targets = vec![SocketAddr::from((Ipv4Addr::BROADCAST, port))];
    if let Some(local) = local_ipv4().filter(Ipv4Addr::is_private) {
        let [a, b, c, _] = local.octets();
        targets.push(SocketAddr::from((Ipv4Addr::new(a, b, c, 255), port)));
    }
    targets.push(SocketAddr::from((Ipv4Addr::LOCALHOST, port)));
    targets
}

/// The address of the interface used for the default route, if any.
///
/// Connecting a UDP socket only selects a route; no packet is sent.
pub(crate) fn local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(address) if !address.ip().is_unspecified() => Some(*address.ip()),
        _ => None,
    }
}

/// Bind the host-side socket that answers discovery queries.
pub(crate) fn bind_responder(address: Ipv4Addr, port: u16) -> io::Result<UdpSocket> {
    let socket = UdpSocket::bind((address, port))?;
    socket.set_nonblocking(true)?;
    Ok(socket)
}

/// Answer the queries waiting on a host's responder socket.
pub(crate) fn answer_queries(socket: &UdpSocket, announce: &DiscoveryPacket) {
    let mut buffer = [0u8; MAX_DISCOVERY_BYTES + 1];
    let mut reply: Option<Vec<u8>> = None;
    for _ in 0..MAX_QUERIES_PER_TICK {
        let Ok((length, source)) = socket.recv_from(&mut buffer) else {
            return;
        };
        if !matches!(
            decode_discovery(&buffer[..length]),
            Some(DiscoveryPacket::Query { .. })
        ) {
            continue;
        }
        let reply = reply.get_or_insert_with(|| serde_json::to_vec(announce).unwrap_or_default());
        let _ = socket.send_to(reply, source);
    }
}

#[cfg(test)]
pub(crate) fn start_for_test(target: SocketAddr) -> Discovery {
    Discovery::start_with(move || vec![target])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jam::protocol::{DISCOVERY_APP, PROTOCOL_VERSION};

    #[test]
    fn announcements_use_the_senders_address_and_advertised_port() {
        let datagram = serde_json::to_vec(&DiscoveryPacket::Announce {
            app: DISCOVERY_APP.into(),
            version: PROTOCOL_VERSION,
            session_id: 9,
            host_name: "Desk".into(),
            port: 4_100,
            token: "123456".into(),
            track: None,
            listeners: 0,
        })
        .unwrap();
        let source: SocketAddr = "192.168.1.20:38722".parse().unwrap();

        let jam = nearby_from(&datagram, source).expect("valid announce");
        assert_eq!(jam.address, "192.168.1.20:4100".parse().unwrap());
        assert!(nearby_from(b"garbage", source).is_none());
        // A query is not a Jam.
        let query = serde_json::to_vec(&discovery_query()).unwrap();
        assert!(nearby_from(&query, source).is_none());
    }

    #[test]
    fn queries_always_reach_broadcast_and_this_machine() {
        let targets = query_targets(4_000);
        assert!(targets.contains(&"255.255.255.255:4000".parse().unwrap()));
        assert!(targets.contains(&"127.0.0.1:4000".parse().unwrap()));
    }
}
