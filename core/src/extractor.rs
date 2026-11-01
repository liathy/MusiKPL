//! Finding songs and their audio on YouTube: yt-dlp on desktop,
//! NewPipeExtractor (with yt-dlp as a fallback) on Android.
//!
//! Song lists are asked for by *source*, a small vocabulary every extractor
//! understands (it's yt-dlp's own, which the desktop passes straight on):
//!
//! - `ytsearchN:query`: the first N YouTube results for `query`
//! - `https://www.youtube.com/watch?v=ID&list=RDID`: the Mix for video ID
//! - any other YouTube link: that video or playlist

use crate::domain::Track;

pub trait Extractor: Send + Sync {
    /// The songs of a source (see the module docs), at most `limit` when set.
    fn list(&self, source: &str, limit: Option<usize>) -> Result<Vec<Track>, String>;

    /// What the user typed in the search box: songs for the words, or the
    /// song or playlist when it's a link. Implementations retry and fall
    /// back as they can, since an empty search is the worst case.
    fn search(&self, query: &str) -> Result<Vec<Track>, String>;

    /// The audio stream for `track`, ready for the [`crate::audio::AudioOutput`].
    /// When YouTube won't give that upload's stream, implementations may
    /// play another upload of the same song.
    fn stream_url(&self, track: &Track) -> Result<String, String>;
}

/// The parts of a `ytsearchN:query` source, if it is one.
pub fn parse_search(source: &str) -> Option<(usize, &str)> {
    let rest = source.strip_prefix("ytsearch")?;
    let (count, query) = rest.split_once(':')?;
    let count = if count.is_empty() {
        1
    } else {
        count.parse().ok()?
    };
    Some((count, query))
}

/// The video id of a Mix source (`…watch?v=ID&list=RDID`), if it is one.
pub fn parse_mix(source: &str) -> Option<&str> {
    let list = source
        .split(['?', '&'])
        .find_map(|part| part.strip_prefix("list=RD"))?;
    (!list.is_empty()).then_some(list)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_read() {
        assert_eq!(
            parse_search("ytsearch30:new pop songs"),
            Some((30, "new pop songs"))
        );
        assert_eq!(parse_search("ytsearch:x"), Some((1, "x")));
        assert_eq!(parse_search("https://youtu.be/x"), None);
        assert_eq!(
            parse_mix("https://www.youtube.com/watch?v=abc&list=RDabc"),
            Some("abc")
        );
        assert_eq!(parse_mix("https://www.youtube.com/watch?v=abc"), None);
    }
}
