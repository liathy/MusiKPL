//! Song lyrics, synced line by line when available, from LRCLIB
//! (lrclib.net: free, open, no key). Found lyrics, and songs without any,
//! are remembered in the data folder so each song is looked up once.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{domain::SongInfo, storage};

const LRCLIB: &str = "https://lrclib.net/api";
/// Lyrics whose length differs more than this from the song's are for
/// another version.
const DURATION_SLACK: f64 = 8.0;

/// One line, with when it starts (seconds).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub at: f64,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    /// Timed lines, in order; empty when only plain lyrics exist.
    pub synced: Vec<Line>,
    /// Lines without times.
    pub plain: Vec<String>,
    pub instrumental: bool,
}

impl Lyrics {
    pub fn is_synced(&self) -> bool {
        !self.synced.is_empty()
    }

    /// The line being sung at `position` seconds (`None` before the first).
    pub fn line_at(&self, position: f64) -> Option<usize> {
        let after = self.synced.partition_point(|line| line.at <= position);
        after.checked_sub(1)
    }
}

/// Read LRC text ("[01:02.50]words") into timed lines, in order. Lines with
/// several times appear once per time; tags like [ar:...] are skipped.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut offset = 0.0;
    let mut lines = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(stripped) = rest.strip_prefix('[') {
            let Some(end) = stripped.find(']') else { break };
            let tag = &stripped[..end];
            if let Some(value) = tag.strip_prefix("offset:") {
                // Milliseconds; positive shows lines earlier.
                offset = value.trim().parse::<f64>().unwrap_or(0.0) / 1000.0;
            } else if let Some(at) = parse_time(tag) {
                times.push(at);
            }
            rest = stripped[end + 1..].trim_start();
        }
        for at in times {
            lines.push(Line {
                at: (at - offset).max(0.0),
                text: rest.trim().to_owned(),
            });
        }
    }
    lines.sort_by(|a, b| a.at.total_cmp(&b.at));
    lines
}

/// "mm:ss", "mm:ss.xx" or "mm:ss:xx" into seconds.
fn parse_time(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes: f64 = minutes.trim().parse().ok()?;
    let seconds: f64 = seconds.trim().replacen(':', ".", 1).parse().ok()?;
    (minutes >= 0.0 && (0.0..60.0).contains(&seconds)).then_some(minutes * 60.0 + seconds)
}

fn from_record(record: &Value) -> Option<Lyrics> {
    let instrumental = record.get("instrumental").and_then(Value::as_bool) == Some(true);
    let synced = record
        .get("syncedLyrics")
        .and_then(Value::as_str)
        .map(parse_lrc)
        .unwrap_or_default();
    let plain: Vec<String> = record
        .get("plainLyrics")
        .and_then(Value::as_str)
        .map(|text| text.lines().map(|line| line.trim().to_owned()).collect())
        .unwrap_or_default();
    (instrumental || !synced.is_empty() || !plain.is_empty()).then_some(Lyrics {
        synced,
        plain,
        instrumental,
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .user_agent(concat!(
            "MusiKPL/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/liathy/MusiKPL)"
        ))
        .build()
}

fn call(path: &str, query: &[(&str, &str)]) -> Result<Option<Value>, String> {
    let mut request = agent().get(&format!("{LRCLIB}{path}"));
    for (name, value) in query {
        request = request.query(name, value);
    }
    match request.call() {
        Ok(response) => response
            .into_string()
            .ok()
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
            .map(Some)
            .ok_or_else(|| "The lyrics service sent an unreadable reply".to_owned()),
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(error) => Err(format!("Couldn't reach the lyrics service ({error})")),
    }
}

/// Of search results, the best: closest in length, synced preferred.
fn pick(results: &[Value], duration: Option<f64>) -> Option<&Value> {
    results
        .iter()
        .filter(
            |record| match (duration, record.get("duration").and_then(Value::as_f64)) {
                (Some(wanted), Some(length)) => (wanted - length).abs() <= DURATION_SLACK,
                _ => true,
            },
        )
        .min_by_key(|record| {
            let unsynced = record.get("syncedLyrics").and_then(Value::as_str).is_none();
            let off = match (duration, record.get("duration").and_then(Value::as_f64)) {
                (Some(wanted), Some(length)) => (wanted - length).abs() as u64,
                _ => 0,
            };
            (unsynced, off)
        })
}

/// Look the song up. `Ok(None)`: it has no lyrics there.
pub fn fetch(song: &SongInfo, duration: Option<f64>) -> Result<Option<Lyrics>, String> {
    if song.title.trim().is_empty() {
        return Ok(None);
    }
    // The exact song first.
    if let Some(duration) = duration {
        let seconds = format!("{}", duration.round() as u64);
        let exact = call(
            "/get",
            &[
                ("artist_name", &song.artist),
                ("track_name", &song.title),
                ("duration", &seconds),
            ],
        )?;
        if let Some(lyrics) = exact.as_ref().and_then(from_record) {
            if lyrics.is_synced() || lyrics.instrumental {
                return Ok(Some(lyrics));
            }
        }
    }
    // Then a search, by artist and title, then by words.
    let mut best_plain = None;
    let words = format!("{} {}", song.artist, song.title);
    let searches: [&[(&str, &str)]; 2] = [
        &[("artist_name", &song.artist), ("track_name", &song.title)],
        &[("q", &words)],
    ];
    for query in searches {
        let Some(Value::Array(results)) = call("/search", query)? else {
            continue;
        };
        if let Some(lyrics) = pick(&results, duration).and_then(from_record) {
            if lyrics.is_synced() {
                return Ok(Some(lyrics));
            }
            best_plain.get_or_insert(lyrics);
        }
    }
    Ok(best_plain)
}

fn cache_name(id: &str) -> Option<String> {
    let safe = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    safe.then(|| format!("lyrics/{id}.json"))
}

/// What's remembered for a song: `Some(None)` means it has none.
pub fn cached(id: &str) -> Option<Option<Lyrics>> {
    let name = cache_name(id)?;
    let path = storage::data_dir()?.join(name);
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn remember(id: &str, lyrics: &Option<Lyrics>) {
    let (Some(name), Some(dir)) = (cache_name(id), storage::data_dir()) else {
        return;
    };
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string(lyrics) {
        let _ = std::fs::write(path, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_lines_are_timed_sorted_and_tags_skipped() {
        let lines = parse_lrc(
            "[ar:Queen]\n[offset:500]\n[00:12.00]Is this the real life?\n[00:17.50][01:00.00]Is this just fantasy?\n[00:20.1]\n",
        );
        let times: Vec<f64> = lines.iter().map(|line| line.at).collect();
        assert_eq!(times, [11.5, 17.0, 19.6, 59.5]);
        assert_eq!(lines[1].text, "Is this just fantasy?");
        assert_eq!(lines[2].text, "");
    }

    #[test]
    fn the_line_being_sung_follows_the_position() {
        let lyrics = Lyrics {
            synced: parse_lrc("[00:05.00]a\n[00:10.00]b\n[00:15.00]c"),
            ..Lyrics::default()
        };
        assert_eq!(lyrics.line_at(1.0), None);
        assert_eq!(lyrics.line_at(5.0), Some(0));
        assert_eq!(lyrics.line_at(12.0), Some(1));
        assert_eq!(lyrics.line_at(500.0), Some(2));
    }

    #[test]
    fn search_results_pick_the_closest_synced_version() {
        let results: Vec<Value> = serde_json::from_str(
            r#"[
                {"duration": 200, "plainLyrics": "far"},
                {"duration": 356, "plainLyrics": "plain"},
                {"duration": 360, "syncedLyrics": "[00:01.00]synced"}
            ]"#,
        )
        .unwrap();
        let best = pick(&results, Some(355.0)).and_then(from_record).unwrap();
        assert!(best.is_synced());
    }

    #[test]
    #[ignore = "reaches lrclib.net"]
    fn finds_real_lyrics() {
        let song = SongInfo {
            artist: "Queen".into(),
            title: "Bohemian Rhapsody".into(),
        };
        let lyrics = fetch(&song, Some(355.0)).unwrap().unwrap();
        assert!(lyrics.is_synced());
    }
}
