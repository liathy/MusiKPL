//! The user's playlists: named, ordered lists of songs, optionally pinned to
//! Home as favorites.

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
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Playlists {
    items: Vec<Playlist>,
    next_id: u64,
}

impl Playlists {
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
        });
        id
    }

    /// Remove a playlist, returning it (so its cover photo can be deleted).
    pub fn delete(&mut self, id: u64) -> Option<Playlist> {
        let index = self.items.iter().position(|playlist| playlist.id == id)?;
        Some(self.items.remove(index))
    }

    /// Set or clear the cover photo; returns the one it replaces.
    pub fn set_cover(&mut self, id: u64, cover: Option<String>) -> Option<String> {
        let playlist = self.get_mut(id)?;
        std::mem::replace(&mut playlist.cover, cover)
    }

    /// Rename; blank names are ignored and long ones shortened.
    pub fn rename(&mut self, id: u64, name: &str) -> bool {
        let name: String = name.trim().chars().take(MAX_NAME).collect();
        match self.get_mut(id) {
            Some(playlist) if !name.is_empty() && playlist.name != name => {
                playlist.name = name;
                true
            }
            _ => false,
        }
    }

    pub fn toggle_favorite(&mut self, id: u64) -> bool {
        match self.get_mut(id) {
            Some(playlist) => {
                playlist.favorite = !playlist.favorite;
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
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, id: u64, index: usize) -> bool {
        match self.get_mut(id) {
            Some(playlist) if index < playlist.tracks.len() => {
                playlist.tracks.remove(index);
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
