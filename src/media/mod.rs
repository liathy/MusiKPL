mod ytdlp;

use musikpl_core::{domain::Track, extractor::Extractor};

use crate::infrastructure::process::ProcessRegistry;

pub(crate) use ytdlp::{query_ytdlp, query_ytdlp_limited, resolve_audio_url, Tools};

#[cfg(test)]
pub(crate) use ytdlp::parse_ytdlp_entries;

/// yt-dlp as the core's [`Extractor`]: the desktop finds songs and their
/// audio by running yt-dlp.
pub(crate) struct YtDlp {
    tools: Tools,
    registry: ProcessRegistry,
}

impl YtDlp {
    pub(crate) fn new(tools: Tools, registry: ProcessRegistry) -> Self {
        Self { tools, registry }
    }

    /// The tools to run now. Linux switches to MusiKPL's own yt-dlp once it
    /// has downloaded.
    fn tools(&self) -> Tools {
        let mut tools = self.tools.clone();
        if let Some(managed) = crate::infrastructure::bundled::managed_yt_dlp() {
            tools.yt_dlp = managed;
        }
        tools
    }
}

impl Extractor for YtDlp {
    fn list(&self, source: &str, limit: Option<usize>) -> Result<Vec<Track>, String> {
        query_ytdlp_limited(&self.tools(), source, true, limit, &self.registry)
    }

    fn search(&self, query: &str) -> Result<Vec<Track>, String> {
        let single = query.starts_with("http://") || query.starts_with("https://");
        query_ytdlp(&self.tools(), query, single, &self.registry)
    }

    fn stream_url(&self, track: &Track) -> Result<String, String> {
        resolve_audio_url(&self.tools(), track, &self.registry)
    }
}
