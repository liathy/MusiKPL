//! Choosing a playlist cover photo with the system's file dialog. Saving
//! it is the core's (`musikpl_core::covers`).

use std::path::PathBuf;

/// Ask the user for a photo. Blocks until the dialog closes, so call it on
/// a background thread. `None` when cancelled.
pub(crate) fn pick_image() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Controls::Dialogs::{
            GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_PATHMUSTEXIST, OPENFILENAMEW,
        };
        let filter: Vec<u16> = "Images (*.jpg, *.png, *.webp)\0*.jpg;*.jpeg;*.png;*.webp\0\0"
            .encode_utf16()
            .collect();
        let title: Vec<u16> = "Choose a playlist cover\0".encode_utf16().collect();
        let mut file = vec![0u16; 1024];
        // SAFETY: a zeroed OPENFILENAMEW is valid; every pointer set below
        // points to a buffer that outlives the call.
        let mut options: OPENFILENAMEW = unsafe { std::mem::zeroed() };
        options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        options.lpstrFilter = filter.as_ptr();
        options.lpstrFile = file.as_mut_ptr();
        options.nMaxFile = file.len() as u32;
        options.lpstrTitle = title.as_ptr();
        options.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY;
        // SAFETY: `options` is fully initialized as described above.
        if unsafe { GetOpenFileNameW(&mut options) } == 0 {
            return None;
        }
        let length = file.iter().position(|&c| c == 0).unwrap_or(file.len());
        Some(PathBuf::from(String::from_utf16_lossy(&file[..length])))
    }
    // Linux: the desktop's own file picker, GNOME's zenity or KDE's kdialog.
    #[cfg(not(windows))]
    {
        use std::process::Command;
        let pickers: [(&str, &[&str]); 2] = [
            (
                "zenity",
                &[
                    "--file-selection",
                    "--title=Choose a playlist cover",
                    "--file-filter=Images | *.jpg *.jpeg *.png *.webp",
                ],
            ),
            (
                "kdialog",
                &[
                    "--getopenfilename",
                    ".",
                    "Images (*.jpg *.jpeg *.png *.webp)",
                ],
            ),
        ];
        for (program, args) in pickers {
            match Command::new(program).args(args).output() {
                Ok(output) => {
                    let chosen = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                    // Cancelled, or chose nothing.
                    return (output.status.success() && !chosen.is_empty())
                        .then(|| PathBuf::from(chosen));
                }
                Err(_) => continue, // Not installed: try the next one.
            }
        }
        None
    }
}
