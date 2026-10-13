use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
pub(crate) struct YtEntry {
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) duration: Option<f64>,
    pub(crate) channel: Option<String>,
    pub(crate) uploader: Option<String>,
    pub(crate) webpage_url: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CookieBrowser {
    None,
    Chrome,
    Edge,
    Firefox,
    Helium,
}

impl Default for CookieBrowser {
    fn default() -> Self {
        Self::None
    }
}

impl CookieBrowser {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Chrome => "Chrome",
            Self::Edge => "Edge",
            Self::Firefox => "Firefox",
            Self::Helium => "Helium",
        }
    }

    pub(crate) fn yt_dlp_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Chrome => Some("chrome"),
            Self::Edge => Some("edge"),
            Self::Firefox => Some("firefox"),
            Self::Helium => None,
        }
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
