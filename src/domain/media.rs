use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct YtEntry {
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) duration: Option<f64>,
    pub(crate) channel: Option<String>,
    pub(crate) uploader: Option<String>,
    pub(crate) webpage_url: Option<String>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum CookieBrowser {
    #[default]
    None,
    Chrome,
    Edge,
    Firefox,
    Helium,
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
