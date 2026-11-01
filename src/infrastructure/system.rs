//! Small operating-system integrations: the screen's usable area, opening a
//! link in the default browser, starting with Windows, the computer's
//! network adapters, and the Windows Firewall rule LAN Jams need.

use std::process::Command;

use super::process::hide_console;

/// Allow this MusiKPL.exe through Windows Firewall for incoming connections
/// on every network type (Private and Public), which LAN Jams need on both
/// the hosting and the joining PC. Windows asks for administrator approval.
/// Blocks until the rule is added or the request is declined.
#[cfg(windows)]
pub(crate) fn allow_through_firewall() -> Result<(), String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, GetLastError},
        System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE},
        UI::Shell::{
            ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE, SHELLEXECUTEINFOW,
        },
    };
    const ERROR_CANCELLED: u32 = 1223;

    let exe = std::env::current_exe().map_err(|e| format!("Could not find MusiKPL.exe: {e}"))?;
    let exe = exe.display().to_string();
    if exe.contains('"') {
        return Err("MusiKPL's folder name can't be used in a firewall rule".into());
    }
    // Replace this copy's earlier rule, if any, then add one for all
    // profiles. Rules for other copies of MusiKPL are left alone.
    let parameters = format!(
        "/s /c \"netsh advfirewall firewall delete rule name=\"MusiKPL\" program=\"{exe}\" >nul 2>&1 & \
         netsh advfirewall firewall add rule name=\"MusiKPL\" dir=in action=allow program=\"{exe}\" enable=yes profile=any\""
    );
    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (verb, file, parameters) = (wide("runas"), wide("cmd.exe"), wide(&parameters));

    // SAFETY: zero is a valid initial state for this plain-data structure.
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = parameters.as_ptr();
    info.nShow = 0; // SW_HIDE
                    // SAFETY: the strings outlive the call and `info` is fully initialized.
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        // SAFETY: no arguments.
        return Err(match unsafe { GetLastError() } {
            ERROR_CANCELLED => "Windows Firewall was not changed (the request was declined)".into(),
            code => format!("Could not ask Windows for permission (error {code})"),
        });
    }
    if info.hProcess == 0 {
        return Ok(());
    }
    let mut exit_code = 1u32;
    // SAFETY: `hProcess` is a valid process handle owned by us until closed.
    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        GetExitCodeProcess(info.hProcess, &mut exit_code);
        CloseHandle(info.hProcess);
    }
    if exit_code == 0 {
        Ok(())
    } else {
        Err(format!(
            "Windows Firewall refused the change (netsh exit code {exit_code})"
        ))
    }
}

#[cfg(not(windows))]
pub(crate) fn allow_through_firewall() -> Result<(), String> {
    Err("Changing the firewall is only supported on Windows".into())
}

/// The desktop's usable area (without the taskbar) in logical points:
/// `[left, top, width, height]`.
#[cfg(windows)]
pub(crate) fn work_area() -> Option<[f32; 4]> {
    use windows_sys::Win32::{
        Foundation::RECT,
        UI::{
            HiDpi::GetDpiForSystem,
            WindowsAndMessaging::{SystemParametersInfoW, SPI_GETWORKAREA},
        },
    };

    let mut area = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: SPI_GETWORKAREA writes one RECT into the pointer it is given.
    let ok =
        unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut area as *mut RECT).cast(), 0) };
    if ok == 0 {
        return None;
    }
    // SAFETY: no arguments; returns the system DPI.
    let scale = unsafe { GetDpiForSystem() }.max(96) as f32 / 96.0;
    let [left, top] = [area.left as f32 / scale, area.top as f32 / scale];
    let width = (area.right - area.left) as f32 / scale;
    let height = (area.bottom - area.top) as f32 / scale;
    (width > 0.0 && height > 0.0).then_some([left, top, width, height])
}

#[cfg(not(windows))]
pub(crate) fn work_area() -> Option<[f32; 4]> {
    None
}

/// Open a web page in the default browser.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened".into());
    }
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("rundll32.exe");
        command.args(["url.dll,FileProtocolHandler", url]);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(url);
        command
    };
    hide_console(&mut command);
    command
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open the browser: {e}"))
}

#[cfg(windows)]
mod autostart {
    use windows_sys::Win32::System::Registry::{
        RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "MusiKPL";
    const ERROR_FILE_NOT_FOUND: u32 = 2;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub(super) fn enabled() -> bool {
        let (key, name) = (wide(RUN_KEY), wide(VALUE_NAME));
        let mut size = 0u32;
        // SAFETY: the strings are NUL-terminated; a null buffer only asks
        // whether the value exists and how large it is.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        status == 0
    }

    pub(super) fn set(enabled: bool) -> Result<(), String> {
        let (key, name) = (wide(RUN_KEY), wide(VALUE_NAME));
        let status = if enabled {
            let exe =
                std::env::current_exe().map_err(|e| format!("Could not find MusiKPL.exe: {e}"))?;
            let command = wide(&format!("\"{}\"", exe.display()));
            // SAFETY: all pointers are valid NUL-terminated UTF-16 buffers and
            // the size is the byte length of `command`, terminator included.
            unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    name.as_ptr(),
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: valid NUL-terminated UTF-16 strings.
            match unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => 0,
                status => status,
            }
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!(
                "Windows refused to change the startup setting (error {status})"
            ))
        }
    }
}

/// Whether MusiKPL is registered to start when the user signs in.
pub(crate) fn starts_with_windows() -> bool {
    #[cfg(windows)]
    {
        autostart::enabled()
    }
    #[cfg(not(windows))]
    {
        linux_autostart::file().is_some_and(|file| file.is_file())
    }
}

pub(crate) fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        autostart::set(enabled)
    }
    #[cfg(not(windows))]
    {
        linux_autostart::set(enabled)
    }
}

/// Linux desktops start the programs listed in ~/.config/autostart.
#[cfg(not(windows))]
mod linux_autostart {
    use std::path::PathBuf;

    pub(super) fn file() -> Option<PathBuf> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(config.join("autostart").join("musikpl.desktop"))
    }

    pub(super) fn set(enabled: bool) -> Result<(), String> {
        let file = file().ok_or("Couldn't find your config folder")?;
        if !enabled {
            return match std::fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(format!("Couldn't turn off starting on login ({error})"))
                }
                _ => Ok(()),
            };
        }
        let exe = std::env::current_exe().map_err(|e| format!("Couldn't find MusiKPL ({e})"))?;
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=MusiKPL\nComment=Music player\nExec=\"{}\"\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            exe.display()
        );
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder)
                .map_err(|e| format!("Couldn't turn on starting on login ({e})"))?;
        }
        std::fs::write(&file, entry)
            .map_err(|e| format!("Couldn't turn on starting on login ({e})"))
    }
}
