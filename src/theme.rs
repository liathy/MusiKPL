use eframe::egui::Color32;

/// Colors from the MusiKPL Figma design.
#[derive(Clone, Copy)]
pub(crate) struct Theme {
    /// Window background.
    pub(crate) background: Color32,
    /// Cards, drawers and the settings modal (#101010).
    pub(crate) panel: Color32,
    /// Raised surfaces: buttons, search field, tracks, cover placeholders (#222).
    pub(crate) surface: Color32,
    /// Hovered surfaces (#333).
    pub(crate) hover: Color32,
    /// Dividers and outlines (#222).
    pub(crate) border: Color32,
    /// Slider thumb outline (#414141).
    pub(crate) ring: Color32,
    pub(crate) text: Color32,
    /// Secondary text: white at 60 %.
    pub(crate) secondary_text: Color32,
    /// Hints and placeholders: white at 40 %.
    pub(crate) muted_text: Color32,
    /// Icons (#E3E3E3).
    pub(crate) icon: Color32,
    /// Primary buttons on hover (#D6D6D6).
    pub(crate) primary_hover: Color32,
    /// Close-window hover and errors.
    pub(crate) danger: Color32,
    pub(crate) success: Color32,
    pub(crate) warning: Color32,
    /// Liked songs' heart.
    pub(crate) like: Color32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background: Color32::BLACK,
            panel: Color32::from_rgb(16, 16, 16),
            surface: Color32::from_rgb(34, 34, 34),
            hover: Color32::from_rgb(51, 51, 51),
            border: Color32::from_rgb(34, 34, 34),
            ring: Color32::from_rgb(65, 65, 65),
            text: Color32::WHITE,
            secondary_text: Color32::from_white_alpha(153),
            muted_text: Color32::from_white_alpha(102),
            icon: Color32::from_rgb(227, 227, 227),
            primary_hover: Color32::from_rgb(214, 214, 214),
            danger: Color32::from_rgb(229, 72, 77),
            success: Color32::from_rgb(76, 194, 108),
            warning: Color32::from_rgb(232, 176, 64),
            like: Color32::from_rgb(255, 77, 109),
        }
    }
}
