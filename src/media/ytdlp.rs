use crate::{
    domain::{CookieBrowser, Track, YtEntry},
    infrastructure::process::{hide_console, run_registered, ProcessRegistry},
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
    pub(crate) fn discover() -> Self {
        let base = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            mpv: base.join("mpv.exe"),
            yt_dlp: base.join("yt-dlp.exe"),
        }
    }
    pub(crate) fn ready(&self) -> Result<(), String> {
        if !self.mpv.exists() || !self.yt_dlp.exists() {
            return Err(format!(
                "Place mpv.exe and yt-dlp.exe next to the MusiKPL executable. Missing: {}{}",
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
    cookie_browser: CookieBrowser,
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
    add_cookie_args(&mut command, cookie_browser)?;
    command.args([
        "--dump-json",
        "--skip-download",
        "--flat-playlist",
        "--no-warnings",
        &target,
    ]);
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
    cookie_browser: CookieBrowser,
    registry: &ProcessRegistry,
) -> Result<String, String> {
    tools.ready()?;
    let mut command = Command::new(&tools.yt_dlp);
    hide_console(&mut command);
    add_cookie_args(&mut command, cookie_browser)?;
    command.args([
        "--get-url",
        "--format",
        "bestaudio/best",
        "--no-playlist",
        "--no-warnings",
        &track.url(),
    ]);
    if cookie_browser == CookieBrowser::None {
        command.args(["--extractor-args", "youtube:player_client=android"]);
    }
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

fn helium_cookie_profile() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
        let candidates = [
            local_app_data.join("net.imput.helium").join("User Data"),
            local_app_data
                .join("imput")
                .join("Helium")
                .join("User Data"),
            local_app_data.join("Helium").join("User Data"),
        ];
        candidates.into_iter().find(|path| path.exists())
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn add_cookie_args(command: &mut Command, browser: CookieBrowser) -> Result<(), String> {
    if browser == CookieBrowser::Helium {
        let profile = helium_cookie_profile().ok_or_else(|| "Helium profile was not found. Sign in to Helium first, or verify its User Data folder.".to_string())?;
        command.args([
            "--cookies-from-browser",
            &format!("chromium:{}", profile.display()),
        ]);
    } else if let Some(name) = browser.yt_dlp_name() {
        command.args(["--cookies-from-browser", name]);
    }
    Ok(())
}
