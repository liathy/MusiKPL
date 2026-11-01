//! Installing for the current user only, with no administrator rights:
//! `%LOCALAPPDATA%\Programs\MusiKPL`, Start menu and Desktop shortcuts, and
//! an entry in Windows' Installed apps that runs the uninstaller.
//!
//! Shared by MusiKPL (which uninstalls itself) and MusiKPL-Setup (which
//! installs and updates it), so it only uses the standard library and the
//! Windows registry API.

#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub const EXE_NAME: &str = "MusiKPL.exe";
pub const UNINSTALL_ARG: &str = "--uninstall";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\MusiKPL";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "MusiKPL";

/// Where MusiKPL is installed.
pub fn install_dir() -> Option<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")?;
    Some(PathBuf::from(base).join("Programs").join("MusiKPL"))
}

pub fn installed_exe() -> Option<PathBuf> {
    install_dir().map(|dir| dir.join(EXE_NAME))
}

/// Folders holding the user's data: settings, taste and session in
/// Roaming, unpacked tools in Local.
pub fn data_dirs() -> Vec<PathBuf> {
    ["APPDATA", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("MusiKPL"))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shortcut {
    StartMenu,
    Desktop,
}

impl Shortcut {
    /// PowerShell expression for the shortcut's folder. Desktop may be
    /// redirected (to OneDrive, say), so Windows is asked where it is.
    /// The expression is parenthesized so it can be passed as an argument.
    fn folder(self) -> &'static str {
        match self {
            Self::StartMenu => "([Environment]::GetFolderPath('Programs'))",
            Self::Desktop => "([Environment]::GetFolderPath('Desktop'))",
        }
    }
}

fn powershell(script: &str) -> Result<(), String> {
    let mut command = Command::new("powershell.exe");
    command.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        // Stop at the first problem, so the error shown is the real one.
        &format!("$ErrorActionPreference = 'Stop'; {script}"),
    ]);
    hide_console(&mut command);
    let output = command
        .output()
        .map_err(|e| format!("Couldn't run PowerShell ({e})"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr
            .lines()
            .next()
            .unwrap_or("PowerShell failed")
            .trim()
            .to_owned())
    }
}

/// Quote for a single-quoted PowerShell string.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

pub fn create_shortcut(kind: Shortcut, exe: &Path) -> Result<(), String> {
    create_shortcut_in(kind.folder(), exe)
}

/// Create `MusiKPL.lnk` in the folder the PowerShell expression `folder`
/// names, pointing at `exe`.
fn create_shortcut_in(folder: &str, exe: &Path) -> Result<(), String> {
    let exe = exe.display().to_string();
    let working_dir = exe_folder(&exe);
    powershell(&format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path {folder} 'MusiKPL.lnk')); \
         $s.TargetPath = {}; $s.WorkingDirectory = {}; $s.IconLocation = {}; \
         $s.Description = 'MusiKPL music player'; $s.Save()",
        quoted(&exe),
        quoted(&working_dir),
        quoted(&format!("{exe},0")),
    ))
    .map_err(|e| format!("Couldn't create the shortcut ({e})"))
}

fn exe_folder(exe: &str) -> String {
    Path::new(exe)
        .parent()
        .map(|parent| parent.display().to_string())
        .unwrap_or_default()
}

pub fn remove_shortcuts() {
    for kind in [Shortcut::StartMenu, Shortcut::Desktop] {
        let _ = powershell(&format!(
            "Remove-Item -LiteralPath (Join-Path {} 'MusiKPL.lnk') -ErrorAction SilentlyContinue",
            kind.folder()
        ));
    }
}

/// Add MusiKPL to Installed apps, with its version and uninstaller.
pub fn register(dir: &Path, version: &str, size_kb: u32) -> Result<(), String> {
    let exe = dir.join(EXE_NAME).display().to_string();
    let strings = [
        ("DisplayName", "MusiKPL".to_owned()),
        ("DisplayVersion", version.to_owned()),
        ("Publisher", "Liathy".to_owned()),
        ("DisplayIcon", format!("{exe},0")),
        ("InstallLocation", dir.display().to_string()),
        ("UninstallString", format!("\"{exe}\" {UNINSTALL_ARG}")),
        ("QuietUninstallString", format!("\"{exe}\" {UNINSTALL_ARG}")),
        (
            "URLInfoAbout",
            "https://github.com/liathy/MusiKPL".to_owned(),
        ),
    ];
    for (name, value) in strings {
        registry::set_string(UNINSTALL_KEY, name, &value)?;
    }
    registry::set_dword(UNINSTALL_KEY, "NoModify", 1)?;
    registry::set_dword(UNINSTALL_KEY, "NoRepair", 1)?;
    registry::set_dword(UNINSTALL_KEY, "EstimatedSize", size_kb)?;
    Ok(())
}

pub fn unregister() {
    registry::delete_key(UNINSTALL_KEY);
    registry::delete_value(RUN_KEY, RUN_VALUE);
}

/// If MusiKPL starts with Windows, make it start the installed copy.
pub fn retarget_start_with_windows(exe: &Path) {
    if registry::has_value(RUN_KEY, RUN_VALUE) {
        let _ = registry::set_string(RUN_KEY, RUN_VALUE, &format!("\"{}\"", exe.display()));
    }
}

/// Close MusiKPL if it is running from `exe`, so it can be replaced. It is
/// asked to close like the user pressing ✕ (so it saves everything), and
/// stopped only if it hasn't after a few seconds. Copies running from
/// anywhere else (a portable MusiKPL.exe, say) are left alone.
pub fn close_running(exe: &Path) {
    let _ = close_running_script(exe);
}

fn close_running_script(exe: &Path) -> Result<(), String> {
    let exe = quoted(&exe.display().to_string());
    powershell(&format!(
        "$mine = @(Get-Process MusiKPL -ErrorAction SilentlyContinue | \
             Where-Object {{ $_.Path -and $_.Path -ieq {exe} }}); \
         if ($mine.Count -eq 0) {{ return }}; \
         $mine | ForEach-Object {{ [void]$_.CloseMainWindow() }}; \
         $mine | ForEach-Object {{ [void]$_.WaitForExit(8000) }}; \
         $mine | Where-Object {{ -not $_.HasExited }} | Stop-Process -Force -ErrorAction SilentlyContinue"
    ))
}

/// Remove the install folder once this process has exited, since a running
/// exe can't delete itself.
pub fn remove_dir_after_exit(dir: &Path) -> Result<(), String> {
    let mut command = Command::new("cmd.exe");
    command.args([
        "/C",
        &format!(
            "ping 127.0.0.1 -n 3 >nul & rmdir /s /q \"{}\"",
            dir.display()
        ),
    ]);
    hide_console(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Couldn't remove {} ({e})", dir.display()))
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_: &mut Command) {}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER,
        REG_DWORD, REG_SZ, RRF_RT_ANY,
    };

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    fn check(status: u32, what: &str) -> Result<(), String> {
        if status == 0 {
            Ok(())
        } else {
            Err(format!("Windows refused to save {what} (error {status})"))
        }
    }

    pub(super) fn set_string(key: &str, name: &str, value: &str) -> Result<(), String> {
        let (key, name_w, value) = (wide(key), wide(name), wide(value));
        // SAFETY: NUL-terminated UTF-16 buffers; the size is in bytes and
        // includes the terminator.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name_w.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                (value.len() * 2) as u32,
            )
        };
        check(status, name)
    }

    pub(super) fn set_dword(key: &str, name: &str, value: u32) -> Result<(), String> {
        let (key, name_w) = (wide(key), wide(name));
        // SAFETY: valid strings and a pointer to a 4-byte value.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name_w.as_ptr(),
                REG_DWORD,
                (&value as *const u32).cast(),
                4,
            )
        };
        check(status, name)
    }

    pub(super) fn has_value(key: &str, name: &str) -> bool {
        let (key, name) = (wide(key), wide(name));
        let mut size = 0u32;
        // SAFETY: a null buffer only asks whether the value exists.
        unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_ANY,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            ) == 0
        }
    }

    pub(super) fn delete_value(key: &str, name: &str) {
        let (key, name) = (wide(key), wide(name));
        // SAFETY: valid NUL-terminated strings.
        unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr());
        }
    }

    pub(super) fn delete_key(key: &str) {
        let key = wide(key);
        // SAFETY: a valid NUL-terminated string.
        unsafe {
            RegDeleteTreeW(HKEY_CURRENT_USER, key.as_ptr());
        }
    }
}

#[cfg(not(windows))]
mod registry {
    pub(super) fn set_string(_: &str, _: &str, _: &str) -> Result<(), String> {
        Err("Installing is only supported on Windows".into())
    }
    pub(super) fn set_dword(_: &str, _: &str, _: u32) -> Result<(), String> {
        Err("Installing is only supported on Windows".into())
    }
    pub(super) fn has_value(_: &str, _: &str) -> bool {
        false
    }
    pub(super) fn delete_value(_: &str, _: &str) {}
    pub(super) fn delete_key(_: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn powershell_strings_are_quoted_safely() {
        assert_eq!(quoted(r"C:\Users\O'Neil\x"), r"'C:\Users\O''Neil\x'");
    }

    /// Runs the real PowerShell commands against a temporary folder, so a
    /// broken script fails here instead of in the setup.
    #[cfg(windows)]
    #[test]
    fn shortcuts_are_created_for_real() {
        let folder =
            std::env::temp_dir().join(format!("musikpl-shortcut-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let exe = folder.join("O'Neil App").join(EXE_NAME);
        create_shortcut_in(
            &format!("({})", quoted(&folder.display().to_string())),
            &exe,
        )
        .expect("the shortcut script should run");
        assert!(folder.join("MusiKPL.lnk").exists());
        let _ = std::fs::remove_dir_all(&folder);

        // The real folder expressions must at least parse and resolve.
        for kind in [Shortcut::StartMenu, Shortcut::Desktop] {
            powershell(&format!(
                "if (-not (Test-Path {})) {{ throw 'missing' }}",
                kind.folder()
            ))
            .expect("the folder expression should resolve");
        }
    }

    /// Runs the real script for a path nothing runs from: it must parse and
    /// succeed, and touches no process (they are matched by exact path).
    #[cfg(windows)]
    #[test]
    fn closing_a_copy_that_isnt_running_does_nothing() {
        close_running_script(Path::new(r"C:\musikpl-test-nowhere\MusiKPL.exe"))
            .expect("the script runs");
    }

    #[test]
    fn installs_go_to_local_programs() {
        if let Some(dir) = install_dir() {
            assert!(dir.ends_with(r"Programs\MusiKPL"));
        }
    }
}
