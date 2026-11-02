use super::cookies;
use crate::{
    domain::{Track, YtEntry},
    infrastructure::{
        bundled,
        process::{hide_console, ProcessRegistry},
    },
};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone)]
pub(crate) struct Tools {
    pub(crate) mpv: PathBuf,
    pub(crate) yt_dlp: PathBuf,
}

/// The tools' file names on this system.
pub(crate) const MPV: &str = if cfg!(windows) { "mpv.exe" } else { "mpv" };
pub(crate) const YT_DLP: &str = if cfg!(windows) {
    "yt-dlp.exe"
} else {
    "yt-dlp"
};

impl Tools {
    /// Find mpv and yt-dlp, in order of preference:
    ///
    /// 1. Beside MusiKPL, so a user can supply their own versions.
    /// 2. The copies bundled inside Windows release builds, unpacked once.
    /// 3. In development builds, the project root.
    /// 4. On Linux, MusiKPL's own downloaded yt-dlp (kept up to date).
    /// 5. Installed on the system (in `PATH`), as mpv is on Linux.
    pub(crate) fn discover() -> Self {
        let beside_exe = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let mut folders = vec![beside_exe.clone()];
        folders.extend(bundled::unpack());
        // Linux: MusiKPL's own up-to-date yt-dlp, before any system copy.
        folders.extend(
            bundled::managed_yt_dlp().and_then(|path| path.parent().map(Path::to_path_buf)),
        );
        #[cfg(debug_assertions)]
        folders.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        if let Some(path) = std::env::var_os("PATH") {
            folders.extend(std::env::split_paths(&path));
        }

        let find = |name: &str| {
            folders
                .iter()
                .map(|folder| folder.join(name))
                .find(|path| path.is_file())
                .unwrap_or_else(|| beside_exe.join(name))
        };
        Self {
            mpv: find(MPV),
            yt_dlp: find(YT_DLP),
        }
    }

    pub(crate) fn ready(&self) -> Result<(), String> {
        if self.mpv.exists() && self.yt_dlp.exists() {
            return Ok(());
        }
        let missing: Vec<&str> = [(MPV, &self.mpv), (YT_DLP, &self.yt_dlp)]
            .into_iter()
            .filter(|(_, path)| !path.exists())
            .map(|(name, _)| name)
            .collect();
        let missing = missing.join(" and ");
        Err(if cfg!(windows) {
            format!("MusiKPL couldn't find {missing}. Place it next to MusiKPL.exe.")
        } else {
            let mut help = Vec::new();
            if !self.mpv.exists() {
                help.push("install mpv (on Ubuntu: sudo apt install mpv)");
            }
            if !self.yt_dlp.exists() {
                help.push("MusiKPL downloads yt-dlp by itself on first start, which needs an internet connection; try again in a moment");
            }
            format!("MusiKPL couldn't find {missing}: {}.", help.join("; "))
        })
    }
}
pub(crate) fn query_ytdlp(
    tools: &Tools,
    query: &str,
    single: bool,
    registry: &ProcessRegistry,
) -> Result<Vec<Track>, String> {
    query_ytdlp_limited(tools, query, single, None, registry)
}

/// Like [`query_ytdlp`], reading at most `limit` entries. YouTube Mixes
/// never end, so without a limit yt-dlp keeps fetching more of them.
///
/// A search (`single` false) that comes back empty is tried again, then on
/// YouTube Music's search, which is a separate service: YouTube's own
/// search sometimes answers nothing for a moment.
pub(crate) fn query_ytdlp_limited(
    tools: &Tools,
    query: &str,
    single: bool,
    limit: Option<usize>,
    registry: &ProcessRegistry,
) -> Result<Vec<Track>, String> {
    tools.ready()?;
    if single {
        return run_query(tools, query, limit, registry);
    }
    let attempts = [
        format!("ytsearch{SEARCH_RESULTS}:{query}"),
        format!("ytsearch{SEARCH_RESULTS}:{query}"),
        format!(
            "https://music.youtube.com/search?q={}#songs",
            url_encode(query)
        ),
    ];
    let mut last_error = String::new();
    for (index, target) in attempts.iter().enumerate() {
        if index == 1 {
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
        match run_query(
            tools,
            target,
            Some(limit.unwrap_or(SEARCH_RESULTS)),
            registry,
        ) {
            Ok(tracks) if !tracks.is_empty() => return Ok(tracks),
            Ok(_) => last_error = "YouTube found nothing for that search".into(),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

/// How many songs a search shows.
const SEARCH_RESULTS: usize = 20;

/// For a URL's query string: letters, digits and `-_.~` stay, spaces become
/// `+`, everything else is percent-encoded.
fn url_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Run yt-dlp on one target (a video, playlist, Mix or search).
fn run_query(
    tools: &Tools,
    target: &str,
    limit: Option<usize>,
    registry: &ProcessRegistry,
) -> Result<Vec<Track>, String> {
    let build = || {
        let mut command = Command::new(&tools.yt_dlp);
        hide_console(&mut command);
        command.args([
            "--dump-json",
            "--skip-download",
            "--flat-playlist",
            "--no-warnings",
        ]);
        if let Some(limit) = limit {
            command.args(["--playlist-end", &limit.to_string()]);
        }
        command.arg(target);
        command
    };
    let output = cookies::run_yt_dlp(&build, registry)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(parse_ytdlp_entries(&output.stdout, &output.stderr)?
        .into_iter()
        .filter_map(|entry| {
            let id = entry.id.or_else(|| {
                entry
                    .webpage_url
                    .and_then(|url| url.split("v=").nth(1).map(str::to_owned))
            })?;
            Some(Track {
                title: entry.title.unwrap_or_else(|| "Untitled".into()),
                id,
                duration: entry.duration,
                channel: entry.channel.or(entry.uploader).unwrap_or_default(),
            })
        })
        .collect())
}
pub(crate) fn parse_ytdlp_entries(stdout: &[u8], stderr: &[u8]) -> Result<Vec<YtEntry>, String> {
    if stdout.iter().all(u8::is_ascii_whitespace) {
        let message = String::from_utf8_lossy(stderr).trim().to_owned();
        return Err(if message.is_empty() {
            "yt-dlp returned no search data".into()
        } else {
            message
        });
    }
    let entries: Vec<YtEntry> = String::from_utf8_lossy(stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    if entries.is_empty() {
        return Err("yt-dlp returned no usable search results".into());
    }
    Ok(entries)
}

/// The audio stream for `track`. When YouTube won't give that upload's
/// stream (age-restricted, blocked in this country, removed, private, or no
/// playable format), another upload of the same song is played instead.
pub(crate) fn resolve_audio_url(
    tools: &Tools,
    track: &Track,
    registry: &ProcessRegistry,
) -> Result<String, String> {
    tools.ready()?;
    let error = match resolve_upload(tools, &track.url(), registry) {
        Ok(url) => return Ok(url),
        Err(error) => error,
    };
    // Other uploads would hit the same bot check, and asking for them adds
    // to the requests that caused it.
    if cookies::is_bot_check(&error) {
        return Err(plain_error(&error));
    }
    // Look for the same song uploaded by someone else, about as long.
    let song = crate::domain::song_info(track);
    let query = if song.artist.is_empty() {
        format!("ytsearch6:{}", song.title)
    } else {
        format!("ytsearch6:{} {}", song.artist, song.title)
    };
    let close_enough = |candidate: &Track| match (track.duration, candidate.duration) {
        (Some(wanted), Some(found)) => (wanted - found).abs() <= 25.0,
        _ => true,
    };
    if let Ok(candidates) = run_query(tools, &query, Some(6), registry) {
        for candidate in candidates
            .into_iter()
            .filter(|candidate| candidate.id != track.id && close_enough(candidate))
            .take(2)
        {
            if let Ok(url) = resolve_upload(tools, &candidate.url(), registry) {
                return Ok(url);
            }
        }
    }
    Err(plain_error(&error))
}

/// One upload's stream. The Android client is quickest when it works; if
/// YouTube gives it no usable stream, yt-dlp's own default clients are
/// tried.
///
/// From the Android client only an audio-only stream is taken: for some
/// videos it offers just the combined video file, which YouTube cuts short,
/// so mpv fails on it and the song is skipped.
fn resolve_upload(tools: &Tools, url: &str, registry: &ProcessRegistry) -> Result<String, String> {
    let mut last_error = String::new();
    for (client, format) in [
        (Some("youtube:player_client=android"), "bestaudio"),
        (None, "bestaudio/best"),
    ] {
        let build = || {
            let mut command = Command::new(&tools.yt_dlp);
            hide_console(&mut command);
            command.args([
                "--get-url",
                "--format",
                format,
                "--no-playlist",
                "--no-warnings",
                url,
            ]);
            if let Some(client) = client {
                command.args(["--extractor-args", client]);
            }
            command
        };
        let output = cookies::run_yt_dlp(&build, registry)?;
        if output.status.success() {
            if let Some(url) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .find(|line| line.starts_with("http"))
            {
                return Ok(url.to_owned());
            }
            last_error = "yt-dlp did not return an audio stream".into();
        } else {
            last_error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        }
    }
    Err(last_error)
}

/// What a yt-dlp failure means, in plain words.
pub(crate) fn plain_error(error: &str) -> String {
    let lower = error.to_lowercase();
    let reason = if lower.contains("confirm your age")
        || lower.contains("age-restricted")
        || lower.contains("inappropriate for some users")
    {
        "This video is age-restricted on YouTube"
    } else if cookies::is_bot_check(&lower) {
        return if cookies::have_saved() {
            "YouTube wants to confirm MusiKPL isn't a bot (too many requests from your connection), even with your browser's YouTube sign-in. Try again in a few minutes.".into()
        } else {
            "YouTube wants to confirm MusiKPL isn't a bot (too many requests from your connection). Sign in to YouTube in Helium or Firefox, then close the browser once so MusiKPL can save that sign-in, or try again in a few minutes.".into()
        };
    } else if lower.contains("not available in your country")
        || lower.contains("blocked it in your country")
        || lower.contains("geo")
    {
        "This video is blocked in your country"
    } else if lower.contains("private video") {
        "This video is private"
    } else if lower.contains("video unavailable")
        || lower.contains("has been removed")
        || lower.contains("copyright")
        || lower.contains("terminated")
    {
        "This video was removed from YouTube"
    } else if lower.contains("members-only") || lower.contains("join this channel") {
        "This video is for the channel's paying members only"
    } else if lower.contains("requested format is not available") {
        "YouTube offered no playable stream for this video"
    } else {
        return error.to_owned();
    };
    format!("{reason}, and no other copy of the song could be played.")
}

#[cfg(test)]
mod fallback_tests {
    use super::*;

    #[test]
    fn searches_are_url_encoded() {
        assert_eq!(url_encode("Hardwell & Co é"), "Hardwell+%26+Co+%C3%A9");
    }

    #[test]
    fn failures_read_plainly() {
        assert!(plain_error("ERROR: Sign in to confirm your age")
            .starts_with("This video is age-restricted"));
        assert!(plain_error("ERROR: [youtube] x: Video unavailable").contains("removed"));
        assert_eq!(plain_error("something else"), "something else");
    }
}
