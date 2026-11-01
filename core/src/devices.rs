//! Finding the user's other devices on the same Wi-Fi: each running
//! MusiKPL with Last.fm connected says "I'm <device>, <friend id>, Last.fm
//! <user>" now and then, and listens for the others. Hearing a device of the
//! same Last.fm user is what offers to link it (the user confirms once);
//! after that, linked devices sync over the friends network, anywhere.
//!
//! Only public ids travel here; nothing is trusted until the user links.

use std::{
    collections::HashMap,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

/// The port devices announce themselves on.
pub const DEVICE_PORT: u16 = 38_723;
const ANNOUNCE_EVERY: Duration = Duration::from_secs(6);
/// A device not heard from in this long is gone.
const FORGET_AFTER: Duration = Duration::from_secs(30);
const MAX_PACKET: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Announcement {
    musikpl_device: u8,
    /// The device's friend id (public key).
    id: String,
    name: String,
    lastfm: String,
}

/// A device heard on the Wi-Fi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeardDevice {
    pub id: String,
    pub name: String,
    pub lastfm: String,
}

/// Announces this device and listens for others, on a thread, until
/// dropped.
pub struct DeviceFinder {
    heard: Arc<Mutex<HashMap<String, (HeardDevice, Instant)>>>,
    stop: Arc<AtomicBool>,
}

impl DeviceFinder {
    pub fn start(id: String, name: String, lastfm: String) -> Self {
        let heard = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (thread_heard, thread_stop) = (heard.clone(), stop.clone());
        let me = Announcement {
            musikpl_device: 1,
            id,
            name,
            lastfm,
        };
        thread::Builder::new()
            .name("musikpl-devices".into())
            .spawn(move || run(me, &thread_heard, &thread_stop))
            .ok();
        Self { heard, stop }
    }

    /// Devices heard recently (not this one).
    pub fn heard(&self) -> Vec<HeardDevice> {
        let Ok(mut heard) = self.heard.lock() else {
            return Vec::new();
        };
        heard.retain(|_, (_, at)| at.elapsed() < FORGET_AFTER);
        heard.values().map(|(device, _)| device.clone()).collect()
    }
}

impl Drop for DeviceFinder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn targets() -> Vec<SocketAddr> {
    let mut addresses = vec![Ipv4Addr::BROADCAST];
    for (ip, prefix) in crate::net::ipv4_networks() {
        if ip.is_private() && (8..=30).contains(&prefix) {
            let host_bits = u32::MAX >> prefix;
            addresses.push(Ipv4Addr::from(u32::from(ip) | host_bits));
        }
    }
    addresses.dedup();
    addresses
        .into_iter()
        .map(|address| SocketAddr::from((address, DEVICE_PORT)))
        .collect()
}

fn run(
    me: Announcement,
    heard: &Mutex<HashMap<String, (HeardDevice, Instant)>>,
    stop: &AtomicBool,
) {
    // The well-known port to hear others; if another program has it, this
    // device still announces itself (and is found by the others).
    let listener = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, DEVICE_PORT))
        .or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)));
    let Ok(socket) = listener else {
        return;
    };
    let _ = socket.set_broadcast(true);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(500)));
    let packet = serde_json::to_vec(&me).unwrap_or_default();
    let mut buffer = [0u8; MAX_PACKET];
    let mut last_announce = Instant::now() - ANNOUNCE_EVERY;
    while !stop.load(Ordering::Relaxed) {
        if last_announce.elapsed() >= ANNOUNCE_EVERY {
            last_announce = Instant::now();
            for target in targets() {
                let _ = socket.send_to(&packet, target);
            }
        }
        let Ok((length, _)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        let Ok(other) = serde_json::from_slice::<Announcement>(&buffer[..length]) else {
            continue;
        };
        if other.musikpl_device != 1 || other.id == me.id || other.id.len() > 100 {
            continue;
        }
        let device = HeardDevice {
            id: other.id.clone(),
            name: other.name.chars().take(40).collect(),
            lastfm: other.lastfm.chars().take(40).collect(),
        };
        if let Ok(mut heard) = heard.lock() {
            if heard.len() < 32 || heard.contains_key(&other.id) {
                heard.insert(other.id, (device, Instant::now()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn announcements_are_small_and_read_back() {
        let me = Announcement {
            musikpl_device: 1,
            id: "a".repeat(64),
            name: "Galaxy A56".into(),
            lastfm: "deathirs".into(),
        };
        let packet = serde_json::to_vec(&me).unwrap();
        assert!(packet.len() < MAX_PACKET);
        assert_eq!(serde_json::from_slice::<Announcement>(&packet).unwrap(), me);
    }
}
