//! rem units for the fixed-size window.
//!
//! The interface is designed on a 1440 × 1024 canvas (the Figma frame). The
//! window keeps that aspect ratio and is sized to fit the screen once at
//! startup; it cannot be resized afterwards. Every size in the UI is written
//! in rem, where 1 rem is 16 design pixels scaled by how large the window is
//! compared to the design, like CSS rem with a root font size that follows
//! the window. A 24 px Figma value is `rem(1.5)`.

use egui::{vec2, Vec2};

/// The design canvas in Figma pixels.
pub(crate) const DESIGN_SIZE: Vec2 = Vec2::new(1440.0, 1024.0);
const DESIGN_REM: f32 = 16.0;
/// Share of the screen's usable area the window covers at most.
const SCREEN_FILL: f32 = 0.9;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ResponsiveUi {
    /// Logical points per rem.
    pub(crate) base_unit: f32,
}

impl ResponsiveUi {
    /// rem units for a window (or canvas) of this size.
    pub(crate) fn from_size(window: Vec2) -> Self {
        let scale = (finite(window.x) / DESIGN_SIZE.x).min(finite(window.y) / DESIGN_SIZE.y);
        Self {
            base_unit: DESIGN_REM * scale.clamp(0.25, 8.0),
        }
    }

    /// `multiplier` rem in egui logical points.
    #[inline]
    pub(crate) fn rem(&self, multiplier: f32) -> f32 {
        finite(multiplier) * self.base_unit
    }

    #[inline]
    pub(crate) fn rem2(&self, x: f32, y: f32) -> Vec2 {
        vec2(self.rem(x), self.rem(y))
    }
}

/// The window size for a screen whose usable area is `work_area` logical
/// points: the largest design-proportioned size that fits comfortably.
pub(crate) fn window_size(work_area: Vec2) -> Vec2 {
    let available = vec2(finite(work_area.x), finite(work_area.y)) * SCREEN_FILL;
    let scale = (available.x / DESIGN_SIZE.x).min(available.y / DESIGN_SIZE.y);
    let size = DESIGN_SIZE * scale;
    vec2(size.x.round().max(1.0), size.y.round().max(1.0))
}

fn finite(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_rem_is_sixteen_design_pixels_at_design_size() {
        let r = ResponsiveUi::from_size(DESIGN_SIZE);
        assert_eq!(r.rem(1.0), 16.0);
        assert_eq!(r.rem(1.5), 24.0);
    }

    #[test]
    fn rem_follows_the_window_size() {
        let small = ResponsiveUi::from_size(DESIGN_SIZE * 0.75);
        let large = ResponsiveUi::from_size(DESIGN_SIZE * 1.5);
        assert!((small.rem(1.0) - 12.0).abs() < 1e-4);
        assert!((large.rem(1.0) - 24.0).abs() < 1e-4);
        // The whole design always fits the window it was scaled for.
        for size in [DESIGN_SIZE * 0.6, DESIGN_SIZE, DESIGN_SIZE * 2.0] {
            let r = ResponsiveUi::from_size(size);
            assert!(r.rem(DESIGN_SIZE.x / 16.0) <= size.x + 0.01);
            assert!(r.rem(DESIGN_SIZE.y / 16.0) <= size.y + 0.01);
        }
    }

    #[test]
    fn invalid_sizes_stay_finite() {
        let r = ResponsiveUi::from_size(vec2(f32::NAN, f32::INFINITY));
        assert!(r.base_unit.is_finite() && r.base_unit > 0.0);
        assert_eq!(r.rem(f32::NAN), 0.0);
    }

    #[test]
    fn window_keeps_the_design_ratio_and_fits_the_screen() {
        for screen in [
            vec2(1366.0, 728.0),
            vec2(1920.0, 1040.0),
            vec2(2560.0, 1400.0),
            vec2(1280.0, 984.0),
            vec2(3840.0, 2120.0),
        ] {
            let size = window_size(screen);
            assert!(
                size.x <= screen.x && size.y <= screen.y,
                "{size:?} on {screen:?}"
            );
            let ratio = size.x / size.y;
            assert!((ratio - DESIGN_SIZE.x / DESIGN_SIZE.y).abs() < 0.01);
        }
        // Larger screens get a larger window, so rem grows with the screen.
        assert!(window_size(vec2(2560.0, 1400.0)).x > window_size(vec2(1920.0, 1040.0)).x);
    }
}
