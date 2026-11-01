//! Where a track's cover image comes from: its YouTube thumbnail, which
//! every video has and which loads straight away.

use super::Track;

/// An image URL and the part of it to show as a square cover, as normalized
/// `[left, top, right, bottom]` texture coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Cover {
    pub url: String,
    pub uv: [f32; 4],
}

/// Covers for songs on screen.
#[derive(Default)]
pub struct CoverArt;

impl CoverArt {
    pub fn cover_for(&self, track: &Track) -> Option<Cover> {
        youtube_thumbnail(&track.id)
    }
}
/// The video's 320×180 thumbnail, cropped to its centered square. Music
/// uploads place the album art in the middle of the 16:9 frame.
pub fn youtube_thumbnail(video_id: &str) -> Option<Cover> {
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
}
