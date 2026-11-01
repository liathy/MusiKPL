//! Read-only Last.fm data for browsing: finding artists, an artist's page,
//! and a user's profile and listening stats. Each needs only an API key.

use serde_json::Value;

use super::api::{get, ApiError, Params};

/// An artist found by a search.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArtistHit {
    pub(crate) name: String,
    pub(crate) listeners: u64,
}

/// A song Last.fm found for a search.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TrackHit {
    pub(crate) artist: String,
    pub(crate) title: String,
    pub(crate) listeners: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TopTrack {
    pub(crate) title: String,
    pub(crate) plays: u64,
}

/// Everything an artist's page shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ArtistPage {
    pub(crate) name: String,
    pub(crate) listeners: u64,
    pub(crate) plays: u64,
    /// A short biography, as plain text.
    pub(crate) bio: String,
    pub(crate) tags: Vec<String>,
    pub(crate) similar: Vec<String>,
    pub(crate) top_tracks: Vec<TopTrack>,
    pub(crate) url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProfileTrack {
    pub(crate) artist: String,
    pub(crate) title: String,
    /// Plays (top tracks) or 0.
    pub(crate) plays: u64,
    /// Unix seconds it was played (recent tracks); 0 while playing now.
    pub(crate) when: u64,
    pub(crate) now_playing: bool,
}

/// A Last.fm user's profile and stats for one period.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Profile {
    pub(crate) name: String,
    pub(crate) real_name: String,
    pub(crate) scrobbles: u64,
    pub(crate) artists: u64,
    pub(crate) loved: u64,
    /// Unix seconds.
    pub(crate) registered: u64,
    pub(crate) country: String,
    pub(crate) image: Option<String>,
    pub(crate) url: String,
    pub(crate) top_artists: Vec<ArtistHit>,
    pub(crate) top_tracks: Vec<ProfileTrack>,
    pub(crate) recent: Vec<ProfileTrack>,
}

/// The stretches of time Last.fm keeps top charts for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Period {
    Week,
    #[default]
    Month,
    Year,
    AllTime,
}

impl Period {
    pub(crate) const ALL: [Period; 4] =
        [Period::Week, Period::Month, Period::Year, Period::AllTime];

    fn api(self) -> &'static str {
        match self {
            Period::Week => "7day",
            Period::Month => "1month",
            Period::Year => "12month",
            Period::AllTime => "overall",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Period::Week => "Last 7 days",
            Period::Month => "Last month",
            Period::Year => "Last year",
            Period::AllTime => "All time",
        }
    }
}

fn call(method: &'static str, api_key: &str, mut params: Params) -> Result<Value, String> {
    params.push(("method", method.into()));
    params.push(("api_key", api_key.into()));
    get(params).map_err(ApiError::describe)
}

/// Last.fm sends most numbers as strings.
fn number(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.parse().unwrap_or(0),
        _ => 0,
    }
}

fn text(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// A list that Last.fm sends as an object when it has one entry.
fn list(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(item @ Value::Object(_)) => vec![item],
        _ => Vec::new(),
    }
}

pub(crate) fn search_artists(api_key: &str, query: &str) -> Result<Vec<ArtistHit>, String> {
    let value = call(
        "artist.search",
        api_key,
        vec![("artist", query.into()), ("limit", "8".into())],
    )?;
    Ok(parse_artist_hits(
        value.pointer("/results/artistmatches/artist"),
    ))
}

/// Songs matching a search, most listened first (as Last.fm orders them).
pub(crate) fn search_tracks(api_key: &str, query: &str) -> Result<Vec<TrackHit>, String> {
    let value = call(
        "track.search",
        api_key,
        vec![("track", query.into()), ("limit", "12".into())],
    )?;
    Ok(parse_track_hits(
        value.pointer("/results/trackmatches/track"),
    ))
}

fn parse_track_hits(value: Option<&Value>) -> Vec<TrackHit> {
    list(value)
        .into_iter()
        .map(|track| TrackHit {
            artist: text(track.get("artist")),
            title: text(track.get("name")),
            listeners: number(track.get("listeners")),
        })
        .filter(|track| !track.title.is_empty() && !track.artist.is_empty())
        .collect()
}

fn parse_artist_hits(value: Option<&Value>) -> Vec<ArtistHit> {
    list(value)
        .into_iter()
        .map(|artist| ArtistHit {
            name: text(artist.get("name")),
            listeners: number(artist.get("listeners").or_else(|| artist.get("playcount"))),
        })
        .filter(|artist| !artist.name.is_empty())
        .collect()
}

pub(crate) fn artist_page(api_key: &str, name: &str) -> Result<ArtistPage, String> {
    let info = call(
        "artist.getInfo",
        api_key,
        vec![("artist", name.into()), ("autocorrect", "1".into())],
    )?;
    let mut page = parse_artist_info(&info);
    if page.name.is_empty() {
        page.name = name.to_owned();
    }
    // The page still opens if the top songs can't be read.
    if let Ok(top) = call(
        "artist.getTopTracks",
        api_key,
        vec![
            ("artist", page.name.clone()),
            ("limit", "15".into()),
            ("autocorrect", "1".into()),
        ],
    ) {
        page.top_tracks = parse_top_tracks(top.pointer("/toptracks/track"));
    }
    Ok(page)
}

fn parse_artist_info(value: &Value) -> ArtistPage {
    let artist = value.get("artist");
    let field = |path: &str| artist.and_then(|artist| artist.pointer(path));
    ArtistPage {
        name: text(field("/name")),
        listeners: number(field("/stats/listeners")),
        plays: number(field("/stats/playcount")),
        bio: plain_bio(&text(field("/bio/summary"))),
        tags: list(field("/tags/tag"))
            .into_iter()
            .map(|tag| text(tag.get("name")))
            .filter(|tag| !tag.is_empty())
            .take(6)
            .collect(),
        similar: list(field("/similar/artist"))
            .into_iter()
            .map(|artist| text(artist.get("name")))
            .filter(|name| !name.is_empty())
            .collect(),
        top_tracks: Vec::new(),
        url: text(field("/url")),
    }
}

fn parse_top_tracks(value: Option<&Value>) -> Vec<TopTrack> {
    list(value)
        .into_iter()
        .map(|track| TopTrack {
            title: text(track.get("name")),
            plays: number(track.get("playcount")),
        })
        .filter(|track| !track.title.is_empty())
        .collect()
}

/// Last.fm's summaries end with a "Read more on Last.fm" link; keep the text.
fn plain_bio(summary: &str) -> String {
    let before_link = summary.split("<a ").next().unwrap_or_default();
    let mut text = String::new();
    let mut in_tag = false;
    for c in before_link.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    text.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_owned()
}

/// A Last.fm user whose name is exactly `name`, if there is one. Last.fm
/// can't search people by part of a name.
pub(crate) fn find_user(api_key: &str, name: &str) -> Result<Option<Profile>, String> {
    let name = name.trim();
    // Usernames are 2–15 letters, digits, - or _.
    let plausible = (2..=15).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !plausible {
        return Ok(None);
    }
    match get(vec![
        ("method", "user.getInfo".into()),
        ("api_key", api_key.into()),
        ("user", name.into()),
    ]) {
        Ok(value) => Ok(Some(parse_user_info(&value)).filter(|user| !user.name.is_empty())),
        // "User not found".
        Err(ApiError::Api { code: 6, .. }) => Ok(None),
        Err(error) => Err(error.describe()),
    }
}

pub(crate) fn profile(api_key: &str, user: &str, period: Period) -> Result<Profile, String> {
    let info = call("user.getInfo", api_key, vec![("user", user.into())])?;
    let mut profile = parse_user_info(&info);
    let user_params = |limit: &str| -> Params {
        vec![
            ("user", user.into()),
            ("period", period.api().into()),
            ("limit", limit.into()),
        ]
    };
    // Each section is optional: a failure leaves it empty.
    if let Ok(value) = call("user.getTopArtists", api_key, user_params("12")) {
        profile.top_artists = parse_artist_hits(value.pointer("/topartists/artist"));
    }
    if let Ok(value) = call("user.getTopTracks", api_key, user_params("10")) {
        profile.top_tracks = parse_profile_tracks(value.pointer("/toptracks/track"));
    }
    if let Ok(value) = call(
        "user.getRecentTracks",
        api_key,
        vec![("user", user.into()), ("limit", "10".into())],
    ) {
        profile.recent = parse_profile_tracks(value.pointer("/recenttracks/track"));
    }
    if let Ok(value) = call(
        "user.getLovedTracks",
        api_key,
        vec![("user", user.into()), ("limit", "1".into())],
    ) {
        profile.loved = number(value.pointer("/lovedtracks/@attr/total"));
    }
    Ok(profile)
}

fn parse_user_info(value: &Value) -> Profile {
    let user = value.get("user");
    let field = |path: &str| user.and_then(|user| user.pointer(path));
    // The largest real avatar; Last.fm leaves the URL empty without one.
    let image = list(field("/image"))
        .into_iter()
        .rev()
        .map(|image| text(image.get("#text")))
        .find(|url| url.starts_with("https://"));
    Profile {
        name: text(field("/name")),
        real_name: text(field("/realname")),
        scrobbles: number(field("/playcount")),
        artists: number(field("/artist_count")),
        loved: 0,
        registered: number(field("/registered/unixtime")).max(number(field("/registered/#text"))),
        country: text(field("/country")).replace("None", ""),
        image,
        url: text(field("/url")),
        top_artists: Vec::new(),
        top_tracks: Vec::new(),
        recent: Vec::new(),
    }
}

fn parse_profile_tracks(value: Option<&Value>) -> Vec<ProfileTrack> {
    list(value)
        .into_iter()
        .map(|track| {
            // Top tracks nest the artist's name; recent tracks keep it in #text.
            let artist = track
                .pointer("/artist/name")
                .or_else(|| track.pointer("/artist/#text"));
            ProfileTrack {
                artist: text(artist),
                title: text(track.get("name")),
                plays: number(track.get("playcount")),
                when: number(track.pointer("/date/uts")),
                now_playing: track.pointer("/@attr/nowplaying").and_then(Value::as_str)
                    == Some("true"),
            }
        })
        .filter(|track| !track.title.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_search_results_are_read() {
        let value: Value = serde_json::from_str(
            r#"{"results":{"trackmatches":{"track":[
                {"name":"Spaceman","artist":"Hardwell","listeners":"412000"},
                {"name":"","artist":"Nobody","listeners":"1"}
            ]}}}"#,
        )
        .unwrap();
        let hits = parse_track_hits(value.pointer("/results/trackmatches/track"));
        assert_eq!(
            hits,
            vec![TrackHit {
                artist: "Hardwell".into(),
                title: "Spaceman".into(),
                listeners: 412_000,
            }]
        );
    }

    #[test]
    fn artist_search_results_are_read_and_single_results_too() {
        let value: Value = serde_json::from_str(
            r#"{"results":{"artistmatches":{"artist":[
                {"name":"Headhunterz","listeners":"350000"},
                {"name":"","listeners":"1"}
            ]}}}"#,
        )
        .unwrap();
        let hits = parse_artist_hits(value.pointer("/results/artistmatches/artist"));
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].listeners, 350_000);

        let single: Value =
            serde_json::from_str(r#"{"a":{"name":"Solo","listeners":"5"}}"#).unwrap();
        assert_eq!(parse_artist_hits(single.get("a")).len(), 1);
    }

    #[test]
    fn artist_pages_keep_plain_text_and_their_lists() {
        let value: Value = serde_json::from_str(
            r#"{"artist":{"name":"Queen","url":"https://www.last.fm/music/Queen",
                "stats":{"listeners":"5000000","playcount":"300000000"},
                "tags":{"tag":[{"name":"rock"},{"name":"classic rock"}]},
                "similar":{"artist":[{"name":"David Bowie"}]},
                "bio":{"summary":"Queen were a <b>British</b> band &amp; more. <a href=\"x\">Read more on Last.fm</a>"}}}"#,
        )
        .unwrap();
        let page = parse_artist_info(&value);
        assert_eq!(page.name, "Queen");
        assert_eq!(page.listeners, 5_000_000);
        assert_eq!(page.tags, ["rock", "classic rock"]);
        assert_eq!(page.similar, ["David Bowie"]);
        assert_eq!(page.bio, "Queen were a British band & more.");
    }

    #[test]
    fn profiles_and_their_tracks_are_read() {
        let value: Value = serde_json::from_str(
            r##"{"user":{"name":"liathy","realname":"","playcount":"12345","artist_count":"900",
                "registered":{"unixtime":"1500000000"},"country":"Brazil",
                "image":[{"#text":"https://img/s.png","size":"small"},{"#text":"https://img/l.png","size":"large"}],
                "url":"https://www.last.fm/user/liathy"}}"##,
        )
        .unwrap();
        let profile = parse_user_info(&value);
        assert_eq!(profile.scrobbles, 12_345);
        assert_eq!(profile.registered, 1_500_000_000);
        assert_eq!(profile.image.as_deref(), Some("https://img/l.png"));

        let recent: Value = serde_json::from_str(
            r##"{"t":[
                {"name":"Now","artist":{"#text":"A"},"@attr":{"nowplaying":"true"}},
                {"name":"Before","artist":{"#text":"B"},"date":{"uts":"1700000000"}}
            ]}"##,
        )
        .unwrap();
        let tracks = parse_profile_tracks(recent.get("t"));
        assert!(tracks[0].now_playing);
        assert_eq!(tracks[1].artist, "B");
        assert_eq!(tracks[1].when, 1_700_000_000);

        let top: Value = serde_json::from_str(
            r#"{"t":[{"name":"Song","playcount":"42","artist":{"name":"C"}}]}"#,
        )
        .unwrap();
        let tracks = parse_profile_tracks(top.get("t"));
        assert_eq!((tracks[0].artist.as_str(), tracks[0].plays), ("C", 42));
    }
}
