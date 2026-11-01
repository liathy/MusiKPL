//! The user's own devices (PC, phone): found on the same Wi-Fi when they
//! use the same Last.fm account, linked once (the user confirms on each),
//! and from then on kept in sync whenever both are open, anywhere: playlists
//! with their songs, order, favorites and cover photos.
//!
//! Syncing goes over the friends network (end-to-end encrypted, no server),
//! and only devices the user linked may send it.

use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::{
    covers,
    devices::DeviceFinder,
    domain::{Playlist, Tombstone},
    friends::my_code,
    storage,
};

use super::Engine;

const DEVICES_FILE: &str = "devices.json";
/// How often linked devices are checked: often while the user looks at
/// them, rarely otherwise.
const CHECK_SHOWN: Duration = Duration::from_secs(20);
const CHECK_HIDDEN: Duration = Duration::from_secs(120);
const NOTICE_TIME: Duration = Duration::from_secs(8);

/// One of the user's own devices, linked to this one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinkedDevice {
    pub id: String,
    pub name: String,
}

/// A device waiting for the user to say Link or Not now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkPrompt {
    pub id: String,
    pub name: String,
}

/// What one device sends another.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncData {
    playlists: Vec<Playlist>,
    deleted: Vec<Tombstone>,
    /// Cover photos by file name, as base64 JPEG.
    covers: HashMap<String, String>,
}

#[derive(Default)]
pub struct DevicesState {
    pub linked: Vec<LinkedDevice>,
    finder: Option<DeviceFinder>,
    pub prompts: Vec<LinkPrompt>,
    /// Devices the user said Not now to, this run.
    declined: HashSet<String>,
    last_check: Option<Instant>,
    /// What was last sent to each device, so unchanged data isn't resent.
    sent: HashMap<String, u64>,
    /// A sync on its way, per device.
    sending: HashSet<String>,
    /// Last successful sync with each device.
    pub synced_at: HashMap<String, Instant>,
    notice: Option<(String, Instant)>,
}

impl DevicesState {
    pub fn ids(&self) -> Vec<String> {
        self.linked.iter().map(|device| device.id.clone()).collect()
    }

    pub fn is_linked(&self, id: &str) -> bool {
        self.linked.iter().any(|device| device.id == id)
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < NOTICE_TIME)
            .map(|(text, _)| text.as_str())
    }
}

/// A short fingerprint of the playlists, to know when they changed.
fn stamp(engine: &Engine) -> u64 {
    let playlists = &engine.library.playlists;
    let mut stamp: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |value: u64| {
        stamp ^= value;
        stamp = stamp.wrapping_mul(0x0100_0000_01b3);
    };
    for playlist in playlists.iter() {
        mix(playlist.modified);
        for byte in playlist.uid.bytes() {
            mix(u64::from(byte));
        }
    }
    mix(playlists.tombstones().len() as u64);
    stamp
}

impl Engine {
    pub fn load_devices(&mut self) {
        self.devices.linked = storage::read(DEVICES_FILE).unwrap_or_default();
    }

    fn save_devices(&mut self) {
        storage::write_in_background(DEVICES_FILE, &self.devices.linked);
        self.allow_peers();
    }

    fn set_device_notice(&mut self, text: impl Into<String>) {
        self.devices.notice = Some((text.into(), Instant::now()));
    }

    /// This device's name for the others.
    fn device_label(&self) -> String {
        crate::platform::device_name().unwrap_or_else(|| self.friend_name())
    }

    /// Every tick: look for the user's devices on the Wi-Fi (with Last.fm
    /// connected), and keep linked ones in sync.
    pub fn tick_devices(&mut self) {
        let user = self.settings.lastfm.username.clone();
        let connected = self.lastfm_connected() && !user.is_empty();
        if connected && self.devices.finder.is_none() {
            self.devices.finder = Some(DeviceFinder::start(
                my_code("").id,
                self.device_label(),
                user.clone(),
            ));
        } else if !connected {
            self.devices.finder = None;
        }
        // The same Last.fm user on another device nearby: offer to link.
        let heard = self
            .devices
            .finder
            .as_ref()
            .map(DeviceFinder::heard)
            .unwrap_or_default();
        for device in heard {
            let same_user = device.lastfm.eq_ignore_ascii_case(&user);
            if same_user
                && !self.devices.is_linked(&device.id)
                && !self.devices.declined.contains(&device.id)
                && !self.devices.prompts.iter().any(|p| p.id == device.id)
            {
                self.devices.prompts.push(LinkPrompt {
                    id: device.id,
                    name: device.name,
                });
                (self.waker)();
            }
        }

        if self.devices.linked.is_empty() {
            return;
        }
        let interval = if self.menu_visible {
            CHECK_SHOWN
        } else {
            CHECK_HIDDEN
        };
        if self
            .devices
            .last_check
            .is_none_or(|at| at.elapsed() >= interval)
        {
            self.devices.last_check = Some(Instant::now());
            if let Some(service) = &self.friends_service() {
                service.check(self.devices.ids());
            }
        }
        // Send to linked devices that are online and haven't got the latest.
        let now = stamp(self);
        let due: Vec<String> = self
            .devices
            .linked
            .iter()
            .filter(|device| self.friends.online.get(&device.id) == Some(&true))
            .filter(|device| self.devices.sent.get(&device.id) != Some(&now))
            .filter(|device| !self.devices.sending.contains(&device.id))
            .map(|device| device.id.clone())
            .collect();
        for id in due {
            self.send_sync(&id, now);
        }
    }

    fn send_sync(&mut self, id: &str, now: u64) {
        let data = self.sync_data();
        let Ok(text) = serde_json::to_string(&data) else {
            return;
        };
        if let Some(service) = &self.friends_service() {
            service.sync(id.to_owned(), text);
            self.devices.sending.insert(id.to_owned());
            self.devices.sent.insert(id.to_owned(), now);
        }
    }

    fn sync_data(&self) -> SyncData {
        let playlists: Vec<Playlist> = self.library.playlists.iter().cloned().collect();
        let covers = playlists
            .iter()
            .filter_map(|playlist| playlist.cover.clone())
            .filter_map(|name| {
                covers::read_cover(&name).map(|bytes| (name, STANDARD.encode(bytes)))
            })
            .collect();
        SyncData {
            playlists,
            deleted: self.library.playlists.tombstones().to_vec(),
            covers,
        }
    }

    /// A linked device sent its data: merge it.
    pub fn receive_sync(&mut self, id: &str, text: &str) {
        // Friends share the network; only the user's own devices may sync.
        if !self.devices.is_linked(id) {
            return;
        }
        let Ok(data) = serde_json::from_str::<SyncData>(text) else {
            return;
        };
        let merged = self.library.playlists.merge(&data.playlists, &data.deleted);
        for uid in &merged.taken {
            let cover = self
                .library
                .playlists
                .iter()
                .find(|playlist| &playlist.uid == uid)
                .and_then(|playlist| playlist.cover.clone());
            if let Some(name) = cover {
                if let Some(bytes) = data
                    .covers
                    .get(&name)
                    .and_then(|encoded| STANDARD.decode(encoded).ok())
                {
                    covers::write_cover(&name, &bytes);
                }
            }
        }
        for name in merged.dropped_covers {
            self.forget_cover(&name);
        }
        if merged.changed {
            self.library.playlists_changed = true;
        }
        self.devices.synced_at.insert(id.to_owned(), Instant::now());
        // They may lack what's here: send it back (once; unchanged data
        // isn't sent again).
        self.devices.sent.remove(id);
        self.friends.online.insert(id.to_owned(), true);
    }

    pub fn sync_finished(&mut self, id: &str, delivered: bool) {
        self.devices.sending.remove(id);
        if delivered {
            self.devices.synced_at.insert(id.to_owned(), Instant::now());
        } else {
            // Try again when it's back.
            self.devices.sent.remove(id);
            self.friends.online.insert(id.to_owned(), false);
        }
    }

    /// Another device asks to link: if it's the same Last.fm user, ask ours.
    pub fn link_requested(&mut self, id: &str, name: &str, lastfm: &str) {
        let user = &self.settings.lastfm.username;
        if user.is_empty() || !lastfm.eq_ignore_ascii_case(user) {
            return;
        }
        if self.devices.is_linked(id) {
            // Linked here already: they linked too, so sync now.
            self.devices.sent.remove(id);
            self.friends.online.insert(id.to_owned(), true);
            return;
        }
        if !self.devices.prompts.iter().any(|prompt| prompt.id == id) {
            self.devices.declined.remove(id);
            self.devices.prompts.push(LinkPrompt {
                id: id.to_owned(),
                name: name.to_owned(),
            });
        }
    }

    /// The user said Link: trust the device, ask it to link back, sync.
    pub fn accept_link(&mut self, id: &str) {
        let Some(index) = self.devices.prompts.iter().position(|p| p.id == id) else {
            return;
        };
        let prompt = self.devices.prompts.remove(index);
        if !self.devices.is_linked(id) {
            self.devices.linked.push(LinkedDevice {
                id: prompt.id.clone(),
                name: prompt.name.clone(),
            });
            self.save_devices();
        }
        self.ensure_friends_service();
        if let Some(service) = &self.friends_service() {
            service.link(prompt.id.clone(), self.settings.lastfm.username.clone());
        }
        self.devices.sent.remove(id);
        self.devices.last_check = None;
        self.set_device_notice(format!(
            "Linked {}. Playlists sync whenever both are open.",
            prompt.name
        ));
    }

    pub fn decline_link(&mut self, id: &str) {
        self.devices.prompts.retain(|prompt| prompt.id != id);
        self.devices.declined.insert(id.to_owned());
    }

    pub fn unlink_device(&mut self, index: usize) {
        if index < self.devices.linked.len() {
            let device = self.devices.linked.remove(index);
            self.devices.declined.insert(device.id);
            self.save_devices();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_linked_devices_sync_and_link_needs_the_same_lastfm_user() {
        let mut phone = Engine::default();
        let gym = phone.library.playlists.create();
        phone.library.playlists.rename(gym, "Gym");
        let data = serde_json::to_string(&phone.sync_data()).unwrap();

        let mut pc = Engine::default();
        pc.settings.lastfm.username = "deathirs".into();
        // Not linked: ignored.
        pc.receive_sync("phone", &data);
        assert_eq!(pc.library.playlists.iter().count(), 0);
        // Someone else's device asking to link isn't shown.
        pc.link_requested("stranger", "Their PC", "someone");
        assert!(pc.devices.prompts.is_empty());
        // The user's phone is, and once linked it syncs.
        pc.link_requested("phone", "Galaxy A56", "Deathirs");
        assert_eq!(pc.devices.prompts.len(), 1);
        pc.accept_link("phone");
        assert!(pc.devices.is_linked("phone"));
        pc.receive_sync("phone", &data);
        let names: Vec<_> = pc
            .library
            .playlists
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert_eq!(names, ["Gym"]);
        assert!(pc.library.playlists_changed);
        pc.unlink_device(0);
        assert!(pc.devices.linked.is_empty());
    }
}
