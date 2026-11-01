//! Updates from the latest GitHub release.
//!
//! Installed copies (in `%LOCALAPPDATA%\Programs\MusiKPL`) run the new
//! release's setup in update mode, which waits for MusiKPL to close,
//! replaces it, and reopens it. Portable copies swap their own exe: Windows
//! lets a running program be renamed, so the old exe moves aside, the new
//! one takes its name, and the new one is started. User data lives in
//! `%APPDATA%\MusiKPL` either way and is never touched.

use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use serde_json::Value;

const LATEST_RELEASE: &str = "https://api.github.com/repos/liathy/MusiKPL/releases/latest";
pub(crate) const SETUP_ASSET: &str = "MusiKPL-Setup.exe";
/// The release file that replaces this build: the Windows exe, or the
/// Linux binary.
pub(crate) const PORTABLE_ASSET: &str = if cfg!(windows) {
    "MusiKPL.exe"
} else {
    "MusiKPL-linux-x86_64"
};
/// Argument for a freshly updated portable copy: wait for this process id.
pub(crate) const AFTER_UPDATE_ARG: &str = "--after-update";

/// A release newer than the running version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Release {
    pub(crate) version: String,
    setup_url: Option<String>,
    portable_url: Option<String>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(concat!("MusiKPL/", env!("CARGO_PKG_VERSION")))
        .build()
}

/// Ask GitHub for the latest release. `Ok(None)` means up to date.
pub(crate) fn check() -> Result<Option<Release>, String> {
    let response = agent()
        .get(LATEST_RELEASE)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("Couldn't reach GitHub to check for updates ({e})"))?;
    let body = response
        .into_string()
        .map_err(|e| format!("Couldn't read the update information ({e})"))?;
    let value: Value =
        serde_json::from_str(&body).map_err(|_| "GitHub sent unreadable update information")?;
    Ok(parse_release(&value)
        .filter(|release| is_newer(&release.version, env!("CARGO_PKG_VERSION"))))
}

fn parse_release(value: &Value) -> Option<Release> {
    if value.get("draft").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let version = value
        .get("tag_name")
        .and_then(Value::as_str)?
        .trim_start_matches('v')
        .to_owned();
    let asset = |name: &str| {
        value
            .get("assets")?
            .as_array()?
            .iter()
            .find(|asset| asset.get("name").and_then(Value::as_str) == Some(name))?
            .get("browser_download_url")?
            .as_str()
            .filter(|url| url.starts_with("https://"))
            .map(str::to_owned)
    };
    Some(Release {
        version,
        setup_url: asset(SETUP_ASSET),
        portable_url: asset(PORTABLE_ASSET),
    })
}

/// Whether version `candidate` ("2.3.0") is newer than `current`.
pub(crate) fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |version: &str| -> Vec<u64> {
        version
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .take(3)
            .map(|part| part.parse().unwrap_or(0))
            .collect()
    };
    parts(candidate) > parts(current)
}

/// Where setup installs MusiKPL.
pub(crate) fn install_dir() -> Option<PathBuf> {
    super::install::install_dir()
}

/// Whether this copy is the installed one (as opposed to portable).
pub(crate) fn is_installed() -> bool {
    let (Ok(exe), Some(dir)) = (std::env::current_exe(), install_dir()) else {
        return false;
    };
    exe.parent().is_some_and(|parent| same_path(parent, &dir))
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .trim_end_matches(['\\', '/'])
        .eq_ignore_ascii_case(b.to_string_lossy().trim_end_matches(['\\', '/']))
}

/// Download `url` to `target`, reporting progress in per-mille.
fn download(url: &str, target: &Path, progress: &AtomicU32) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| format!("The update download failed ({e})");
    let response = agent().get(url).call().map_err(|e| failed(&e))?;
    let total: u64 = response
        .header("Content-Length")
        .and_then(|length| length.parse().ok())
        .unwrap_or(0);
    let temporary = target.with_extension("part");
    let mut file = std::fs::File::create(&temporary).map_err(|e| failed(&e))?;
    let mut reader = response.into_reader();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut done: u64 = 0;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| failed(&e))?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|e| failed(&e))?;
        done += read as u64;
        if let Some(per_mille) = (done * 1000).checked_div(total) {
            progress.store(per_mille.min(1000) as u32, Ordering::Relaxed);
        }
    }
    file.flush().map_err(|e| failed(&e))?;
    drop(file);
    if total > 0 && done != total {
        let _ = std::fs::remove_file(&temporary);
        return Err("The update download was cut short. Try again.".into());
    }
    std::fs::rename(&temporary, target).map_err(|e| failed(&e))
}

/// Download and start installing `release`. On success MusiKPL must close
/// right away; the new version opens by itself.
pub(crate) fn install(release: &Release, progress: &AtomicU32) -> Result<(), String> {
    let pid = std::process::id().to_string();
    if is_installed() {
        if let Some(url) = &release.setup_url {
            let setup = std::env::temp_dir().join(SETUP_ASSET);
            download(url, &setup, progress)?;
            Command::new(&setup)
                .args(["--update", "--wait-pid", &pid])
                .spawn()
                .map_err(|e| format!("Couldn't start the update ({e})"))?;
            return Ok(());
        }
    }
    let url = release
        .portable_url
        .as_ref()
        .ok_or_else(|| format!("This release has no {PORTABLE_ASSET} to update with"))?;
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find MusiKPL ({e})"))?;
    let new = beside(&exe, "new");
    download(url, &new, progress)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Couldn't make the update runnable ({e})"))?;
    }
    let old = beside(&exe, "old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| format!("Couldn't replace MusiKPL ({e})"))?;
    if let Err(error) = std::fs::rename(&new, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("Couldn't replace MusiKPL ({error})"));
    }
    Command::new(&exe)
        .args([AFTER_UPDATE_ARG, &pid])
        .spawn()
        .map_err(|e| format!("Couldn't start the new version ({e})"))?;
    Ok(())
}

/// MusiKPL.exe → MusiKPL.exe.old, MusiKPL → MusiKPL.old.
fn beside(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    exe.with_file_name(name)
}

/// After a portable update: remove the previous exe that was moved aside.
pub(crate) fn clean_up_previous_version() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(beside(&exe, "old"));
    }
}

/// Wait (at most 20 s) for process `pid` to exit.
pub(crate) fn wait_for_exit(pid: u32) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        // SAFETY: plain handle use; a missing process yields a null handle.
        unsafe {
            let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
            if handle != 0 {
                WaitForSingleObject(handle, 20_000);
                CloseHandle(handle);
            }
        }
    }
    // Linux: the process is gone once /proc no longer lists it.
    #[cfg(not(windows))]
    {
        let started = std::time::Instant::now();
        let alive = Path::new("/proc").join(pid.to_string());
        while alive.exists() && started.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("2.3.0", "2.2.0"));
        assert!(is_newer("v2.10.0", "2.9.9"));
        assert!(is_newer("3.0.0", "2.99.99"));
        assert!(!is_newer("2.2.0", "2.2.0"));
        assert!(!is_newer("2.1.9", "2.2.0"));
        assert!(!is_newer("garbage", "2.2.0"));
    }

    #[test]
    fn releases_are_read_from_github_answers() {
        let value: Value = serde_json::from_str(
            r#"{"tag_name":"v2.3.0","draft":false,"assets":[
                {"name":"MusiKPL.exe","browser_download_url":"https://github.com/x/MusiKPL.exe"},
                {"name":"MusiKPL-linux-x86_64","browser_download_url":"https://github.com/x/MusiKPL-linux-x86_64"},
                {"name":"MusiKPL-Setup.exe","browser_download_url":"https://github.com/x/MusiKPL-Setup.exe"}
            ]}"#,
        )
        .unwrap();
        let release = parse_release(&value).unwrap();
        assert_eq!(release.version, "2.3.0");
        assert_eq!(
            release.setup_url.as_deref(),
            Some("https://github.com/x/MusiKPL-Setup.exe")
        );
        assert!(release.portable_url.is_some());

        let draft: Value = serde_json::from_str(r#"{"tag_name":"v9.0.0","draft":true}"#).unwrap();
        assert!(parse_release(&draft).is_none());
        let insecure: Value = serde_json::from_str(
            r#"{"tag_name":"v9.0.0","assets":[{"name":"MusiKPL.exe","browser_download_url":"http://x"}]}"#,
        )
        .unwrap();
        assert!(parse_release(&insecure).unwrap().portable_url.is_none());
    }

    #[test]
    fn swapped_files_sit_beside_the_program() {
        assert_eq!(
            beside(Path::new("/opt/MusiKPL"), "old"),
            Path::new("/opt/MusiKPL.old")
        );
        assert_eq!(
            beside(Path::new(r"C:\x\MusiKPL.exe"), "new"),
            Path::new(r"C:\x\MusiKPL.exe.new")
        );
    }

    #[test]
    fn paths_compare_like_windows_does() {
        assert!(same_path(
            Path::new(r"C:\Users\A\AppData\Local\Programs\MusiKPL\"),
            Path::new(r"c:\users\a\appdata\local\programs\musikpl")
        ));
        assert!(!same_path(
            Path::new(r"D:\Downloads"),
            Path::new(r"C:\Downloads")
        ));
    }
}
