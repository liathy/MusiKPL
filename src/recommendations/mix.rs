//! Auto Mix rules: which songs are new, where to look for more, and how
//! long to wait after a refill finds nothing.

use std::{collections::HashSet, time::Duration};

use crate::domain::{song_info, youtube_mix_url, Track};

/// Songs added per refill.
pub(crate) const MIX_BATCH: usize = 8;
/// Refill while fewer songs than this are waiting, so the next one is
/// already queued long before the current one ends.
pub(crate) const MIX_LOW_WATER: usize = 3;

/// Songs that must not be added again: by video, and by song, so another
/// upload of something already heard (a lyric video, a remaster) is skipped.
#[derive(Default)]
pub(crate) struct Exclusions {
    ids: HashSet<String>,
    songs: HashSet<String>,
}

impl Exclusions {
    pub(crate) fn add(&mut self, track: &Track) {
        self.ids.insert(track.id.clone());
        self.songs.insert(song_key(track));
    }

    pub(crate) fn allows(&self, track: &Track) -> bool {
        !self.ids.contains(&track.id) && !self.songs.contains(&song_key(track))
    }
}

/// The song a video is, ignoring how it was uploaded: artist and title,
/// without bracketed notes, letters and digits only.
pub(crate) fn song_key(track: &Track) -> String {
    let song = song_info(track);
    let mut key = String::new();
    let mut depth = 0u32;
    for character in format!("{} {}", song.artist, song.title).chars() {
        match character {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if depth == 0 && c.is_alphanumeric() => key.extend(c.to_lowercase()),
            _ => {}
        }
    }
    key
}

/// Pick up to `max_items` new songs, skipping excluded ones and repeats
/// within `items`. Picked songs are added to `exclusions`.
pub(crate) fn select_mix_items(
    items: impl IntoIterator<Item = Track>,
    exclusions: &mut Exclusions,
    max_items: usize,
) -> Vec<Track> {
    let mut selected = Vec::new();
    for item in items {
        if selected.len() >= max_items {
            break;
        }
        if exclusions.allows(&item) {
            exclusions.add(&item);
            selected.push(item);
        }
    }
    selected
}

/// Where to look for songs like `seed`, best first: YouTube's own Mix for
/// the video, then the artist's songs, for videos YouTube has no Mix for.
pub(crate) fn mix_sources(seed: &Track) -> Vec<String> {
    let mut sources = vec![youtube_mix_url(seed)];
    let artist = song_info(seed).artist;
    if !artist.trim().is_empty() {
        sources.push(format!("ytsearch15:{} songs", artist.trim()));
    }
    sources
}

/// How long to wait before trying again after `failures` refills in a row
/// found nothing: 10 s, 30 s, then every minute.
pub(crate) fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs(match failures {
        0 | 1 => 10,
        2 => 30,
        _ => 60,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str, title: &str) -> Track {
        Track {
            title: title.into(),
            id: id.into(),
            duration: Some(120.0),
            channel: "Test".into(),
        }
    }

    #[test]
    fn selection_skips_excluded_videos_and_repeats() {
        let mut exclusions = Exclusions::default();
        exclusions.add(&track("current", "Current"));
        exclusions.add(&track("played", "Played"));
        let items = vec![
            track("current", "Current"),
            track("played", "Played"),
            track("new", "New"),
            track("new", "New"),
        ];
        let selected = select_mix_items(items, &mut exclusions, 8);
        assert_eq!(
            selected.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["new"]
        );
        // Picked songs are excluded from later refills.
        assert!(!exclusions.allows(&track("new", "New")));
    }

    #[test]
    fn other_uploads_of_the_same_song_are_skipped() {
        let mut exclusions = Exclusions::default();
        exclusions.add(&track("a", "Queen - Bohemian Rhapsody (Official Video)"));
        assert!(!exclusions.allows(&track("b", "Queen – Bohemian Rhapsody [Lyrics]")));
        assert!(!exclusions.allows(&track("c", "Queen - Bohemian Rhapsody (Remastered 2011)")));
        assert!(exclusions.allows(&track("d", "Queen - Killer Queen")));
    }

    #[test]
    fn selection_respects_maximum() {
        let mut exclusions = Exclusions::default();
        let selected = select_mix_items(
            (0..4).map(|index| track(&index.to_string(), &format!("Song {index}"))),
            &mut exclusions,
            2,
        );
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn sources_fall_back_from_the_youtube_mix_to_the_artist() {
        let seed = track("abc", "Queen - Killer Queen");
        assert_eq!(
            mix_sources(&seed),
            [
                "https://www.youtube.com/watch?v=abc&list=RDabc".to_string(),
                "ytsearch15:Queen songs".to_string(),
            ]
        );
    }

    #[test]
    fn retries_back_off() {
        assert!(retry_delay(1) < retry_delay(2));
        assert!(retry_delay(2) < retry_delay(3));
        assert_eq!(retry_delay(10), retry_delay(3));
    }
}
