//! What Home shows: rows planned from the user's taste, filled from a few
//! YouTube searches and Mixes, and saved so Home opens instantly.
//!
//! Home pages in both directions: each row keeps more songs than it shows
//! and knows further searches for more, and more rows can be planned from
//! favorites that don't have one yet.

use serde::{Deserialize, Serialize};

use std::collections::HashSet;

use crate::domain::{youtube_mix_url, Track};

use super::{rng::Rng, taste::Taste};

/// Home is rebuilt in the background once it is older than this.
pub const HOME_MAX_AGE_SECS: u64 = 6 * 60 * 60;
/// Songs shown per page of a row.
pub const ROW_LEN: usize = 12;
/// Songs a row keeps ready to page through before searching again.
const ROW_POOL: usize = ROW_LEN * 4;
/// Rows shorter than this are not worth showing.
const MIN_ROW_LEN: usize = 3;
/// Songs outside this length are usually compilations or clips.
const SONG_SECS: std::ops::RangeInclusive<f64> = 60.0..=900.0;
/// Rows added each time the user reaches the bottom of Home.
pub const MORE_ROWS: usize = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HomeRow {
    pub title: String,
    /// Every song found so far; the screen shows them a page at a time.
    pub tracks: Vec<Track>,
    /// Only songs and artists the user hasn't heard.
    #[serde(default)]
    pub discover: bool,
    /// Searches still unused, for when the user pages past `tracks`.
    #[serde(default)]
    pub next: Vec<String>,
}

/// The rows built from searches, saved between runs.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HomeCache {
    /// Unix seconds.
    pub built_at: u64,
    pub rows: Vec<HomeRow>,
}

impl HomeCache {
    pub fn is_stale(&self, now: u64) -> bool {
        self.rows.is_empty() || now.saturating_sub(self.built_at) >= HOME_MAX_AGE_SECS
    }
}

/// One row to build: its title, where to look (best first), further
/// searches for paging, and whether it should only hold songs and artists
/// the user hasn't heard.
#[derive(Clone, Debug, PartialEq)]
pub struct RowPlan {
    pub title: String,
    pub sources: Vec<String>,
    pub more: Vec<String>,
    pub discover: bool,
}

/// How many favorites the first rows draw from, so every refresh can pick
/// different ones while staying close to what the user likes most.
const ARTIST_POOL: usize = 6;
const GENRE_POOL: usize = 6;
/// How deep "load more rows" goes into the favorites.
const MORE_POOL: usize = 25;

/// Ways to ask YouTube for an artist's or a genre's songs. One is used to
/// build the row (picked at random, so results vary); the rest page it.
const ARTIST_QUERIES: &[&str] = &[
    "{} songs",
    "{} best songs",
    "{} popular songs",
    "{} new songs",
    "{} live",
];
const GENRE_QUERIES: &[&str] = &[
    "{} music",
    "best {} songs",
    "{} mix",
    "{} hits",
    "{} playlist",
    "new {} songs",
];

/// All the searches for `name`, shuffled: the first builds the row.
fn searches(rng: &mut Rng, patterns: &[&str], name: &str) -> Vec<String> {
    let mut all: Vec<String> = patterns
        .iter()
        .map(|pattern| format!("ytsearch30:{}", pattern.replace("{}", name)))
        .collect();
    rng.shuffle(&mut all);
    all
}

fn artist_row(taste: &Taste, artist: &str, now: u64, rng: &mut Rng) -> RowPlan {
    let mut queries = searches(rng, ARTIST_QUERIES, artist);
    // Mixes of the user's best-loved songs by the artist, one at random first.
    let mut mixes: Vec<String> = taste
        .favorite_songs_by(artist, now, 3)
        .iter()
        .map(youtube_mix_url)
        .collect();
    rng.shuffle(&mut mixes);
    let mut sources = Vec::new();
    if !mixes.is_empty() {
        sources.push(mixes.remove(0));
    }
    sources.push(queries.remove(0));
    queries.extend(mixes);
    RowPlan {
        title: format!("Because you like {artist}"),
        sources,
        more: queries,
        discover: false,
    }
}

fn genre_row(genre: &str, rng: &mut Rng) -> RowPlan {
    let mut queries = searches(rng, GENRE_QUERIES, genre);
    let first = queries.remove(0);
    RowPlan {
        title: format!("Because you like {}", display_genre(genre)),
        sources: vec![first],
        more: queries,
        discover: false,
    }
}

/// Plan Home from listening history, falling back to the genres picked on
/// first run. Each call draws from the user's favorites at random, leaning
/// toward the most played, so Home changes every time it is rebuilt.
/// An artist the user bookmarked, with Last.fm's similar artists.
#[derive(Clone, Debug, PartialEq)]
pub struct Followed {
    pub name: String,
    pub similar: Vec<String>,
}

/// "From {artist}": songs by a bookmarked artist.
fn followed_row(taste: &Taste, followed: &Followed, now: u64, rng: &mut Rng) -> RowPlan {
    RowPlan {
        title: format!("From {}", followed.name),
        ..artist_row(taste, &followed.name, now, rng)
    }
}

/// "Similar to {artist}": songs by artists Last.fm finds similar to a
/// bookmarked one, two at a time, the rest kept for paging.
fn similar_row(followed: &Followed, rng: &mut Rng) -> Option<RowPlan> {
    let mut similar = followed.similar.clone();
    if similar.is_empty() {
        return None;
    }
    rng.shuffle(&mut similar);
    let searches: Vec<String> = similar
        .iter()
        .map(|artist| format!("ytsearch15:{artist} songs"))
        .collect();
    let split = searches.len().min(2);
    Some(RowPlan {
        title: format!("Similar to {}", followed.name),
        sources: searches[..split].to_vec(),
        more: searches[split..].to_vec(),
        discover: false,
    })
}

pub fn plan_home(
    taste: &Taste,
    seeds: &[String],
    followed: &[Followed],
    now: u64,
    rng: &mut Rng,
) -> Vec<RowPlan> {
    let mut rows = Vec::new();
    // Bookmarked artists come first: one of them, and artists like another.
    if let Some(artist) = rng.pick(followed) {
        rows.push(followed_row(taste, artist, now, rng));
    }
    let with_similar: Vec<&Followed> = followed.iter().filter(|f| !f.similar.is_empty()).collect();
    if let Some(artist) = rng.pick(&with_similar) {
        rows.extend(similar_row(artist, rng));
    }
    let artists = rng.weighted_sample(&taste.top_artists(now, ARTIST_POOL), 2);
    for (artist, _) in &artists {
        rows.push(artist_row(taste, artist, now, rng));
    }
    let genre_rows = if artists.is_empty() { 3 } else { 2 };
    let genres = rng.weighted_sample(&taste.top_genres(now, seeds, GENRE_POOL), genre_rows + 1);
    for (genre, _) in genres.iter().take(genre_rows) {
        rows.push(genre_row(genre, rng));
    }
    // Discover looks in a genre the user likes, often one without its own row.
    if let Some((genre, _)) = genres.last() {
        let mut sources = vec![format!("ytsearch30:new {genre} songs")];
        if let Some((artist, _)) = artists.first() {
            let songs = taste.favorite_songs_by(artist, now, 3);
            if let Some(song) = rng.pick(&songs) {
                sources.push(youtube_mix_url(song));
            }
        }
        let mut more: Vec<String> = genres
            .iter()
            .map(|(genre, _)| format!("ytsearch30:new {genre} artists"))
            .collect();
        more.push(format!("ytsearch30:underground {genre}"));
        rows.push(RowPlan {
            title: "Discover".into(),
            sources,
            more,
            discover: true,
        });
    }
    if rows.is_empty() {
        // Nothing known yet: something to start from.
        rows.push(RowPlan {
            title: "Popular right now".into(),
            sources: vec!["ytsearch30:top hits this week official audio".into()],
            more: vec![
                "ytsearch30:viral songs this month".into(),
                "ytsearch30:trending music videos".into(),
                "ytsearch30:new music friday".into(),
            ],
            discover: false,
        });
    }
    rows
}

/// More rows for the bottom of Home: favorite artists and genres that
/// don't have a row yet (`shown` holds the current titles). Empty when
/// there are none left.
pub fn plan_more_rows(
    taste: &Taste,
    seeds: &[String],
    followed: &[Followed],
    now: u64,
    shown: &HashSet<String>,
    rng: &mut Rng,
) -> Vec<RowPlan> {
    let mut candidates: Vec<RowPlan> = Vec::new();
    for artist in followed {
        candidates.push(followed_row(taste, artist, now, rng));
        candidates.extend(similar_row(artist, rng));
    }
    for (artist, _) in rng.weighted_sample(&taste.top_artists(now, MORE_POOL), MORE_POOL) {
        candidates.push(artist_row(taste, &artist, now, rng));
    }
    for (genre, _) in rng.weighted_sample(&taste.top_genres(now, seeds, MORE_POOL), MORE_POOL) {
        candidates.push(genre_row(&genre, rng));
    }
    candidates.retain(|plan| !shown.contains(&plan.title));
    rng.shuffle(&mut candidates);
    candidates.truncate(MORE_ROWS);
    candidates
}

/// The usable songs from a search: no repeats or songs in `exclude`, no
/// compilations, nothing the user keeps skipping, only new songs and
/// artists for Discover; ranked toward the user's taste.
pub fn usable_songs(
    candidates: Vec<Track>,
    discover: bool,
    taste: &Taste,
    now: u64,
    exclude: &HashSet<String>,
) -> Vec<Track> {
    let preferences = taste.preferences(now);
    let mut seen = HashSet::new();
    let candidates: Vec<Track> = candidates
        .into_iter()
        .filter(|track| !exclude.contains(&track.id) && seen.insert(track.id.clone()))
        .filter(|track| track.duration.is_none_or(|secs| SONG_SECS.contains(&secs)))
        .filter(|track| {
            !discover || (!taste.has_played(&track.id) && !preferences.knows_artist(track))
        })
        .collect();
    preferences.rank(candidates)
}

/// Turn a plan's songs into a row. Songs in `avoid` (the previous Home)
/// are left out when there are enough others, and the songs are shuffled,
/// so each rebuild looks different.
pub fn finish_row(
    plan: &RowPlan,
    candidates: Vec<Track>,
    taste: &Taste,
    now: u64,
    avoid: &HashSet<String>,
    rng: &mut Rng,
) -> Option<HomeRow> {
    let mut ranked = usable_songs(candidates, plan.discover, taste, now, &HashSet::new());
    let fresh = ranked
        .iter()
        .filter(|track| !avoid.contains(&track.id))
        .count();
    if fresh >= ROW_LEN {
        ranked.retain(|track| !avoid.contains(&track.id));
    }
    ranked.truncate(ROW_POOL);
    rng.shuffle(&mut ranked);
    (ranked.len() >= MIN_ROW_LEN).then(|| HomeRow {
        title: plan.title.clone(),
        tracks: ranked,
        discover: plan.discover,
        next: plan.more.clone(),
    })
}
/// "k-pop" → "K-pop", "hardstyle" → "Hardstyle".
pub fn display_genre(genre: &str) -> String {
    let mut chars = genre.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Genres offered on first run, mainstream and niche, grouped loosely.
pub const STARTER_GENRES: &[&str] = &[
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
            &[],
            NOW,
            &mut Rng::new(1),
        );
        let titles: Vec<_> = plans.iter().map(|p| p.title.as_str()).collect();
        assert!(titles.contains(&"Because you like Hardstyle"));
        assert!(titles.contains(&"Because you like Russian rap"));
        assert_eq!(titles.last(), Some(&"Discover"));
        assert!(plans.last().unwrap().discover);
        let fresh = plan_home(&Taste::default(), &[], &[], NOW, &mut Rng::new(1));
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].title, "Popular right now");
    }

    #[test]
    fn favorite_artists_get_rows_seeded_by_their_best_song() {
        let mut taste = Taste::default();
        taste.record(&track("q1", "Queen"), 900.0, ListenEnd::Finished, NOW);
        let plans = plan_home(&taste, &[], &[], NOW, &mut Rng::new(1));
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
            more: vec![],
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
            &HashSet::new(),
            &mut Rng::new(1),
        )
        .unwrap();
        let mut ids: Vec<_> = row.tracks.iter().map(|t| t.id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["a", "b", "c"]);
        // Too few songs left: no row.
        assert!(finish_row(
            &plan,
            vec![track("z", "Z")],
            &taste,
            NOW,
            &HashSet::new(),
            &mut Rng::new(1)
        )
        .is_none());
    }

    #[test]
    fn discover_only_keeps_new_songs_from_new_artists() {
        let mut taste = Taste::default();
        taste.record(&track("known", "Known"), 600.0, ListenEnd::Finished, NOW);
        let plan = RowPlan {
            title: "Discover".into(),
            sources: vec![],
            more: vec![],
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
            &HashSet::new(),
            &mut Rng::new(1),
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
                discover: false,
                next: vec![],
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
