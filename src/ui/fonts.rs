//! The design's typefaces: Baloo Thambi for titles and headings, Baloo
//! Thambi 2 for everything else. Both are SIL Open Font License fonts from
//! Google Fonts, bundled into the executable.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};

const DISPLAY: &str = "display";
const BALOO_THAMBI: &[u8] = include_bytes!("../assets/fonts/BalooThambi-Regular.ttf");
const BALOO_THAMBI_2: &[u8] = include_bytes!("../assets/fonts/BalooThambi2-Regular.ttf");

pub(crate) fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("baloo-thambi".into(), FontData::from_static(BALOO_THAMBI));
    fonts.font_data.insert(
        "baloo-thambi-2".into(),
        FontData::from_static(BALOO_THAMBI_2),
    );

    // egui's bundled fonts stay as fallbacks for symbols Baloo lacks.
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut body = vec!["baloo-thambi-2".to_owned()];
    body.extend(fallbacks.iter().cloned());
    let mut display = vec!["baloo-thambi".to_owned(), "baloo-thambi-2".to_owned()];
    display.extend(fallbacks);
    fonts.families.insert(FontFamily::Proportional, body);
    fonts
        .families
        .insert(FontFamily::Name(DISPLAY.into()), display);
    ctx.set_fonts(fonts);
}

/// Baloo Thambi: song titles, headings, and large labels.
pub(crate) fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Baloo Thambi 2: body text, captions, and inputs.
pub(crate) fn body(size: f32) -> FontId {
    FontId::proportional(size)
}
