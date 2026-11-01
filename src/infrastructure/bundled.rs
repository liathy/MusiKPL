//! mpv.exe and yt-dlp.exe packed inside Windows release builds, and on
//! Linux, MusiKPL's own downloaded yt-dlp.
//!
//! Windows can only start programs from files, so on first run they are
//! unpacked once to `%LOCALAPPDATA%\MusiKPL\tools` and reused afterwards.
//! A new MusiKPL build with different tools replaces them. Like before,
//! they run only while needed: yt-dlp for each search or song lookup, mpv
//! while music plays.

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use super::process::{hide_console, run_registered, ProcessRegistry};

/// Identifies the tools packed into this build (set by build.rs).
const BUNDLE_ID: &str = env!("MUSIKPL_BUNDLE_ID");
const MARKER: &str = "bundle.id";
/// yt-dlp is kept current between MusiKPL releases, since YouTube changes
/// often: checked weekly, or the next day after a check failed (offline).
const UPDATE_INTERVAL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
const UPDATE_STAMP: &str = "yt-dlp.last-update";

#[cfg(bundled_tools)]
const PACKED: [(&str, &[u8]); 2] = [
    (
        "mpv.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/mpv.exe.deflate")),
    ),
    (
        "yt-dlp.exe",
        include_bytes!(concat!(env!("OUT_DIR"), "/yt-dlp.exe.deflate")),
    ),
];
#[cfg(not(bundled_tools))]
const PACKED: [(&str, &[u8]); 0] = [];

/// Where MusiKPL keeps its own tools: the bundled ones unpacked on Windows,
/// and the downloaded yt-dlp on Linux.
pub(crate) fn tools_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA"))?;
        Some(PathBuf::from(base).join("MusiKPL").join("tools"))
    }
    #[cfg(not(windows))]
    {
        super::storage::data_dir().map(|dir| dir.join("tools"))
    }
}

/// The official standalone yt-dlp for 64-bit Linux.
#[cfg(not(windows))]
const LINUX_YT_DLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";

/// Linux: MusiKPL's own yt-dlp, kept current, so an outdated system copy
/// (Ubuntu's package lags far behind YouTube) never breaks playback.
pub(crate) fn managed_yt_dlp() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let path = tools_dir()?.join("yt-dlp");
    path.is_file().then_some(path)
}

/// Linux: download MusiKPL's own yt-dlp in the background if it isn't there
/// yet. Until it arrives, a yt-dlp installed on the system is used.
#[cfg(not(windows))]
pub(crate) fn download_yt_dlp_in_background() {
    let Some(dir) = tools_dir() else {
        return;
    };
    let target = dir.join("yt-dlp");
    if target.is_file() {
        return;
    }
    std::thread::spawn(move || {
        if let Err(error) = download_yt_dlp(&dir) {
            eprintln!("Could not download yt-dlp: {error}");
        }
    });
}

/// Download the official Linux yt-dlp into dir as yt-dlp, runnable.
#[cfg(not(windows))]
fn download_yt_dlp(dir: &Path) -> Result<(), String> {
    use std::{io::Read, os::unix::fs::PermissionsExt};
    let target = dir.join("yt-dlp");
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let response = ureq::get(LINUX_YT_DLP_URL)
        .call()
        .map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(200 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    // A real program starts with the ELF signature.
    if !bytes.starts_with(b"\x7fELF") {
        return Err("the download isn't a Linux program".into());
    }
    let partial = dir.join("yt-dlp.part");
    std::fs::write(&partial, bytes).map_err(|e| e.to_string())?;
    std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| e.to_string())?;
    std::fs::rename(&partial, &target).map_err(|e| e.to_string())?;
    // Fresh from GitHub: no update check needed for a week.
    let _ = std::fs::write(dir.join(UPDATE_STAMP), "ok");
    Ok(())
}

#[cfg(all(test, not(windows)))]
mod linux_tests {
    /// Downloads the real yt-dlp from GitHub (run with --ignored).
    #[test]
    #[ignore]
    fn the_downloaded_yt_dlp_runs() {
        let dir = std::env::temp_dir().join(format!("musikpl-ytdlp-{}", std::process::id()));
        super::download_yt_dlp(&dir).expect("downloads");
        let output = std::process::Command::new(dir.join("yt-dlp"))
            .arg("--version")
            .output()
            .expect("runs");
        assert!(output.status.success());
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(windows)]
pub(crate) fn download_yt_dlp_in_background() {}

/// Make sure this build's tools are unpacked, and return their folder.
/// `None` when this build carries no tools (debug builds) or unpacking
/// failed; the caller then looks beside the executable.
pub(crate) fn unpack() -> Option<PathBuf> {
    if PACKED.is_empty() {
        return None;
    }
    let dir = tools_dir()?;
    let present = PACKED.iter().all(|(name, _)| dir.join(name).is_file());
    let current = std::fs::read_to_string(dir.join(MARKER)).is_ok_and(|id| id.trim() == BUNDLE_ID);
    if present && current {
        return Some(dir);
    }
    std::fs::create_dir_all(&dir).ok()?;
    for (name, packed) in PACKED {
        let target = dir.join(name);
        if let Err(error) = write_unpacked(&target, packed) {
            // A running copy (another MusiKPL window) keeps the old file
            // locked; it still works, so only fail without any file.
            if !target.is_file() {
                eprintln!("Could not unpack {name}: {error}");
                return None;
            }
        }
    }
    std::fs::write(dir.join(MARKER), BUNDLE_ID).ok()?;
    // The yt-dlp just unpacked may be older than one that updated itself
    // before; check for an update on this run.
    let _ = std::fs::remove_file(dir.join(UPDATE_STAMP));
    Some(dir)
}

#[cfg_attr(not(bundled_tools), allow(dead_code))]
fn write_unpacked(target: &Path, packed: &[u8]) -> Result<(), String> {
    let bytes = miniz_oxide::inflate::decompress_to_vec(packed)
        .map_err(|error| format!("corrupt bundle ({error:?})"))?;
    // Write beside the target and swap it in, so an interrupted first run
    // never leaves a truncated program behind.
    let temporary = target.with_extension("exe.tmp");
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, target).map_err(|e| {
        let _ = std::fs::remove_file(&temporary);
        e.to_string()
    })
}

/// Let the unpacked yt-dlp update itself in the background, at most weekly.
/// It replaces only its own copy in the tools folder.
pub(crate) fn update_yt_dlp_in_background(yt_dlp: PathBuf, registry: ProcessRegistry) {
    let Some(dir) = tools_dir() else {
        return;
    };
    if !yt_dlp.starts_with(&dir) {
        return; // A user-provided yt-dlp.exe is left alone.
    }
    let stamp = dir.join(UPDATE_STAMP);
    let last_ok = std::fs::read_to_string(&stamp).is_ok_and(|text| text.trim() == "ok");
    let interval = if last_ok {
        UPDATE_INTERVAL
    } else {
        RETRY_INTERVAL
    };
    let due = std::fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_none_or(|age| age >= interval);
    if !due {
        return;
    }
    std::thread::spawn(move || {
        let mut command = std::process::Command::new(&yt_dlp);
        hide_console(&mut command);
        command.args(["--update", "--no-warnings"]);
        let Ok(output) = run_registered(command, &registry) else {
            return; // The app is closing; try next time.
        };
        let _ = std::fs::write(
            stamp,
            if output.status.success() {
                "ok"
            } else {
                "failed"
            },
        );
    });
}
