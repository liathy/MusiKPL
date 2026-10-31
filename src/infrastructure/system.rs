//! Small operating-system integrations: the screen's usable area, opening a
//! link in the default browser, and starting with Windows.

use std::process::Command;

use super::process::hide_console;

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
        false
    }
}

pub(crate) fn set_start_with_windows(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        autostart::set(enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("Starting with the system is only supported on Windows".into())
    }
}
