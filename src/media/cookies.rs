//! YouTube's bot check ("Sign in to confirm you're not a bot") answers too
//! many requests from one connection. yt-dlp gets past it with the cookies
//! of a browser where YouTube was used, so once YouTube asks, MusiKPL runs
//! yt-dlp with the user's browser cookies for a while.
//!
//! Chromium browsers (Helium among them) lock their cookie file while open,
//! so MusiKPL keeps its own copy, refreshed whenever the browser's file can
//! be read. Only the YouTube and Google cookies are kept in it.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    time::{Duration, Instant},
};

use crate::infrastructure::{
    bundled,
    process::{hide_console, run_registered, ProcessRegistry},
};

/// How long after a bot check every request carries cookies.
const COOLDOWN: Duration = Duration::from_secs(60 * 60);

static LAST_BOT_CHECK: Mutex<Option<Instant>> = Mutex::new(None);
static EXPORTS: AtomicU64 = AtomicU64::new(0);

/// Whether a yt-dlp error is YouTube's bot check.
pub(crate) fn is_bot_check(error: &str) -> bool {
    error.to_lowercase().contains("not a bot")
}

fn recently_checked() -> bool {
    LAST_BOT_CHECK
        .lock()
        .ok()
        .and_then(|last| *last)
        .is_some_and(|at| at.elapsed() < COOLDOWN)
}

fn note_bot_check() {
    if let Ok(mut last) = LAST_BOT_CHECK.lock() {
        *last = Some(Instant::now());
    }
}

/// Run the yt-dlp command `build` makes. If YouTube has asked whether
/// MusiKPL is a bot (now or in the last hour), it is run with cookies.
pub(crate) fn run_yt_dlp(
    build: &dyn Fn() -> Command,
    registry: &ProcessRegistry,
) -> Result<Output, String> {
    if !recently_checked() {
        let output = run_registered(build(), registry)?;
        if output.status.success() || !is_bot_check(&String::from_utf8_lossy(&output.stderr)) {
            return Ok(output);
        }
        note_bot_check();
        return Ok(run_with_cookies(build, registry).unwrap_or(output));
    }
    match run_with_cookies(build, registry) {
        Some(output) => Ok(output),
        None => run_registered(build(), registry),
    }
}

/// Try each readable browser, then MusiKPL's saved cookies. `None` when no
/// cookies could be loaded at all.
fn run_with_cookies(build: &dyn Fn() -> Command, registry: &ProcessRegistry) -> Option<Output> {
    let jar = saved_jar()?;
    let dir = jar.parent()?.to_path_buf();
    std::fs::create_dir_all(&dir).ok()?;
    for browser in browsers() {
        let export = dir.join(format!(
            "cookies-{}-{}.tmp",
            std::process::id(),
            EXPORTS.fetch_add(1, Ordering::Relaxed)
        ));
        let mut command = build();
        command
            .arg("--cookies-from-browser")
            .arg(&browser)
            .arg("--cookies")
            .arg(&export);
        let output = run_registered(command, registry);
        // yt-dlp writes every site's cookies; keep only YouTube's.
        save_youtube_cookies(&export, &jar);
        let _ = std::fs::remove_file(&export);
        match output {
            Ok(output) if !cookies_unreadable(&output) => return Some(output),
            _ => continue,
        }
    }
    if !jar.is_file() {
        return None;
    }
    let mut command = build();
    command.arg("--cookies").arg(&jar);
    run_registered(command, registry).ok()
}

/// Save the browsers' YouTube cookies for later, in the background. This
/// reads them from disk without contacting YouTube. A browser that is open
/// now (and locking them) is read again next start, or when YouTube next
/// asks.
pub(crate) fn refresh_saved_in_background(yt_dlp: PathBuf, registry: ProcessRegistry) {
    std::thread::spawn(move || {
        // Leave startup and the yt-dlp update check to go first.
        std::thread::sleep(Duration::from_secs(20));
        let Some(jar) = saved_jar() else { return };
        let Some(dir) = jar.parent() else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        for browser in browsers() {
            let export = dir.join(format!("cookies-{}-startup.tmp", std::process::id()));
            let mut command = Command::new(&yt_dlp);
            hide_console(&mut command);
            // With no video given, yt-dlp just loads and writes the cookies.
            command
                .arg("--cookies-from-browser")
                .arg(&browser)
                .arg("--cookies")
                .arg(&export);
            let _ = run_registered(command, &registry);
            let saved = save_youtube_cookies(&export, &jar);
            let _ = std::fs::remove_file(&export);
            if saved {
                return;
            }
        }
    });
}

/// Keep the YouTube cookies of yt-dlp's `export` in `jar`. False when there
/// were none.
fn save_youtube_cookies(export: &Path, jar: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(export) else {
        return false;
    };
    let kept = keep_youtube(&text);
    let any = kept
        .lines()
        .any(|line| !line.is_empty() && (!line.starts_with('#') || line.starts_with("#HttpOnly_")));
    any && std::fs::write(jar, kept).is_ok()
}

/// Whether yt-dlp stopped because it couldn't load the browser's cookies
/// (browser open and locking them, not installed, or can't decrypt).
fn cookies_unreadable(output: &Output) -> bool {
    if output.status.success() {
        return false;
    }
    let error = String::from_utf8_lossy(&output.stderr).to_lowercase();
    !is_bot_check(&error) && (error.contains("cookie") || error.contains("decrypt"))
}

/// Whether MusiKPL has YouTube cookies saved to fall back on.
pub(crate) fn have_saved() -> bool {
    saved_jar().is_some_and(|jar| jar.is_file())
}

fn saved_jar() -> Option<PathBuf> {
    Some(bundled::tools_dir()?.parent()?.join("youtube-cookies.txt"))
}

/// The Netscape cookie file lines YouTube's sign-in needs: youtube.com,
/// google.com and accounts.google.com.
fn keep_youtube(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let domain = line
            .strip_prefix("#HttpOnly_")
            .unwrap_or(line)
            .split('\t')
            .next()
            .unwrap_or("");
        let is_comment = line.starts_with('#') && !line.starts_with("#HttpOnly_");
        let host = domain.trim_start_matches('.');
        let wanted = host == "youtube.com"
            || host.ends_with(".youtube.com")
            || host == "google.com"
            || host == "accounts.google.com";
        if is_comment || line.is_empty() || wanted {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// yt-dlp `--cookies-from-browser` values for the installed browsers it can
/// read, Helium first. (Chrome and Edge on Windows encrypt their cookies so
/// only they can read them, so they aren't tried there.)
fn browsers() -> Vec<OsString> {
    let mut found = Vec::new();
    if let Some(profile) = helium_profile() {
        let mut spec = OsString::from("chromium:");
        spec.push(profile);
        found.push(spec);
    }
    let firefox = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("Mozilla/Firefox"))
    } else {
        std::env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".mozilla/firefox"))
    };
    if firefox.is_some_and(|dir| dir.is_dir()) {
        found.push("firefox".into());
    }
    if !cfg!(windows) {
        if let Some(config) = std::env::var_os("HOME").map(|dir| PathBuf::from(dir).join(".config"))
        {
            for (folder, name) in [
                ("chromium", "chromium"),
                ("google-chrome", "chrome"),
                ("BraveSoftware/Brave-Browser", "brave"),
            ] {
                if config.join(folder).is_dir() {
                    found.push(name.into());
                }
            }
        }
    }
    found
}

/// Helium's last used profile folder.
fn helium_profile() -> Option<PathBuf> {
    let data = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("imput/Helium/User Data")
    } else {
        PathBuf::from(std::env::var_os("HOME")?).join(".config/net.imput.helium")
    };
    let last_used = std::fs::read_to_string(data.join("Local State"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|state| state["profile"]["last_used"].as_str().map(str::to_owned));
    let profile = [last_used.as_deref(), Some("Default")]
        .into_iter()
        .flatten()
        .map(|name| data.join(name))
        .find(|profile| profile.is_dir());
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_youtube_cookies_are_kept() {
        let text = "# Netscape HTTP Cookie File\n\
            .youtube.com\tTRUE\t/\tTRUE\t0\tSID\ta\n\
            #HttpOnly_.google.com\tTRUE\t/\tTRUE\t0\tHSID\tb\n\
            accounts.google.com\tFALSE\t/\tTRUE\t0\tLSID\tc\n\
            .notyoutube.com\tTRUE\t/\tTRUE\t0\tX\td\n\
            mail.google.com\tFALSE\t/\tTRUE\t0\tMAIL\tg\n\
            .amazon.com\tTRUE\t/\tTRUE\t0\tY\te\n\
            #HttpOnly_.bank.com\tTRUE\t/\tTRUE\t0\tZ\tf\n";
        let kept = keep_youtube(text);
        assert!(kept.contains("SID\ta") && kept.contains("HSID\tb") && kept.contains("LSID\tc"));
        for other in ["notyoutube", "MAIL", "amazon", "bank"] {
            assert!(!kept.contains(other), "{other} was kept");
        }
    }

    #[test]
    fn bot_check_is_recognised() {
        assert!(is_bot_check(
            "ERROR: [youtube] x: Sign in to confirm you're not a bot."
        ));
        assert!(!is_bot_check("ERROR: Video unavailable"));
    }
}
