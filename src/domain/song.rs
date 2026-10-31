//! Artist/title metadata derived from YouTube video metadata.
//!
//! YouTube has no structured "artist" and "track" fields for most videos, so
//! scrobbling and album-art lookups use the conventions music uploads follow:
//! auto-generated "Artist - Topic" channels, and "Artist - Title" video names.

use super::Track;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SongInfo {
    pub(crate) artist: String,
    pub(crate) title: String,
}

/// Bracketed suffixes that describe the upload rather than the song.
const NOISE_WORDS: &[&str] = &[
    "official",
    "video",
    "lyric",
    "lyrics",
    "audio",
    "visualizer",
    "visualiser",
    "hd",
    "hq",
    "4k",
    "mv",
    "m/v",
];

const DASHES: &[&str] = &[" - ", " – ", " — ", " | "];

pub(crate) fn song_info(track: &Track) -> SongInfo {
    let channel = track.channel.trim();
    let title = clean_title(&track.title);

    // "Artist - Topic" channels upload the plain track title.
    if let Some(artist) = channel.strip_suffix(" - Topic") {
        return SongInfo {
            artist: artist.trim().to_owned(),
            title,
        };
    }

    for dash in DASHES {
        if let Some((artist, song)) = title.split_once(dash) {
            let (artist, song) = (artist.trim(), song.trim());
            if !artist.is_empty() && !song.is_empty() {
                return SongInfo {
                    artist: artist.to_owned(),
                    title: song.to_owned(),
                };
            }
        }
    }

    SongInfo {
        artist: clean_channel(channel),
        title,
    }
}

/// Drop "(Official Video)", "[Lyrics]" and similar upload descriptions, but
/// keep meaningful brackets such as "(Remastered 2011)" or "(feat. X)".
fn clean_title(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut rest = title;
    while let Some(start) = rest.find(['(', '[']) {
        let close = if rest.as_bytes()[start] == b'(' {
            ')'
        } else {
            ']'
        };
        let Some(length) = rest[start..].find(close) else {
            break;
        };
        let inner = rest[start + 1..start + length].to_lowercase();
        let noise = inner
            .split(|c: char| !c.is_alphanumeric() && c != '/')
            .any(|word| NOISE_WORDS.contains(&word));
        out.push_str(&rest[..start]);
        if !noise {
            out.push_str(&rest[start..=start + length]);
        }
        rest = &rest[start + length + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn clean_channel(channel: &str) -> String {
    let channel = channel.strip_suffix("VEVO").unwrap_or(channel);
    let channel = channel
        .strip_suffix(" Official")
        .or_else(|| channel.strip_suffix(" - Official"))
        .unwrap_or(channel);
    channel.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str, channel: &str) -> Track {
        Track {
            title: title.into(),
            id: "id".into(),
            duration: None,
            channel: channel.into(),
        }
    }

    fn info(artist: &str, title: &str) -> SongInfo {
        SongInfo {
            artist: artist.into(),
            title: title.into(),
        }
    }

    #[test]
    fn topic_channels_name_the_artist() {
        assert_eq!(
            song_info(&track(
                "Bohemian Rhapsody (Remastered 2011)",
                "Queen - Topic"
            )),
            info("Queen", "Bohemian Rhapsody (Remastered 2011)")
        );
    }

    #[test]
    fn dashed_titles_split_into_artist_and_song() {
        assert_eq!(
            song_info(&track(
                "Queen – Don't Stop Me Now (Official Video)",
                "Queen Official"
            )),
            info("Queen", "Don't Stop Me Now")
        );
        assert_eq!(
            song_info(&track(
                "Daft Punk - One More Time [Official Audio]",
                "Daft Punk"
            )),
            info("Daft Punk", "One More Time")
        );
    }

    #[test]
    fn plain_titles_fall_back_to_the_channel() {
        assert_eq!(
            song_info(&track(
                "Blinding Lights (Official Music Video)",
                "TheWeekndVEVO"
            )),
            info("TheWeeknd", "Blinding Lights")
        );
    }

    #[test]
    fn meaningful_brackets_are_kept() {
        assert_eq!(
            clean_title("Under Pressure (feat. David Bowie) [HD]"),
            "Under Pressure (feat. David Bowie)"
        );
        assert_eq!(clean_title("Song (Lyric Video)"), "Song");
        assert_eq!(clean_title("Unclosed (bracket"), "Unclosed (bracket");
    }
}
