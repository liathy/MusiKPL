#[derive(Clone, Debug)]
pub(crate) struct Track {
    pub(crate) title: String,
    pub(crate) id: String,
    pub(crate) duration: Option<f64>,
    pub(crate) channel: String,
}

impl Track {
    pub(crate) fn url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.id)
    }

    pub(crate) fn duration_text(&self) -> String {
        self.duration
            .map(format_duration)
            .unwrap_or_else(|| "--:--".into())
    }
}

pub(crate) fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}

pub(crate) fn format_position(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

pub(crate) fn youtube_mix_url(track: &Track) -> String {
    format!(
        "https://www.youtube.com/watch?v={}&list=RD{}",
        track.id, track.id
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: "Song".into(),
            id: id.into(),
            duration: Some(125.9),
            channel: "Test artist".into(),
        }
    }

    #[test]
    fn duration_format_is_stable_for_progress_and_queue_rows() {
        assert_eq!(format_duration(0.0), "0:00");
        assert_eq!(format_duration(125.9), "2:05");
        assert_eq!(format_duration(-1.0), "0:00");
        assert_eq!(format_position(0.0), "00:00");
        assert_eq!(format_position(125.9), "02:05");
    }

    #[test]
    fn mix_url_uses_youtube_rd_playlist_for_the_seed_track() {
        assert_eq!(
            youtube_mix_url(&track("abc123")),
            "https://www.youtube.com/watch?v=abc123&list=RDabc123"
        );
    }
}
