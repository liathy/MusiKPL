//! Turns `logo.svg` into the program's icons at build time (shared by the
//! app's and the setup's build scripts):
//!
//! - `icon.rgba`: a 256 px picture the app shows as its window and taskbar
//!   icon (`include_bytes!` from `OUT_DIR`), on every system.
//! - On Windows, an `.ico` embedded as resource 1, which Explorer shows as
//!   the exe's icon and the tray icon is loaded from.

use std::{fs, path::Path};

use resvg::{tiny_skia, usvg};

/// Side of the window icon `icon.rgba` holds.
pub const WINDOW_ICON_SIZE: u32 = 256;

/// Render the SVG as a square of `size` pixels.
fn render(svg: &[u8], size: u32) -> tiny_skia::Pixmap {
    use usvg::TreeParsing;
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).expect("logo.svg parses");
    let tree = resvg::Tree::from_usvg(&tree);
    let mut pixmap = tiny_skia::Pixmap::new(size, size).expect("icon size");
    let view = tree.view_box.rect;
    let scale = size as f32 / view.width().max(view.height());
    tree.render(
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
}

/// Straight (not premultiplied) RGBA, as window icons expect.
fn rgba(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}

/// An `.ico` holding PNG images of several sizes.
fn ico(svg: &[u8]) -> Vec<u8> {
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes
        .iter()
        .map(|&size| render(svg, size).encode_png().expect("png"))
        .collect();
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (size, image) in sizes.iter().zip(&images) {
        let side = if *size >= 256 { 0 } else { *size as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        out.extend_from_slice(&image);
    }
    out
}

/// Write `icon.rgba` to `out`, and on Windows targets embed the `.ico`.
pub fn build(logo: &Path, out: &Path) {
    println!("cargo:rerun-if-changed={}", logo.display());
    let svg = fs::read(logo).expect("logo.svg is in the project root");
    fs::write(out.join("icon.rgba"), rgba(&render(&svg, WINDOW_ICON_SIZE)))
        .expect("write icon.rgba");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let ico_path = out.join("app.ico");
        fs::write(&ico_path, ico(&svg)).expect("write app.ico");
        let rc_path = out.join("app.rc");
        let ico = ico_path.display().to_string().replace('\\', "\\\\");
        fs::write(&rc_path, format!("1 ICON \"{ico}\"\n")).expect("write app.rc");
        embed_resource::compile(&rc_path, embed_resource::NONE)
            .manifest_optional()
            .expect("embed the icon");
    }
}
