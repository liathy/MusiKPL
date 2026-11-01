//! The user's playlists: named, ordered lists of songs, optionally pinned to
//! Home as favorites.
//!
//! Playlists sync between the user's devices, so each has a permanent `uid`
//! and the time it last changed, and deleted ones leave a tombstone: when
//! two devices merge, the newer version of each playlist wins.

use serde::{Deserialize, Serialize};

use super::Track;

/// Longest playlist name kept.
pub const MAX_NAME: usize = 60;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: u64,
    pub name: String,
    pub tracks: Vec<Track>,
    /// Shown as a row on Home.
    #[serde(default)]
    pub favorite: bool,
    /// A photo the user chose, as a file name in the covers folder.
    #[serde(default)]
    pub cover: Option<String>,
    /// The same playlist on every device.
    #[serde(default)]
    pub uid: String,
    /// When it last changed, in Unix milliseconds.
    #[serde(default)]
    pub modified: u64,
    /// How its songs fade into each other.
    #[serde(default)]
    pub transition: Transition,
}

/// A playlist's own crossfade, or the one in Settings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transition {
    /// As in Settings.
    #[default]
    Default,
    /// No crossfade for this playlist.
    Off,
    Fade {
        seconds: u8,
        curve: crate::audio::FadeCurve,
    },
}

/// A deleted playlist, remembered so syncing doesn't bring it back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub uid: String,
    /// When it was deleted, in Unix milliseconds.
    pub at: u64,
}

/// Deleted playlists remembered, at most.
const MAX_TOMBSTONES: usize = 500;

/// What a merge with another device did.
#[derive(Debug, Default, PartialEq)]
pub struct Merged {
    /// Playlists taken from the other device: their cover photos (when
    /// they have one) must be written.
    pub taken: Vec<String>,
    /// Cover photos no playlist uses anymore.
    pub dropped_covers: Vec<String>,
    pub changed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlists {
    items: Vec<Playlist>,
    next_id: u64,
    deleted: Vec<Tombstone>,
}

/// Now, in Unix milliseconds.
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_millis() as u64)
}

/// A new random-enough id: the time and a counter, in hex.
fn new_uid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos() as u64);
    let mix = nanos
        ^ (u64::from(std::process::id()) << 32)
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    format!("{nanos:x}{mix:x}")
}

impl Playlists {
    /// Give playlists saved by older versions their permanent ids. True when
    /// something changed (and should be saved).
    pub fn ensure_uids(&mut self) -> bool {
        let mut changed = false;
        for playlist in &mut self.items {
            if playlist.uid.is_empty() {
                playlist.uid = new_uid();
                playlist.modified = now_ms();
                changed = true;
            }
        }
        changed
    }

    pub fn tombstones(&self) -> &[Tombstone] {
        &self.deleted
    }

    fn touch(&mut self, id: u64) {
        if let Some(playlist) = self.get_mut(id) {
            playlist.modified = now_ms();
        }
    }

    /// Merge another device's playlists and deletions: for each playlist
    /// the newer version wins; a deletion newer than the playlist removes it.
    pub fn merge(&mut self, remote: &[Playlist], deleted: &[Tombstone]) -> Merged {
        let mut merged = Merged::default();
        for theirs in remote {
            if theirs.uid.is_empty() {
                continue;
            }
            let gone = self
                .deleted
                .iter()
                .any(|tomb| tomb.uid == theirs.uid && tomb.at >= theirs.modified);
            if gone {
                continue;
            }
            match self.items.iter_mut().find(|mine| mine.uid == theirs.uid) {
                Some(mine) if theirs.modified > mine.modified => {
                    if let Some(old) = mine.cover.take() {
                        if theirs.cover.as_ref() != Some(&old) {
                            merged.dropped_covers.push(old);
                        }
                    }
                    let id = mine.id;
                    *mine = Playlist {
                        id,
                        ..theirs.clone()
                    };
                    merged.taken.push(theirs.uid.clone());
                    merged.changed = true;
                }
                Some(_) => {}
                None => {
                    self.next_id = self
                        .next_id
                        .max(self.items.iter().map(|p| p.id).max().unwrap_or(0))
                        + 1;
                    self.items.push(Playlist {
                        id: self.next_id,
                        ..theirs.clone()
                    });
                    merged.taken.push(theirs.uid.clone());
                    merged.changed = true;
                }
            }
        }
        for tomb in deleted {
            if let Some(index) = self
                .items
                .iter()
                .position(|mine| mine.uid == tomb.uid && mine.modified <= tomb.at)
            {
                let removed = self.items.remove(index);
                merged.dropped_covers.extend(removed.cover);
                merged.changed = true;
            }
            if !self.deleted.iter().any(|known| known.uid == tomb.uid) {
                self.deleted.push(tomb.clone());
            }
        }
        self.trim_tombstones();
        merged
    }

    fn trim_tombstones(&mut self) {
        if self.deleted.len() > MAX_TOMBSTONES {
            let extra = self.deleted.len() - MAX_TOMBSTONES;
            self.deleted.drain(..extra);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Playlist> {
        self.items.iter()
    }

    pub fn get(&self, id: u64) -> Option<&Playlist> {
        self.items.iter().find(|playlist| playlist.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Playlist> {
        self.items.iter_mut().find(|playlist| playlist.id == id)
    }

    /// Create an empty playlist named "Playlist N"; returns its id.
    pub fn create(&mut self) -> u64 {
        let mut number = self.items.len() + 1;
        let name = loop {
            let name = format!("Playlist {number}");
            if !self.items.iter().any(|playlist| playlist.name == name) {
                break name;
            }
            number += 1;
        };
        self.next_id = self
            .next_id
            .max(self.items.iter().map(|p| p.id).max().unwrap_or(0))
            + 1;
        let id = self.next_id;
        self.items.push(Playlist {
            id,
            name,
            tracks: Vec::new(),
            favorite: false,
            cover: None,
            uid: new_uid(),
            modified: now_ms(),
            transition: Transition::Default,
        });
        id
    }

    /// How the playlist's songs fade into each other.
    pub fn set_transition(&mut self, id: u64, transition: Transition) -> bool {
        let Some(playlist) = self.items.iter_mut().find(|p| p.id == id) else {
            return false;
        };
        if playlist.transition == transition {
            return false;
        }
        playlist.transition = transition;
        playlist.modified = now_ms();
        true
    }

    /// Remove a playlist, returning it (so its cover photo can be deleted).
    pub fn delete(&mut self, id: u64) -> Option<Playlist> {
        let index = self.items.iter().position(|playlist| playlist.id == id)?;
        let removed = self.items.remove(index);
        if !removed.uid.is_empty() {
            self.deleted.push(Tombstone {
                uid: removed.uid.clone(),
                at: now_ms(),
            });
            self.trim_tombstones();
        }
        Some(removed)
    }

    /// Set or clear the cover photo; returns the one it replaces.
    pub fn set_cover(&mut self, id: u64, cover: Option<String>) -> Option<String> {
        self.touch(id);
        let playlist = self.get_mut(id)?;
        std::mem::replace(&mut playlist.cover, cover)
    }

    /// Rename; blank names are ignored and long ones shortened.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let name: String = name.trim().chars().take(MAX_NAME).collect();
        match self.get_mut(id) {
            Some(playlist) if !name.is_empty() && playlist.name != name => {
                playlist.name = name;
                playlist.modified = now_ms();
                true
            }
            _ => false,
        }
    }

    pub fn toggle_favorite(&mut self, id: u64) -> bool {
        match self.get_mut(id) {
            Some(playlist) => {
                playlist.favorite = !playlist.favorite;
                playlist.modified = now_ms();
                true
            }
            None => false,
        }
    }

    /// Add a song at the end, once. Returns whether it was added.
    pub fn add(&mut self, id: u64, track: Track) -> bool {
        match self.get_mut(id) {
            Some(playlist) if !playlist.tracks.iter().any(|t| t.id == track.id) => {
                playlist.tracks.push(track);
                playlist.modified = now_ms();
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, id: u64, index: usize) -> bool {
        match self.get_mut(id) {
            Some(playlist) if index < playlist.tracks.len() => {
                playlist.tracks.remove(index);
                playlist.modified = now_ms();
                true
            }
            _ => false,
        }
    }

    /// Move the song at `from` to position `to`.
    pub fn reorder(&mut self, id: u64, from: usize, to: usize) -> bool {
        match self.get_mut(id) {
            Some(playlist) if from < playlist.tracks.len() && to < playlist.tracks.len() => {
                if from != to {
                    let track = playlist.tracks.remove(from);
                    playlist.tracks.insert(to, track);
                    playlist.modified = now_ms();
                }
                from != to
            }
            _ => false,
        }
    }

    pub fn contains(&self, id: u64, track_id: &str) -> bool {
        self.get(id)
            .is_some_and(|playlist| playlist.tracks.iter().any(|t| t.id == track_id))
    }
}

/// Where to add a song: an existing playlist, or a new one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaylistTarget {
    Existing(u64),
    New,
    /// Not a playlist: save the song for offline (or remove it), from the
    /// same menu.
    Download,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(180.0),
            channel: "Artist".into(),
        }
    }

    fn ids(playlists: &Playlists, id: u64) -> Vec<String> {
        playlists
            .get(id)
            .unwrap()
            .tracks
            .iter()
            .map(|t| t.id.clone())
            .collect()
    }

    #[test]
    fn new_playlists_get_unique_names_and_ids() {
        let mut playlists = Playlists::default();
        let a = playlists.create();
        let b = playlists.create();
        assert_ne!(a, b);
        assert_eq!(playlists.get(a).unwrap().name, "Playlist 1");
        assert_eq!(playlists.get(b).unwrap().name, "Playlist 2");
        playlists.delete(a);
        let c = playlists.create();
        assert_ne!(c, b, "ids are never reused while others exist");
        assert_eq!(playlists.get(c).unwrap().name, "Playlist 3");
    }

    #[test]
    fn songs_are_added_once_removed_and_reordered() {
        let mut playlists = Playlists::default();
        let id = playlists.create();
        for song in ["a", "b", "c", "a"] {
            playlists.add(id, track(song));
        }
        assert_eq!(ids(&playlists, id), ["a", "b", "c"]);
        assert!(playlists.reorder(id, 2, 0));
        assert_eq!(ids(&playlists, id), ["c", "a", "b"]);
        assert!(!playlists.reorder(id, 0, 9));
        assert!(playlists.remove(id, 1));
        assert_eq!(ids(&playlists, id), ["c", "b"]);
        assert!(playlists.contains(id, "b") && !playlists.contains(id, "a"));
    }

    #[test]
    fn devices_merge_to_the_newest_version_and_deletions_spread() {
        let mut phone = Playlists::default();
        let gym = phone.create();
        phone.add(gym, track("a"));
        let mut pc = Playlists::default();
        // The PC learns the phone's playlist.
        let merged = pc.merge(&phone.items, phone.tombstones());
        assert!(merged.changed && merged.taken.len() == 1);
        let pc_gym = pc.iter().next().unwrap().id;
        assert_eq!(ids(&pc, pc_gym), ["a"]);
        // An older copy doesn't overwrite a newer edit.
        std::thread::sleep(std::time::Duration::from_millis(2));
        pc.add(pc_gym, track("b"));
        let merged = pc.merge(&phone.items, phone.tombstones());
        assert!(!merged.changed);
        assert_eq!(ids(&pc, pc_gym), ["a", "b"]);
        // The newer one wins on the phone.
        phone.merge(&pc.items, pc.tombstones());
        assert_eq!(ids(&phone, gym), ["a", "b"]);
        // A deletion spreads and isn't undone by the old copy.
        let before = pc.items.clone();
        std::thread::sleep(std::time::Duration::from_millis(2));
        phone.delete(gym);
        let merged = pc.merge(&phone.items, phone.tombstones());
        assert!(merged.changed && pc.iter().next().is_none());
        phone.merge(&before, &[]);
        assert!(
            phone.iter().next().is_none(),
            "the deleted one stays deleted"
        );
    }

    #[test]
    fn older_playlists_get_permanent_ids() {
        let mut playlists: Playlists =
            serde_json::from_str(r#"{"items":[{"id":1,"name":"Old","tracks":[]}],"next_id":1}"#)
                .unwrap();
        assert!(playlists.ensure_uids());
        assert!(!playlists.iter().next().unwrap().uid.is_empty());
        assert!(!playlists.ensure_uids());
    }

    #[test]
    fn renaming_ignores_blank_names_and_favorites_toggle() {
        let mut playlists = Playlists::default();
        let id = playlists.create();
        assert!(!playlists.rename(id, "   "));
        assert!(playlists.rename(id, "  Gym hardstyle "));
        assert_eq!(playlists.get(id).unwrap().name, "Gym hardstyle");
        assert!(playlists.toggle_favorite(id));
        assert!(playlists.get(id).unwrap().favorite);
        assert!(playlists.set_cover(id, Some("a.jpg".into())).is_none());
        assert_eq!(playlists.set_cover(id, None).as_deref(), Some("a.jpg"));
        assert!(playlists.delete(id).is_some() && playlists.get(id).is_none());
    }
}
