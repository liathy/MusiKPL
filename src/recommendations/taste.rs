//! The user's music taste, learned locally from what they play.
//!
//! Every song played gets a small record: how long it was listened to, how
//! often it was finished or skipped, whether it is liked, and its genres
//! (Last.fm tags, looked up later). Scores fade with a 45-day half-life, so
//! taste follows what the user listens to now. Only this computer stores it.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::domain::{song_info, Track};

/// Listening shorter than this, without finishing, is a skip.
pub(crate) const SKIP_SECS: f64 = 30.0;
/// Songs remembered; the least-loved are forgotten beyond this.
const MAX_SONGS: usize = 1_500;
const HALF_LIFE_DAYS: f64 = 45.0;
const DAY_SECS: f64 = 86_400.0;
/// Tags kept per song.
const MAX_TAGS: usize = 5;
/// Weight of a genre picked on first run, before any listening.
const SEED_GENRE_WEIGHT: f64 = 3.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct SongStats {
    pub(crate) track: Track,
    #[serde(default)]
    plays: u32,
    #[serde(default)]
    listened_secs: f64,
    #[serde(default)]
    completions: u32,
    #[serde(default)]
    skips: u32,
    /// Unix seconds; `None` when not liked.
    #[serde(default)]
    liked_at: Option<u64>,
    #[serde(default)]
    last_played: u64,
    /// Genres, most relevant first; `None` until looked up.
    #[serde(default)]
    tags: Option<Vec<String>>,
}

impl SongStats {
    fn new(track: &Track) -> Self {
        Self {
            track: track.clone(),
            plays: 0,
            listened_secs: 0.0,
            completions: 0,
            skips: 0,
            liked_at: None,
            last_played: 0,
            tags: None,
        }
    }

    /// How much the user likes this song now: listening, finishing, and
    /// liking count; skipping counts against. Listening fades over time.
    fn score(&self, now: u64) -> f64 {
        let age_days = now.saturating_sub(self.last_played) as f64 / DAY_SECS;
        let fade = 0.5_f64.powf(age_days / HALF_LIFE_DAYS);
        let listening = self.listened_secs / 60.0 + f64::from(self.completions) * 1.5;
        let like = if self.liked_at.is_some() { 8.0 } else { 0.0 };
        (listening - f64::from(self.skips) * 2.0) * fade + like
    }

    fn disliked(&self) -> bool {
        self.liked_at.is_none() && self.skips >= 2 && self.completions == 0
    }
}

/// How one listen ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListenEnd {
    /// Heard to (nearly) the end.
    Finished,
    /// Left within the first 30 seconds.
    Skipped,
    /// Left part-way.
    Stopped,
}

#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Taste {
    songs: HashMap<String, SongStats>,
}

impl Taste {
    pub(crate) fn has_history(&self) -> bool {
        self.songs
            .values()
            .any(|song| song.plays > 0 || song.liked_at.is_some())
    }

    pub(crate) fn record(&mut self, track: &Track, listened: f64, end: ListenEnd, now: u64) {
        let song = self
            .songs
            .entry(track.id.clone())
            .or_insert_with(|| SongStats::new(track));
        song.track = track.clone();
        song.last_played = now;
        song.listened_secs += listened.max(0.0);
        match end {
            ListenEnd::Finished => {
                song.plays += 1;
                song.completions += 1;
            }
            ListenEnd::Skipped => song.skips += 1,
            ListenEnd::Stopped => song.plays += 1,
        }
        self.prune(now);
    }

    /// Like or unlike; returns whether the song is now liked.
    pub(crate) fn toggle_like(&mut self, track: &Track, now: u64) -> bool {
        let song = self
            .songs
            .entry(track.id.clone())
            .or_insert_with(|| SongStats::new(track));
        song.liked_at = match song.liked_at {
            Some(_) => None,
            None => Some(now),
        };
        song.liked_at.is_some()
    }

    pub(crate) fn is_liked(&self, id: &str) -> bool {
        self.songs
            .get(id)
            .is_some_and(|song| song.liked_at.is_some())
    }

    pub(crate) fn has_played(&self, id: &str) -> bool {
        self.songs.get(id).is_some_and(|song| song.plays > 0)
    }

    pub(crate) fn set_tags(&mut self, id: &str, tags: Vec<String>) {
        if let Some(song) = self.songs.get_mut(id) {
            song.tags = Some(tags);
        }
    }

    /// Songs worth knowing the genres of: listened to, never looked up.
    pub(crate) fn needing_tags(&self, limit: usize) -> Vec<Track> {
        self.songs
            .values()
            .filter(|song| song.tags.is_none() && (song.plays > 0 || song.liked_at.is_some()))
            .take(limit)
            .map(|song| song.track.clone())
            .collect()
    }

    /// Favorite artists with their score, best first.
    pub(crate) fn top_artists(&self, now: u64, limit: usize) -> Vec<(String, f64)> {
        let mut artists: HashMap<String, (String, f64)> = HashMap::new();
        for song in self.songs.values() {
            let score = song.score(now);
            if score <= 0.0 {
                continue;
            }
            let artist = song_info(&song.track).artist;
            if artist.is_empty() {
                continue;
            }
            let entry = artists
                .entry(artist.to_lowercase())
                .or_insert_with(|| (artist, 0.0));
            entry.1 += score;
        }
        best(artists.into_values().collect(), limit)
    }

    /// Favorite genres with their weight, best first. First-run picks count
    /// a little, so they matter until real listening outweighs them.
    pub(crate) fn top_genres(
        &self,
        now: u64,
        seeds: &[String],
        limit: usize,
    ) -> Vec<(String, f64)> {
        let mut genres: HashMap<String, f64> = HashMap::new();
        for seed in seeds {
            *genres.entry(seed.to_lowercase()).or_default() += SEED_GENRE_WEIGHT;
        }
        for song in self.songs.values() {
            let score = song.score(now);
            if score <= 0.0 {
                continue;
            }
            for (rank, tag) in song.tags.iter().flatten().enumerate() {
                *genres.entry(tag.clone()).or_default() += score / (rank as f64 + 1.0);
            }
        }
        best(genres.into_iter().collect(), limit)
    }

    /// The user's best-loved song by an artist, to start a mix from.
    pub(crate) fn favorite_song_by(&self, artist: &str, now: u64) -> Option<Track> {
        self.songs
            .values()
            .filter(|song| song_info(&song.track).artist.eq_ignore_ascii_case(artist))
            .max_by(|a, b| a.score(now).total_cmp(&b.score(now)))
            .map(|song| song.track.clone())
    }

    pub(crate) fn recently_played(&self, limit: usize) -> Vec<Track> {
        let mut played: Vec<&SongStats> = self.songs.values().filter(|s| s.plays > 0).collect();
        played.sort_by_key(|s| std::cmp::Reverse(s.last_played));
        played
            .into_iter()
            .take(limit)
            .map(|s| s.track.clone())
            .collect()
    }

    pub(crate) fn liked(&self, limit: usize) -> Vec<Track> {
        let mut liked: Vec<&SongStats> = self
            .songs
            .values()
            .filter(|s| s.liked_at.is_some())
            .collect();
        liked.sort_by_key(|s| std::cmp::Reverse(s.liked_at));
        liked
            .into_iter()
            .take(limit)
            .map(|s| s.track.clone())
            .collect()
    }

    /// What Auto Mix needs to lean toward the user's taste.
    pub(crate) fn preferences(&self, now: u64) -> Preferences {
        let artists = self.top_artists(now, 50);
        let top = artists.first().map_or(1.0, |(_, score)| score.max(1.0));
        Preferences {
            artists: artists
                .into_iter()
                .map(|(artist, score)| (artist.to_lowercase(), score / top))
                .collect(),
            disliked: self
                .songs
                .values()
                .filter(|song| song.disliked())
                .map(|song| song.track.id.clone())
                .collect(),
        }
    }

    /// Forget the least-loved songs beyond the limit; liked songs stay.
    fn prune(&mut self, now: u64) {
        if self.songs.len() <= MAX_SONGS {
            return;
        }
        let mut forgettable: Vec<(String, f64)> = self
            .songs
            .values()
            .filter(|song| song.liked_at.is_none())
            .map(|song| (song.track.id.clone(), song.score(now)))
            .collect();
        forgettable.sort_by(|a, b| a.1.total_cmp(&b.1));
        let excess = self.songs.len() - MAX_SONGS;
        for (id, _) in forgettable.into_iter().take(excess) {
            self.songs.remove(&id);
        }
    }
}

fn best(mut scored: Vec<(String, f64)>, limit: usize) -> Vec<(String, f64)> {
    scored.retain(|(_, score)| *score > 0.0);
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(limit);
    scored
}

/// A snapshot of taste for ranking candidate songs off the UI thread.
#[derive(Clone, Debug, Default)]
pub(crate) struct Preferences {
    /// Lower-case artist name to weight in 0..=1.
    artists: HashMap<String, f64>,
    disliked: HashSet<String>,
}

impl Preferences {
    /// Order candidates by taste: songs the user keeps skipping are dropped,
    /// favorite artists move up, and the original order breaks ties, so the
    /// source's own relevance still counts.
    pub(crate) fn rank(&self, candidates: Vec<Track>) -> Vec<Track> {
        let mut ranked: Vec<(usize, f64, Track)> = candidates
            .into_iter()
            .filter(|track| !self.disliked.contains(&track.id))
            .enumerate()
            .map(|(index, track)| {
                let artist = song_info(&track).artist.to_lowercase();
                let affinity = self.artists.get(&artist).copied().unwrap_or(0.0);
                // A favorite artist is worth moving up about four places.
                let rank = index as f64 - affinity * 4.0;
                (index, rank, track)
            })
            .collect();
        ranked.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        ranked.into_iter().map(|(_, _, track)| track).collect()
    }

    pub(crate) fn knows_artist(&self, track: &Track) -> bool {
        self.artists
            .contains_key(&song_info(track).artist.to_lowercase())
    }
}

/// Last.fm tags that describe a listener rather than the music.
const NOT_GENRES: &[&str] = &[
    "seen live",
    "favorites",
    "favourites",
    "favorite",
    "favourite",
    "favorite songs",
    "love",
    "loved",
    "awesome",
    "beautiful",
    "amazing",
    "best",
    "cool",
    "good",
    "my favorite",
    "albums i own",
    "spotify",
    "youtube",
    "under 2000 listeners",
];

/// Turn Last.fm tags into genres: lower-case, no listener notes or years,
/// most relevant first.
pub(crate) fn genres_from_tags(tags: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut genres: Vec<String> = Vec::new();
    for tag in tags {
        let tag = tag.trim().to_lowercase();
        let is_year = tag.chars().all(|c| c.is_ascii_digit()) && !tag.is_empty();
        if tag.is_empty()
            || tag.len() > 40
            || is_year
            || NOT_GENRES.contains(&tag.as_str())
            || genres.contains(&tag)
        {
            continue;
        }
        genres.push(tag);
        if genres.len() == MAX_TAGS {
            break;
        }
    }
    genres
}

/// Follows playback and reports each finished listen: how long the song
/// was heard and how it ended.
#[derive(Default)]
pub(crate) struct ListenTracker {
    current: Option<Track>,
    listened: f64,
    last_position: f64,
    /// Position and wall-clock time of the previous update.
    last: Option<(f64, f64)>,
}

impl ListenTracker {
    /// Call every frame. `now` is a clock in seconds. Returns the previous
    /// song's listen when the song changes.
    pub(crate) fn update(
        &mut self,
        track: Option<&Track>,
        position: f64,
        playing: bool,
        now: f64,
    ) -> Option<(Track, f64, ListenEnd)> {
        let same = match (&self.current, track) {
            (Some(current), Some(track)) => current.id == track.id,
            (None, None) => true,
            _ => false,
        };
        if !same {
            let finished = self.finish();
            self.current = track.cloned();
            if playing && self.current.is_some() {
                self.last = Some((position, now));
                self.last_position = position;
            }
            return finished;
        }
        if !playing {
            self.last = None;
            return None;
        }
        if let Some((last_position, last_time)) = self.last {
            let advanced = position - last_position;
            // A jump larger than the time that passed is a seek.
            if advanced > 0.0 && advanced <= (now - last_time).max(0.0) * 1.5 + 1.0 {
                self.listened += advanced;
            }
        }
        self.last = Some((position, now));
        self.last_position = position;
        None
    }

    /// End the current listen (for example when the app closes).
    pub(crate) fn finish(&mut self) -> Option<(Track, f64, ListenEnd)> {
        let track = self.current.take()?;
        let listened = std::mem::take(&mut self.listened);
        let last_position = std::mem::take(&mut self.last_position);
        self.last = None;
        let finished = track.duration.is_some_and(|duration| {
            duration > 0.0 && (last_position >= duration - 5.0 || listened >= duration * 0.9)
        });
        let end = if finished {
            ListenEnd::Finished
        } else if listened < SKIP_SECS {
            ListenEnd::Skipped
        } else {
            ListenEnd::Stopped
        };
        // Opening a song and leaving at once tells nothing about taste.
        (listened >= 1.0 || finished).then_some((track, listened, end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn track(id: &str, artist: &str) -> Track {
        Track {
            title: format!("{artist} - Song {id}"),
            id: id.into(),
            duration: Some(200.0),
            channel: artist.into(),
        }
    }

    #[test]
    fn listening_finishing_and_liking_raise_an_artist_skipping_lowers_it() {
        let mut taste = Taste::default();
        let now = 1_000 * DAY;
        taste.record(&track("a1", "Queen"), 200.0, ListenEnd::Finished, now);
        taste.record(&track("a2", "Queen"), 120.0, ListenEnd::Stopped, now);
        taste.record(&track("b1", "Abba"), 10.0, ListenEnd::Skipped, now);
        taste.record(&track("c1", "Muse"), 60.0, ListenEnd::Stopped, now);
        let artists = taste.top_artists(now, 10);
        assert_eq!(artists[0].0, "Queen");
        assert!(!artists.iter().any(|(artist, _)| artist == "Abba"));

        assert!(taste.toggle_like(&track("c1", "Muse"), now));
        assert!(taste.is_liked("c1"));
        assert_eq!(taste.top_artists(now, 10)[0].0, "Muse");
        assert!(!taste.toggle_like(&track("c1", "Muse"), now));
    }

    #[test]
    fn old_listening_fades_and_new_listening_takes_over() {
        let mut taste = Taste::default();
        taste.record(&track("old", "Old Band"), 600.0, ListenEnd::Finished, 0);
        let later = 200 * DAY;
        taste.record(&track("new", "New Band"), 200.0, ListenEnd::Finished, later);
        assert_eq!(taste.top_artists(later, 1)[0].0, "New Band");
    }

    #[test]
    fn songs_skipped_again_and_again_are_disliked_unless_liked() {
        let mut taste = Taste::default();
        let song = track("x", "Someone");
        taste.record(&song, 5.0, ListenEnd::Skipped, 10);
        assert!(!taste.songs["x"].disliked());
        taste.record(&song, 3.0, ListenEnd::Skipped, 20);
        assert!(taste.songs["x"].disliked());
        taste.toggle_like(&song, 30);
        assert!(!taste.songs["x"].disliked());
    }

    #[test]
    fn genres_come_from_tags_and_first_run_picks() {
        let mut taste = Taste::default();
        let now = 100 * DAY;
        assert_eq!(
            taste.top_genres(now, &["Hardstyle".into()], 3)[0].0,
            "hardstyle"
        );
        taste.record(&track("k", "Band"), 900.0, ListenEnd::Finished, now);
        taste.set_tags("k", vec!["k-pop".into(), "pop".into()]);
        let genres = taste.top_genres(now, &["hardstyle".into()], 3);
        assert_eq!(genres[0].0, "k-pop");
        assert!(genres.iter().any(|(genre, _)| genre == "hardstyle"));
        assert!(taste.needing_tags(10).is_empty());
    }

    #[test]
    fn tags_are_cleaned_into_genres() {
        let tags = [
            "Hardstyle",
            "seen live",
            "2011",
            "hardstyle",
            "Russian",
            "EDM",
            "rave",
            "x",
            "y",
        ]
        .map(String::from);
        assert_eq!(
            genres_from_tags(tags),
            ["hardstyle", "russian", "edm", "rave", "x"]
        );
    }

    #[test]
    fn recent_and_liked_lists_are_newest_first() {
        let mut taste = Taste::default();
        taste.record(&track("1", "A"), 100.0, ListenEnd::Stopped, 10);
        taste.record(&track("2", "B"), 100.0, ListenEnd::Stopped, 20);
        taste.toggle_like(&track("1", "A"), 30);
        taste.toggle_like(&track("3", "C"), 40);
        let ids = |tracks: Vec<Track>| tracks.into_iter().map(|t| t.id).collect::<Vec<_>>();
        assert_eq!(ids(taste.recently_played(10)), ["2", "1"]);
        assert_eq!(ids(taste.liked(10)), ["3", "1"]);
        assert!(taste.has_played("1") && !taste.has_played("3"));
    }

    #[test]
    fn auto_mix_ranking_favors_loved_artists_and_drops_skipped_songs() {
        let mut taste = Taste::default();
        let now = 50 * DAY;
        taste.record(&track("fav", "Fav Artist"), 900.0, ListenEnd::Finished, now);
        let skipped = track("meh", "Meh");
        taste.record(&skipped, 2.0, ListenEnd::Skipped, now);
        taste.record(&skipped, 2.0, ListenEnd::Skipped, now);
        let preferences = taste.preferences(now);
        let ranked = preferences.rank(vec![
            track("n1", "Stranger"),
            track("n2", "Stranger"),
            track("meh", "Meh"),
            track("f2", "Fav Artist"),
        ]);
        let ids: Vec<_> = ranked.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["f2", "n1", "n2"]);
        assert!(preferences.knows_artist(&track("x", "fav artist")));
    }

    #[test]
    fn the_tracker_reports_how_each_listen_ended() {
        let mut tracker = ListenTracker::default();
        let song = track("s", "A");
        let mut clock = 0.0;
        assert!(tracker.update(Some(&song), 0.0, true, clock).is_none());
        for second in 1..=12 {
            clock += 1.0;
            tracker.update(Some(&song), f64::from(second), true, clock);
        }
        let other = track("o", "B");
        let (done, listened, end) = tracker.update(Some(&other), 0.0, true, clock).unwrap();
        assert_eq!(done.id, "s");
        assert!((listened - 12.0).abs() < 0.01);
        assert_eq!(end, ListenEnd::Skipped);

        // Heard to the end.
        for second in 1..=198 {
            clock += 1.0;
            tracker.update(Some(&other), f64::from(second), true, clock);
        }
        let (_, _, end) = tracker.finish().unwrap();
        assert_eq!(end, ListenEnd::Finished);

        // Seeking forward isn't listening, and leaving at once isn't a skip.
        tracker.update(Some(&song), 0.0, true, clock);
        tracker.update(Some(&song), 150.0, true, clock + 1.0);
        assert!(tracker.update(None, 0.0, false, clock + 2.0).is_none());
    }

    #[test]
    fn the_library_stays_bounded_but_keeps_liked_songs() {
        let mut taste = Taste::default();
        let liked = track("liked", "L");
        taste.toggle_like(&liked, 0);
        for index in 0..MAX_SONGS + 50 {
            taste.record(
                &track(&index.to_string(), "Band"),
                1.0,
                ListenEnd::Skipped,
                5,
            );
        }
        assert!(taste.songs.len() <= MAX_SONGS);
        assert!(taste.is_liked("liked"));
    }
}
