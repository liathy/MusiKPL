use eframe::egui::{self, Color32, Rect, SizeHint, TextureHandle};

macro_rules! icon_files {
    ($($field:ident => $file:literal),* $(,)?) => {
        /// The design's icons, rasterized once from the bundled SVGs. They are
        /// drawn in white and tinted when painted.
        #[derive(Clone)]
        pub(crate) struct IconTextures {
            $(pub(crate) $field: TextureHandle,)*
        }

        impl IconTextures {
            pub(crate) fn load(ctx: &egui::Context) -> Result<Self, String> {
                Ok(Self {
                    $($field: load(ctx, $file, include_bytes!(concat!("../assets/icons/", $file)))?,)*
                })
            }
        }
    };
}

icon_files! {
    music => "music.svg",
    play => "play.svg",
    pause => "pause.svg",
    previous => "previous.svg",
    next => "next.svg",
    menu => "menu.svg",
    queue => "queue.svg",
    settings => "settings.svg",
    close => "close.svg",
    minimize => "minimize.svg",
    add => "add.svg",
    added => "added.svg",
    check_box => "check_box.svg",
    check_box_off => "check_box_off.svg",
    home => "home.svg",
    heart => "heart.svg",
    heart_filled => "heart_filled.svg",
    refresh => "refresh.svg",
    history => "history.svg",
    chevron => "chevron.svg",
    edit => "edit.svg",
    playlist => "playlist.svg",
    playlist_add => "playlist_add.svg",
    volume_off => "volume_off.svg",
    volume_low => "volume_low.svg",
    volume_high => "volume_high.svg",
    maximize => "maximize.svg",
    restore => "restore.svg",
    friends => "friends.svg",
    artist => "artist.svg",
    back => "back.svg",
    forward => "forward.svg",
}

fn load(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Result<TextureHandle, String> {
    // Rasterize above display size so icons stay sharp at any rem scale, and
    // keep mipmaps so shrinking them to button size stays smooth.
    egui_extras::image::load_svg_bytes_with_size(bytes, Some(SizeHint::Height(128)))
        .map(|image| ctx.load_texture(name, image, SMOOTH))
}

/// Linear filtering with mipmaps: textures drawn much smaller than their
/// pixel size (icons, cover thumbnails) keep clean edges.
pub(crate) const SMOOTH: egui::TextureOptions = egui::TextureOptions {
    mipmap_mode: Some(egui::TextureFilter::Linear),
    ..egui::TextureOptions::LINEAR
};

/// Paint `texture` centered in `rect`, as large as fits without distortion.
pub(crate) fn paint_icon(
    painter: &egui::Painter,
    texture: &TextureHandle,
    rect: Rect,
    tint: Color32,
) {
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / size.x).min(rect.height() / size.y);
    let target = Rect::from_center_size(rect.center(), size * scale);
    painter.image(
        texture.id(),
        target,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        tint,
    );
}
