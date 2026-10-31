//! What Home shows: rows planned from the user's taste, filled from a few
//! YouTube searches and Mixes, and saved so Home opens instantly.

use serde::{Deserialize, Serialize};

use crate::domain::{youtube_mix_url, Track};

use super::taste::Taste;

/// Home is rebuilt in the background once it is older than this.
pub(crate) const HOME_MAX_AGE_SECS: u64 = 6 * 60 * 60;
/// Songs per row.
pub(crate) const ROW_LEN: usize = 12;
/// Rows shorter than this are not worth showing.
const MIN_ROW_LEN: usize = 3;
/// Songs outside this length are usually compilations or clips.
const SONG_SECS: std::ops::RangeInclusive<f64> = 60.0..=900.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct HomeRow {
    pub(crate) title: String,
    pub(crate) tracks: Vec<Track>,
}

/// The rows built from searches, saved between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct HomeCache {
    /// Unix seconds.
    pub(crate) built_at: u64,
    pub(crate) rows: Vec<HomeRow>,
}

impl HomeCache {
    pub(crate) fn is_stale(&self, now: u64) -> bool {
        self.rows.is_empty() || now.saturating_sub(self.built_at) >= HOME_MAX_AGE_SECS
    }
}

/// One row to build: its title, where to look (best first), and whether it
/// should only hold songs and artists the user hasn't heard.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowPlan {
    pub(crate) title: String,
    pub(crate) sources: Vec<String>,
    pub(crate) discover: bool,
}

/// Plan Home from listening history, falling back to the genres picked on
/// first run. Favorite artists and genres come first; Discover looks for
/// new music in the user's top genre.
pub(crate) fn plan_home(taste: &Taste, seeds: &[String], now: u64) -> Vec<RowPlan> {
    let mut rows = Vec::new();
    let artists = taste.top_artists(now, 2);
    for (artist, _) in &artists {
        let mut sources = Vec::new();
        if let Some(song) = taste.favorite_song_by(artist, now) {
            sources.push(youtube_mix_url(&song));
        }
        sources.push(format!("ytsearch15:{artist} songs"));
        rows.push(RowPlan {
            title: format!("Because you like {artist}"),
            sources,
            discover: false,
        });
    }
    let genres = taste.top_genres(now, seeds, 3);
    let genre_rows = if artists.is_empty() { 3 } else { 2 };
    for (genre, _) in genres.iter().take(genre_rows) {
        rows.push(RowPlan {
            title: format!("Because you like {}", display_genre(genre)),
            sources: vec![format!("ytsearch15:{genre} music")],
            discover: false,
        });
    }
    if let Some((genre, _)) = genres.first() {
        let mut sources = vec![format!("ytsearch20:new {genre} songs")];
        if let Some((artist, _)) = artists.first() {
            if let Some(song) = taste.favorite_song_by(artist, now) {
                sources.push(youtube_mix_url(&song));
            }
        }
        rows.push(RowPlan {
            title: "Discover".into(),
            sources,
            discover: true,
        });
    }
    if rows.is_empty() {
        // Nothing known yet: something to start from.
        rows.push(RowPlan {
            title: "Popular right now".into(),
            sources: vec!["ytsearch20:top hits this week official audio".into()],
            discover: false,
        });
    }
    rows
}

/// Turn a source's songs into a row: no repeats, no songs the user keeps
/// skipping, no compilations, ranked toward the user's taste. Discover keeps
/// only songs and artists the user hasn't played.
pub(crate) fn finish_row(
    plan: &RowPlan,
    candidates: Vec<Track>,
    taste: &Taste,
    now: u64,
) -> Option<HomeRow> {
    let preferences = taste.preferences(now);
    let mut seen = std::collections::HashSet::new();
    let candidates: Vec<Track> = candidates
        .into_iter()
        .filter(|track| seen.insert(track.id.clone()))
        .filter(|track| track.duration.is_none_or(|secs| SONG_SECS.contains(&secs)))
        .filter(|track| {
            !plan.discover || (!taste.has_played(&track.id) && !preferences.knows_artist(track))
        })
        .collect();
    let tracks: Vec<Track> = preferences
        .rank(candidates)
        .into_iter()
        .take(ROW_LEN)
        .collect();
    (tracks.len() >= MIN_ROW_LEN).then(|| HomeRow {
        title: plan.title.clone(),
        tracks,
    })
}

/// "k-pop" → "K-pop", "hardstyle" → "Hardstyle".
pub(crate) fn display_genre(genre: &str) -> String {
    let mut chars = genre.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Genres offered on first run, mainstream and niche, grouped loosely.
pub(crate) const STARTER_GENRES: &[&str] = &[
    "pop",
    "rock",
    "hip-hop",
    "rap",
    "trap",
    "r&b",
    "indie",
    "alternative",
    "metal",
    "punk",
    "emo",
    "electronic",
    "edm",
    "house",
    "techno",
    "trance",
    "drum and bass",
    "dubstep",
    "hardstyle",
    "rawstyle",
    "hardcore",
    "frenchcore",
    "hardtekk",
    "speedcore",
    "gabber",
    "phonk",
    "synthwave",
    "lo-fi",
    "ambient",
    "k-pop",
    "j-pop",
    "anime",
    "russian pop",
    "russian rap",
    "russian rock",
    "reggaeton",
    "latin",
    "funk",
    "brazilian funk",
    "sertanejo",
    "mpb",
    "pagode",
    "jazz",
    "blues",
    "soul",
    "classical",
    "country",
    "reggae",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recommendations::taste::ListenEnd;

    const NOW: u64 = 10_000_000;

    fn track(id: &str, artist: &str) -> Track {
        Track {
            title: format!("{artist} - Song {id}"),
            id: id.into(),
            duration: Some(200.0),
            channel: artist.into(),
        }
    }

    #[test]
    fn first_run_picks_plan_genre_rows_and_discover() {
        let plans = plan_home(
            &Taste::default(),
            &["hardstyle".into(), "russian rap".into()],
            NOW,
        );
        let titles: Vec<_> = plans.iter().map(|p| p.title.as_str()).collect();
        assert!(titles.contains(&"Because you like Hardstyle"));
        assert!(titles.contains(&"Because you like Russian rap"));
        assert_eq!(titles.last(), Some(&"Discover"));
        assert!(plans.last().unwrap().discover);
        let fresh = plan_home(&Taste::default(), &[], NOW);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].title, "Popular right now");
    }

    #[test]
    fn favorite_artists_get_rows_seeded_by_their_best_song() {
        let mut taste = Taste::default();
        taste.record(&track("q1", "Queen"), 900.0, ListenEnd::Finished, NOW);
        let plans = plan_home(&taste, &[], NOW);
        assert_eq!(plans[0].title, "Because you like Queen");
        assert!(plans[0].sources[0].contains("list=RDq1"));
    }

    #[test]
    fn rows_drop_repeats_compilations_and_skipped_songs() {
        let mut taste = Taste::default();
        let skipped = track("skip", "X");
        taste.record(&skipped, 1.0, ListenEnd::Skipped, NOW);
        taste.record(&skipped, 1.0, ListenEnd::Skipped, NOW);
        let mut long = track("long", "Y");
        long.duration = Some(3_600.0);
        let plan = RowPlan {
            title: "Row".into(),
            sources: vec![],
            discover: false,
        };
        let row = finish_row(
            &plan,
            vec![
                track("a", "A"),
                track("a", "A"),
                skipped,
                long,
                track("b", "B"),
                track("c", "C"),
            ],
            &taste,
            NOW,
        )
        .unwrap();
        let ids: Vec<_> = row.tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        // Too few songs left: no row.
        assert!(finish_row(&plan, vec![track("z", "Z")], &taste, NOW).is_none());
    }

    #[test]
    fn discover_only_keeps_new_songs_from_new_artists() {
        let mut taste = Taste::default();
        taste.record(&track("known", "Known"), 600.0, ListenEnd::Finished, NOW);
        let plan = RowPlan {
            title: "Discover".into(),
            sources: vec![],
            discover: true,
        };
        let row = finish_row(
            &plan,
            vec![
                track("known", "Known"),
                track("same-artist", "Known"),
                track("n1", "New One"),
                track("n2", "New Two"),
                track("n3", "New Three"),
            ],
            &taste,
            NOW,
        )
        .unwrap();
        assert!(row.tracks.iter().all(|t| t.id.starts_with('n')));
    }

    #[test]
    fn home_goes_stale_after_six_hours() {
        let cache = HomeCache {
            built_at: NOW,
            rows: vec![HomeRow {
                title: "x".into(),
                tracks: vec![],
            }],
        };
        assert!(!cache.is_stale(NOW + 60));
        assert!(cache.is_stale(NOW + HOME_MAX_AGE_SECS));
        assert!(HomeCache::default().is_stale(NOW));
    }

    #[test]
    fn genres_display_nicely_and_the_starter_list_has_the_niche_ones() {
        assert_eq!(display_genre("k-pop"), "K-pop");
        assert_eq!(display_genre(""), "");
        for genre in [
            "hardstyle",
            "hardcore",
            "hardtekk",
            "frenchcore",
            "russian rap",
        ] {
            assert!(STARTER_GENRES.contains(&genre));
        }
    }
}
