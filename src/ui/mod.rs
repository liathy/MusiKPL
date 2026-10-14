pub(crate) mod footer;
pub(crate) mod icons;
pub(crate) mod layout;
pub(crate) mod responsive;
pub(crate) mod screens;

use eframe::egui;

/// Make egui's default text styles follow the responsive typography tokens so
/// plain buttons, labels, and text fields match explicitly sized text.
pub(crate) fn apply_text_sizes(ui: &mut egui::Ui, responsive: &responsive::ResponsiveUi) {
    let styles = &mut ui.style_mut().text_styles;
    for style in [egui::TextStyle::Body, egui::TextStyle::Button] {
        styles.insert(style, egui::FontId::proportional(responsive.font_body));
    }
    styles.insert(
        egui::TextStyle::Small,
        egui::FontId::proportional(responsive.font_caption),
    );
}
