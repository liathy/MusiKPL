//! Updates from the latest GitHub release.
//!
//! Installed copies (in `%LOCALAPPDATA%\Programs\MusiKPL`) run the new
//! release's setup in update mode, which waits for MusiKPL to close,
//! replaces it, and reopens it. Portable copies swap their own exe: Windows
//! lets a running program be renamed, so the old exe moves aside, the new
//! one takes its name, and the new one is started. User data lives in
//! `%APPDATA%\MusiKPL` either way and is never touched.

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicU32,
};

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

/// Ask GitHub for the latest release (through Cloudflare's DNS, in the
/// core). `Ok(None)` means up to date.
pub(crate) fn check() -> Result<Option<Release>, String> {
    Ok(musikpl_core::updates::check()?.map(|release| Release {
        setup_url: release.asset(SETUP_ASSET),
        portable_url: release.asset(PORTABLE_ASSET),
        version: release.version,
    }))
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

fn download(url: &str, target: &Path, progress: &AtomicU32) -> Result<(), String> {
    musikpl_core::updates::download(url, target, progress)
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
        while alive.exists() && started.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
