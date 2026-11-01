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

/// The square of a photo to keep, as fractions of its size: `x` and `y`
/// from the left and top, `side` of the shorter side (1 = all of it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub x: f32,
    pub y: f32,
    pub side: f32,
}

/// Copy `source` into the covers folder as a centered square JPEG. Returns
/// the saved file's name. `stem` makes the name unique to the playlist.
pub fn save_cover(source: &Path, stem: &str) -> Result<String, String> {
    save_cover_cropped(source, stem, None)
}

/// Like [`save_cover`], keeping the square the user chose (the centered
/// one without a crop).
pub fn save_cover_cropped(source: &Path, stem: &str, crop: Option<Crop>) -> Result<String, String> {
    // The format is read from the file, not its name (a picked photo may
    // come without an extension).
    let image = image::ImageReader::open(source)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| format!("Couldn't open that image ({e})"))?
        .decode()
        .map_err(|e| format!("Couldn't read that image ({e})"))?;
    let shorter = image.width().min(image.height());
    if shorter == 0 {
        return Err("That image is empty".into());
    }
    let (x, y, side) = match crop {
        Some(crop) => {
            let side = ((crop.side.clamp(0.01, 1.0) * shorter as f32) as u32).clamp(1, shorter);
            let x =
                ((crop.x.clamp(0.0, 1.0) * image.width() as f32) as u32).min(image.width() - side);
            let y = ((crop.y.clamp(0.0, 1.0) * image.height() as f32) as u32)
                .min(image.height() - side);
            (x, y, side)
        }
        None => (
            (image.width() - shorter) / 2,
            (image.height() - shorter) / 2,
            shorter,
        ),
    };
    let square = image.crop_imm(x, y, side, side);
    let side = square.width();
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

/// Only plain names inside the covers folder.
fn safe_name(file_name: &str) -> bool {
    !file_name.is_empty() && !file_name.contains(['/', '\\']) && !file_name.contains("..")
}

/// A saved cover's bytes (to send to the user's other devices).
pub fn read_cover(file_name: &str) -> Option<Vec<u8>> {
    if !safe_name(file_name) {
        return None;
    }
    std::fs::read(covers_dir()?.join(file_name)).ok()
}

/// Save a cover that came from another of the user's devices. Only images
/// are kept.
pub fn write_cover(file_name: &str, bytes: &[u8]) {
    if !safe_name(file_name) || bytes.len() > 4 * 1024 * 1024 || image::guess_format(bytes).is_err()
    {
        return;
    }
    if let Some(dir) = covers_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(file_name), bytes);
    }
}

pub fn delete_cover(file_name: &str) {
    if !safe_name(file_name) {
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

    #[test]
    fn a_chosen_square_is_kept_and_names_dont_matter() {
        let dir = std::env::temp_dir().join(format!("musikpl-crop-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Left half red, right half blue; saved without an extension.
        let mut picture = image::RgbImage::new(400, 200);
        for (x, _, pixel) in picture.enumerate_pixels_mut() {
            *pixel = if x < 200 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            };
        }
        let png = dir.join("photo.png");
        picture.save(&png).unwrap();
        let source = dir.join("picked-photo");
        std::fs::rename(&png, &source).unwrap();

        let crop = Crop {
            x: 0.5,
            y: 0.0,
            side: 1.0,
        };
        let name = save_cover_cropped(&source, "playlist-2", Some(crop)).expect("saves");
        let saved = image::open(covers_dir().unwrap().join(&name))
            .unwrap()
            .to_rgb8();
        assert_eq!(saved.width(), 200);
        let middle = saved.get_pixel(100, 100);
        assert!(middle[2] > 200 && middle[0] < 60, "the blue half was kept");
        delete_cover(&name);
        let _ = std::fs::remove_dir_all(dir);
    }
}
