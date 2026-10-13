use crate::theme::Theme;
use eframe::egui;

const MUSIC_ICON: &[u8] =
    include_bytes!("../assets/icons/music_note_2_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const PLAY_ICON: &[u8] =
    include_bytes!("../assets/icons/play_arrow_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const PAUSE_ICON: &[u8] =
    include_bytes!("../assets/icons/pause_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const PREVIOUS_ICON: &[u8] =
    include_bytes!("../assets/icons/skip_previous_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");
const NEXT_ICON: &[u8] =
    include_bytes!("../assets/icons/skip_next_24dp_E3E3E3_FILL0_wght400_GRAD0_opsz24.svg");

#[derive(Clone)]
pub(crate) struct IconTextures {
    pub(crate) music: egui::TextureHandle,
    pub(crate) play: egui::TextureHandle,
    pub(crate) pause: egui::TextureHandle,
    pub(crate) previous: egui::TextureHandle,
    pub(crate) next: egui::TextureHandle,
}

impl IconTextures {
    pub(crate) fn load(ctx: &egui::Context) -> Result<Self, String> {
        let load = |name: &str, bytes: &[u8]| {
            egui_extras::image::load_svg_bytes(bytes)
                .map(|image| ctx.load_texture(name, image, egui::TextureOptions::LINEAR))
        };
        Ok(Self {
            music: load("musikpl-music", MUSIC_ICON)?,
            play: load("musikpl-play", PLAY_ICON)?,
            pause: load("musikpl-pause", PAUSE_ICON)?,
            previous: load("musikpl-previous", PREVIOUS_ICON)?,
            next: load("musikpl-next", NEXT_ICON)?,
        })
    }
}

pub(crate) fn design_icon(texture: &egui::TextureHandle, size: f32) -> egui::Image<'static> {
    egui::Image::from_texture((texture.id(), egui::vec2(size, size)))
        .fit_to_exact_size(egui::vec2(size, size))
        .tint(Theme::default().accent)
}

pub(crate) fn icon_button(
    ui: &mut egui::Ui,
    texture: &egui::TextureHandle,
    size: f32,
    button_size: egui::Vec2,
    tooltip: &str,
) -> egui::Response {
    ui.add_sized(button_size, egui::Button::image(design_icon(texture, size)))
        .on_hover_text(tooltip)
}
