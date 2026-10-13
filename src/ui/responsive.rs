//! Responsive sizing tokens for the egui UI.
//!
//! `rem` is a project-level convention: one rem-like unit equals `base_unit`
//! egui logical points. It is not a native egui unit.
//!
//! This module deliberately keeps its core calculations pure and independent
//! of `egui::Context`, making them easy to unit-test and reuse in nested UI
//! regions. Create a `ResponsiveUi` from the available size of the region
//! being laid out, then pass it to child UI functions.

use egui::Vec2;

/// Structural layout mode selected from the available region width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutMode {
    Compact,
    Standard,
    Wide,
}

/// Shared responsive sizing tokens for one UI region.
///
/// Values are expressed in egui logical points. Use `rem(multiplier)` for
/// sizes that should scale together, and use available space for regions that
/// should expand rather than scale.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ResponsiveUi {
    pub(crate) mode: LayoutMode,
    pub(crate) base_unit: f32,

    // Typography
    pub(crate) font_caption: f32,
    pub(crate) font_body: f32,
    pub(crate) font_body_emphasis: f32,
    pub(crate) font_heading: f32,
    pub(crate) font_title: f32,

    // Spacing
    pub(crate) space_xs: f32,
    pub(crate) space_sm: f32,
    pub(crate) space_md: f32,
    pub(crate) space_lg: f32,
    pub(crate) space_xl: f32,

    // Controls
    pub(crate) icon_size: f32,
    pub(crate) control_height: f32,
    pub(crate) button_min_size: Vec2,

    // Major layout regions
    pub(crate) sidebar_width: f32,
    pub(crate) queue_width: f32,
    pub(crate) header_height: f32,
    pub(crate) player_footer_height: f32,
    pub(crate) artwork_size: f32,
}

impl ResponsiveUi {
    /// Build responsive tokens from the available size of the UI region.
    ///
    /// The breakpoints and reference dimensions are conservative defaults.
    /// Tune them after checking the application's actual minimum window size
    /// and existing visual design.
    pub(crate) fn from_size(available: Vec2) -> Self {
        let width = finite_non_negative(available.x);
        let height = finite_non_negative(available.y);

        // These boundaries match the existing minimum window width and leave
        // enough room for the main content after optional panels are shown.
        let mode = if width < 820.0 {
            LayoutMode::Compact
        } else if width < 1_120.0 {
            LayoutMode::Standard
        } else {
            LayoutMode::Wide
        };

        // Use both dimensions, but clamp the scale to avoid extreme UI sizes.
        // A very short window should influence spacing conservatively rather
        // than making all text unusably small.
        let width_scale = width / 1_200.0;
        let height_scale = if height > 0.0 { height / 800.0 } else { 0.80 };
        let scale = width_scale.min(height_scale).clamp(0.80, 1.20);
        let base_unit = 16.0 * scale;

        let rem = |multiplier: f32| base_unit * multiplier;

        let (sidebar_width, queue_width) = match mode {
            LayoutMode::Compact => (0.0, 0.0),
            LayoutMode::Standard => (rem(12.875).clamp(144.0, 220.0), 0.0),
            LayoutMode::Wide => (
                rem(12.875).clamp(144.0, 220.0),
                rem(26.75).clamp(240.0, 360.0),
            ),
        };
        let artwork_size = rem(3.0)
            .clamp(40.0, 64.0)
            .min(width * 0.70)
            .min(height * 0.55);

        Self {
            mode,
            base_unit,

            font_caption: rem(0.75).max(9.0),
            font_body: rem(0.875).max(10.0),
            font_body_emphasis: rem(0.9375).max(10.0),
            font_heading: rem(1.25).max(12.0),
            font_title: rem(1.125).clamp(16.0, 20.0),

            space_xs: rem(0.25),
            space_sm: rem(0.5),
            space_md: rem(1.0),
            space_lg: rem(1.5),
            space_xl: rem(2.0),

            icon_size: rem(1.25).clamp(14.0, 28.0),
            control_height: rem(2.5).clamp(32.0, 52.0),
            button_min_size: Vec2::new(rem(2.5).clamp(32.0, 52.0), rem(2.5).clamp(32.0, 52.0)),

            sidebar_width,
            queue_width,
            header_height: rem(4.625).clamp(56.0, 76.0),
            player_footer_height: rem(4.9375).clamp(76.0, 104.0),
            artwork_size,
        }
    }

    /// Convert a rem-like multiplier to egui logical points.
    ///
    /// For example, `rem(1.5)` returns one and a half times `base_unit`.
    #[inline]
    pub(crate) fn rem(&self, multiplier: f32) -> f32 {
        finite_non_negative(multiplier) * self.base_unit
    }

    /// Calculate a rem-like size bounded by explicit logical-point limits.
    ///
    /// The preferred value is `preferred_multiplier * base_unit`, then
    /// constrained to `[min, max]`. Invalid bounds are normalized safely.
    pub(crate) fn clamp_rem(&self, min: f32, preferred_multiplier: f32, max: f32) -> f32 {
        let min = finite_non_negative(min);
        let max = finite_non_negative(max).max(min);
        self.rem(preferred_multiplier).clamp(min, max)
    }
}

fn finite_non_negative(value: f32) -> f32 {
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
    fn rem_uses_base_unit() {
        let responsive = ResponsiveUi::from_size(Vec2::new(1_200.0, 800.0));

        assert_eq!(responsive.rem(1.0), responsive.base_unit);
        assert_eq!(responsive.rem(2.0), responsive.base_unit * 2.0);
    }

    #[test]
    fn clamp_rem_respects_lower_and_upper_bounds() {
        let responsive = ResponsiveUi::from_size(Vec2::new(1_200.0, 800.0));

        assert_eq!(responsive.clamp_rem(20.0, 0.1, 40.0), 20.0);
        assert_eq!(responsive.clamp_rem(20.0, 10.0, 40.0), 40.0);
    }

    #[test]
    fn selects_compact_mode_for_narrow_regions() {
        assert_eq!(
            ResponsiveUi::from_size(Vec2::new(819.0, 700.0)).mode,
            LayoutMode::Compact
        );
    }

    #[test]
    fn selects_standard_mode_for_medium_regions() {
        assert_eq!(
            ResponsiveUi::from_size(Vec2::new(900.0, 700.0)).mode,
            LayoutMode::Standard
        );
    }

    #[test]
    fn selects_wide_mode_for_large_regions() {
        assert_eq!(
            ResponsiveUi::from_size(Vec2::new(1_120.0, 800.0)).mode,
            LayoutMode::Wide
        );
    }

    #[test]
    fn mode_boundaries_are_inclusive_on_the_larger_layout() {
        assert_eq!(
            ResponsiveUi::from_size(Vec2::new(820.0, 700.0)).mode,
            LayoutMode::Standard
        );
        assert_eq!(
            ResponsiveUi::from_size(Vec2::new(1_119.0, 700.0)).mode,
            LayoutMode::Standard
        );
    }

    #[test]
    fn dimensions_are_finite_for_zero_and_invalid_input() {
        let responsive = ResponsiveUi::from_size(Vec2::new(f32::NAN, f32::INFINITY));

        assert!(responsive.base_unit.is_finite());
        assert!(responsive.base_unit >= 0.0);
        assert!(responsive.artwork_size.is_finite());
        assert!(responsive.artwork_size >= 0.0);
        assert!(responsive.sidebar_width.is_finite());
        assert!(responsive.player_footer_height.is_finite());
    }

    #[test]
    fn scale_is_bounded_for_extreme_dimensions() {
        let tiny = ResponsiveUi::from_size(Vec2::ZERO);
        let huge = ResponsiveUi::from_size(Vec2::new(1.0e9, 1.0e9));

        assert!((tiny.base_unit - 12.8).abs() < f32::EPSILON * 16.0);
        assert!(huge.base_unit <= 19.2);
        assert!(huge.artwork_size.is_finite());
    }

    #[test]
    fn artwork_does_not_exceed_region_constraints() {
        let size = Vec2::new(320.0, 240.0);
        let responsive = ResponsiveUi::from_size(size);

        assert!(responsive.artwork_size <= size.x * 0.70);
        assert!(responsive.artwork_size <= size.y * 0.55);
    }

    #[test]
    fn clamp_rem_normalizes_reversed_bounds() {
        let responsive = ResponsiveUi::from_size(Vec2::new(1_200.0, 800.0));

        assert_eq!(responsive.clamp_rem(40.0, 1.0, 20.0), 40.0);
    }
}
