//! Playlist cover photos: picking one with the Windows file dialog, and
//! keeping a small square copy in `%APPDATA%\MusiKPL\covers`.

use std::path::{Path, PathBuf};

use super::storage;

/// Covers are stored at this size (pixels per side): sharp at any size the
/// app draws them, and small on disk.
const COVER_SIZE: u32 = 512;

fn covers_dir() -> Option<PathBuf> {
    storage::data_dir().map(|dir| dir.join("covers"))
}

/// The image URI egui loads a saved cover from.
pub(crate) fn cover_uri(file_name: &str) -> Option<String> {
    let path = covers_dir()?.join(file_name);
    Some(format!(
        "file://{}",
        path.to_string_lossy().replace('\\', "/")
    ))
}

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

/// Copy `source` into the covers folder as a centered square JPEG. Returns
/// the saved file's name. `stem` makes the name unique to the playlist.
pub(crate) fn save_cover(source: &Path, stem: &str) -> Result<String, String> {
    let image = image::open(source).map_err(|e| format!("Couldn't open that image ({e})"))?;
    let side = image.width().min(image.height());
    if side == 0 {
        return Err("That image is empty".into());
    }
    let square = image.crop_imm(
        (image.width() - side) / 2,
        (image.height() - side) / 2,
        side,
        side,
    );
    let small = square
        .resize_exact(
            COVER_SIZE.min(side),
            COVER_SIZE.min(side),
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8();
    let dir = covers_dir().ok_or("Couldn't find the AppData folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't save the cover ({e})"))?;
    // A new name each time, so the old picture isn't served from cache.
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.as_millis());
    let name = format!("{stem}-{stamp}.jpg");
    small
        .save_with_format(dir.join(&name), image::ImageFormat::Jpeg)
        .map_err(|e| format!("Couldn't save the cover ({e})"))?;
    Ok(name)
}

pub(crate) fn delete_cover(file_name: &str) {
    // Only plain names inside the covers folder.
    if file_name.contains(['/', '\\']) || file_name.contains("..") {
        return;
    }
    if let Some(dir) = covers_dir() {
        let _ = std::fs::remove_file(dir.join(file_name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn covers_are_cropped_square_and_saved_small() {
        let dir = std::env::temp_dir().join(format!("musikpl-cover-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("wide.png");
        image::RgbImage::new(900, 600).save(&source).unwrap();

        let name = save_cover(&source, "playlist-1").expect("saves");
        let saved = covers_dir().unwrap().join(&name);
        let saved_image = image::open(&saved).unwrap();
        assert_eq!((saved_image.width(), saved_image.height()), (512, 512));
        assert!(cover_uri(&name).unwrap().starts_with("file://"));

        delete_cover(&name);
        assert!(!saved.exists());
        delete_cover("../settings.json"); // ignored, not outside the folder
        let _ = std::fs::remove_dir_all(dir);
    }
}
