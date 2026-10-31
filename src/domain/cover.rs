//! Where a track's cover image comes from.
//!
//! Every YouTube video has a thumbnail, so it is always the fallback. When
//! Last.fm knows the song's album, its square album art is preferred.

use std::collections::HashMap;

use super::Track;

/// An image URL and the part of it to show as a square cover, as normalized
/// `[left, top, right, bottom]` texture coordinates.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Cover {
    pub(crate) url: String,
    pub(crate) uv: [f32; 4],
}

const FULL_IMAGE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// Album art found on Last.fm, by YouTube video ID. `None` records a lookup
/// that is running or found nothing, so it is not repeated.
#[derive(Default)]
pub(crate) struct CoverArt {
    found: HashMap<String, Option<String>>,
}

impl CoverArt {
    pub(crate) fn insert(&mut self, video_id: String, url: Option<String>) {
        self.found.insert(video_id, url);
    }

    pub(crate) fn has_looked_up(&self, video_id: &str) -> bool {
        self.found.contains_key(video_id)
    }

    /// Mark a lookup as started so it is only requested once.
    pub(crate) fn mark_pending(&mut self, video_id: &str) {
        self.found.entry(video_id.to_owned()).or_insert(None);
    }

    pub(crate) fn cover_for(&self, track: &Track) -> Option<Cover> {
        if let Some(Some(url)) = self.found.get(&track.id) {
            return Some(Cover {
                url: url.clone(),
                uv: FULL_IMAGE,
            });
        }
        youtube_thumbnail(&track.id)
    }
}

/// The video's 320×180 thumbnail, cropped to its centered square. Music
/// uploads place the album art in the middle of the 16:9 frame.
pub(crate) fn youtube_thumbnail(video_id: &str) -> Option<Cover> {
    if !is_video_id(video_id) {
        return None;
    }
    let side = 180.0 / 320.0;
    let left = (1.0 - side) / 2.0;
    Some(Cover {
        url: format!("https://i.ytimg.com/vi/{video_id}/mqdefault.jpg"),
        uv: [left, 0.0, left + side, 1.0],
    })
}

fn is_video_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: "Song".into(),
            id: id.into(),
            duration: None,
            channel: "Artist".into(),
        }
    }

    #[test]
    fn thumbnails_are_square_center_crops() {
        let cover = youtube_thumbnail("dQw4w9WgXcQ").unwrap();
        assert_eq!(
            cover.url,
            "https://i.ytimg.com/vi/dQw4w9WgXcQ/mqdefault.jpg"
        );
        let [left, top, right, bottom] = cover.uv;
        // Square in pixels: 180 px of the 320 px width, the full height.
        assert!(((right - left) * 320.0 - 180.0).abs() < 0.01);
        assert!(((left + right) / 2.0 - 0.5).abs() < 0.001);
        assert_eq!((top, bottom), (0.0, 1.0));
    }

    #[test]
    fn unsafe_ids_get_no_url() {
        assert!(youtube_thumbnail("../../etc").is_none());
        assert!(youtube_thumbnail("").is_none());
    }

    #[test]
    fn album_art_is_preferred_when_found() {
        let mut art = CoverArt::default();
        art.mark_pending("abc");
        assert!(art.has_looked_up("abc"));
        assert!(art.cover_for(&track("abc")).unwrap().url.contains("ytimg"));

        art.insert("abc".into(), Some("https://lastfm.example/a.png".into()));
        let cover = art.cover_for(&track("abc")).unwrap();
        assert_eq!(cover.url, "https://lastfm.example/a.png");
        assert_eq!(cover.uv, FULL_IMAGE);
    }
}
