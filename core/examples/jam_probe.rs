//! A Jam guest without an interface, for testing Jams between devices:
//!
//!   cargo run -p musikpl-core --example jam_probe            (joins the first Jam nearby)
//!   cargo run -p musikpl-core --example jam_probe -- INVITE  (joins over the internet)
//!   cargo run -p musikpl-core --example jam_probe -- --to 192.168.1.67
//!     (asks that device directly instead of broadcasting, which works
//!     without a firewall rule: the answer comes from where it was sent)
//!
//! It prints what it sees for a minute: the connection, the host's song
//! and position, and the shared queue.

use std::time::{Duration, Instant};

use musikpl_core::jam::JamController;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let invite = args.first().filter(|arg| !arg.starts_with("--")).cloned();
    let direct: Option<std::net::Ipv4Addr> = args.iter().position(|arg| arg == "--to").map(|at| {
        args.get(at + 1)
            .and_then(|ip| ip.parse().ok())
            .expect("--to IP")
    });
    let mut jam = JamController::default();
    jam.start_discovery();
    let mut joined = invite.is_some();
    if let Some(invite) = &invite {
        jam.join_invite(invite).expect("a valid invite");
        println!("joining over the internet…");
    }
    if let Some(ip) = direct {
        let found = ask_directly(ip).expect("the device answered the discovery query");
        println!(
            "{} answered: {}'s Jam at {} ({} listening)",
            ip, found.host_name, found.address, found.listeners
        );
        jam.join_nearby(&found);
        joined = true;
    }
    let started = Instant::now();
    let mut last_print = Instant::now() - Duration::from_secs(10);
    while started.elapsed() < Duration::from_secs(60) {
        jam.poll();
        if !joined {
            if let Some(found) = jam.nearby().into_iter().next() {
                println!(
                    "found {}'s Jam nearby at {} ({} listening)",
                    found.host_name, found.address, found.listeners
                );
                jam.join_nearby(&found);
                joined = true;
            }
        }
        if last_print.elapsed() > Duration::from_secs(2) {
            last_print = Instant::now();
            let info = &jam.info;
            let song = jam.latest_snapshot().and_then(|snapshot| {
                snapshot
                    .track
                    .as_ref()
                    .map(|t| (t.title.clone(), snapshot.position_secs, snapshot.playing))
            });
            println!(
                "[{:>2}s] role={:?} connecting={} host={:?} status={:?} song={:?} queue={}",
                started.elapsed().as_secs(),
                info.role,
                info.connecting,
                info.host_name,
                info.status,
                song,
                jam.queue().len()
            );
            if info.role.is_some()
                && !info.connecting
                && song.is_some()
                && started.elapsed() > Duration::from_secs(12)
            {
                println!("connected and following the host: OK");
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    jam.leave();
}

/// Ask one device for its Jam with the same query discovery broadcasts.
fn ask_directly(ip: std::net::Ipv4Addr) -> Option<musikpl_core::jam::NearbyJam> {
    use musikpl_core::jam::{decode_discovery, discovery_query, DiscoveryPacket, NearbyJam};
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let query = serde_json::to_vec(&discovery_query()).ok()?;
    let mut buffer = [0u8; 4096];
    for _ in 0..5 {
        socket.send_to(&query, (ip, 38_722)).ok()?;
        if let Ok((length, source)) = socket.recv_from(&mut buffer) {
            if let Some(DiscoveryPacket::Announce {
                session_id,
                host_name,
                port,
                token,
                track,
                listeners,
                ..
            }) = decode_discovery(&buffer[..length])
            {
                return Some(NearbyJam {
                    session_id,
                    host_name,
                    address: std::net::SocketAddr::new(source.ip(), port),
                    token,
                    track,
                    listeners,
                });
            }
        }
    }
    None
}
