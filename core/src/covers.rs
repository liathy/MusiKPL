//! Playlist cover photos: a small square copy of the photo the user chose,
//! kept in the data folder's `covers` folder. (Choosing the photo is the
//! platform's: a file dialog on desktop, the photo picker on Android.)

use std::path::{Path, PathBuf};

use crate::storage;

/// Covers are stored at this size (pixels per side): sharp at any size the
/// app draws them, and small on disk.
const COVER_SIZE: u32 = 512;

fn covers_dir() -> Option<PathBuf> {
    storage::data_dir().map(|dir| dir.join("covers"))
}

/// The image URI egui loads a saved cover from.
pub fn cover_uri(file_name: &str) -> Option<String> {
    let path = covers_dir()?.join(file_name);
    Some(format!(
        "file://{}",
        path.to_string_lossy().replace('\\', "/")
    ))
}

/// Copy `source` into the covers folder as a centered square JPEG. Returns
/// the saved file's name. `stem` makes the name unique to the playlist.
pub fn save_cover(source: &Path, stem: &str) -> Result<String, String> {
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

pub fn delete_cover(file_name: &str) {
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
