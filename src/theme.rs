use eframe::egui;

#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) background: egui::Color32,
    pub(crate) header: egui::Color32,
    pub(crate) surface: egui::Color32,
    pub(crate) raised: egui::Color32,
    pub(crate) hover: egui::Color32,
    pub(crate) border: egui::Color32,
    pub(crate) text: egui::Color32,
    pub(crate) secondary_text: egui::Color32,
    pub(crate) muted_text: egui::Color32,
    pub(crate) accent: egui::Color32,
    pub(crate) spacing: f32,
    pub(crate) radius: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: egui::Color32::from_rgb(0, 0, 0),
            header: egui::Color32::from_rgb(0, 0, 0),
            surface: egui::Color32::from_rgb(0, 0, 0),
            raised: egui::Color32::from_rgb(0, 0, 0),
            hover: egui::Color32::from_rgb(20, 20, 20),
            border: egui::Color32::from_rgb(40, 40, 40),
            text: egui::Color32::from_rgb(245, 245, 245),
            secondary_text: egui::Color32::from_rgb(160, 160, 160),
            muted_text: egui::Color32::from_rgb(112, 112, 112),
            accent: egui::Color32::from_rgb(173, 0, 255),
            spacing: 8.0,
            radius: 8.0,
        }
    }
}
