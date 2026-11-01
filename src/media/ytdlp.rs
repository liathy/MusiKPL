use crate::{
    domain::{Track, YtEntry},
    infrastructure::{
        bundled,
        process::{hide_console, run_registered, ProcessRegistry},
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

impl Tools {
    /// Find mpv and yt-dlp, in order of preference:
    ///
    /// 1. Beside MusiKPL.exe, so a user can supply their own versions.
    /// 2. The copies bundled inside release builds, unpacked once.
    /// 3. In development builds, the project root.
    pub(crate) fn discover() -> Self {
        let beside_exe = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        let mut folders = vec![beside_exe.clone()];
        folders.extend(bundled::unpack());
        #[cfg(debug_assertions)]
        folders.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

        let find = |name: &str| {
            folders
                .iter()
                .map(|folder| folder.join(name))
                .find(|path| path.is_file())
                .unwrap_or_else(|| beside_exe.join(name))
        };
        Self {
            mpv: find("mpv.exe"),
            yt_dlp: find("yt-dlp.exe"),
        }
    }

    pub(crate) fn ready(&self) -> Result<(), String> {
        if !self.mpv.exists() || !self.yt_dlp.exists() {
            return Err(format!(
                "MusiKPL couldn't find its playback tools. Place mpv.exe and yt-dlp.exe next to MusiKPL.exe. Missing: {}{}",
                if self.mpv.exists() { "" } else { "mpv.exe " },
                if self.yt_dlp.exists() {
                    ""
                } else {
                    "yt-dlp.exe"
                }
            ));
        }
        Ok(())
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

/// Like [query_ytdlp], reading at most limit entries. YouTube Mixes
/// never end, so without a limit yt-dlp keeps fetching more of them.
pub(crate) fn query_ytdlp_limited(
    tools: &Tools,
    query: &str,
    single: bool,
    limit: Option<usize>,
    registry: &ProcessRegistry,
) -> Result<Vec<Track>, String> {
    tools.ready()?;
    let target = if single {
        query.to_string()
    } else {
        format!("ytsearch8:{query}")
    };
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
    command.arg(&target);
    let output = run_registered(command, registry)?;
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

pub(crate) fn resolve_audio_url(
    tools: &Tools,
    track: &Track,
    registry: &ProcessRegistry,
) -> Result<String, String> {
    tools.ready()?;
    let mut command = Command::new(&tools.yt_dlp);
    hide_console(&mut command);
    command.args([
        "--get-url",
        "--format",
        "bestaudio/best",
        "--no-playlist",
        "--no-warnings",
        &track.url(),
        "--extractor-args",
        "youtube:player_client=android",
    ]);
    let output = run_registered(command, registry)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("http"))
        .map(str::to_owned)
        .ok_or_else(|| "yt-dlp did not return an audio stream".into())
}
