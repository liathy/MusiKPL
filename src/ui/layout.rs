//! Window geometry from the Figma frame, in rem.

use eframe::egui::{pos2, vec2, Pos2, Rect, Vec2};

use crate::ui::responsive::ResponsiveUi;

/// The three bands of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowLayout {
    pub(crate) top_bar: Rect,
    pub(crate) main: Rect,
    pub(crate) footer: Rect,
}

pub(crate) fn window_layout(window: Rect, r: &ResponsiveUi) -> WindowLayout {
    let top = (window.top() + r.rem(3.75)).min(window.bottom());
    let footer_top = (window.bottom() - r.rem(6.5)).max(top);
    WindowLayout {
        top_bar: Rect::from_min_max(window.min, pos2(window.right(), top)),
        main: Rect::from_min_max(pos2(window.left(), top), pos2(window.right(), footer_top)),
        footer: Rect::from_min_max(pos2(window.left(), footer_top), window.max),
    }
}

/// The footer's three equal columns: track info, playback, volume.
pub(crate) fn footer_columns(footer: Rect) -> [Rect; 3] {
    let third = footer.width() / 3.0;
    [0.0, 1.0, 2.0].map(|index| {
        Rect::from_min_size(
            pos2(footer.left() + third * index, footer.top()),
            vec2(third, footer.height()),
        )
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TransportButtons {
    pub(crate) previous: Rect,
    pub(crate) play: Rect,
    pub(crate) next: Rect,
}

/// Previous, play and next around `center`, `gap` apart.
pub(crate) fn transport_buttons(center: Pos2, size: Vec2, gap: f32) -> TransportButtons {
    let step = size.x + gap;
    TransportButtons {
        previous: Rect::from_center_size(pos2(center.x - step, center.y), size),
        play: Rect::from_center_size(center, size),
        next: Rect::from_center_size(pos2(center.x + step, center.y), size),
    }
}

/// How many fixed-width cards fit in a row.
pub(crate) fn grid_columns(available: f32, card: f32, gap: f32) -> usize {
    if card <= 0.0 {
        return 1;
    }
    (((available + gap) / (card + gap)).floor() as usize).max(1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Left,
    Right,
}

/// The settings modal: the whole window less a 2.5 rem margin.
pub(crate) fn modal_rect(window: Rect, r: &ResponsiveUi) -> Rect {
    window.shrink(r.rem(2.5))
}

/// The window's edges and corners, where dragging resizes it (the app
/// draws its own frame, so Windows provides no resize border). Returns the
/// side being dragged, if a drag started.
pub(crate) fn resize_edges(
    ui: &mut eframe::egui::Ui,
    window: Rect,
) -> Option<eframe::egui::ResizeDirection> {
    use eframe::egui::{CursorIcon, Id, ResizeDirection, Sense};
    // Thin enough not to get in the way of the window's own controls.
    let edge = 5.0;
    let corner = 12.0;
    let (l, t, r, b) = (window.left(), window.top(), window.right(), window.bottom());
    let handles = [
        (
            Rect::from_min_max(pos2(l, t), pos2(l + corner, t + corner)),
            ResizeDirection::NorthWest,
            CursorIcon::ResizeNorthWest,
        ),
        (
            Rect::from_min_max(pos2(r - corner, t), pos2(r, t + corner)),
            ResizeDirection::NorthEast,
            CursorIcon::ResizeNorthEast,
        ),
        (
            Rect::from_min_max(pos2(l, b - corner), pos2(l + corner, b)),
            ResizeDirection::SouthWest,
            CursorIcon::ResizeSouthWest,
        ),
        (
            Rect::from_min_max(pos2(r - corner, b - corner), pos2(r, b)),
            ResizeDirection::SouthEast,
            CursorIcon::ResizeSouthEast,
        ),
        (
            Rect::from_min_max(pos2(l + corner, t), pos2(r - corner, t + edge)),
            ResizeDirection::North,
            CursorIcon::ResizeNorth,
        ),
        (
            Rect::from_min_max(pos2(l + corner, b - edge), pos2(r - corner, b)),
            ResizeDirection::South,
            CursorIcon::ResizeSouth,
        ),
        (
            Rect::from_min_max(pos2(l, t + corner), pos2(l + edge, b - corner)),
            ResizeDirection::West,
            CursorIcon::ResizeWest,
        ),
        (
            Rect::from_min_max(pos2(r - edge, t + corner), pos2(r, b - corner)),
            ResizeDirection::East,
            CursorIcon::ResizeEast,
        ),
    ];
    let mut started = None;
    for (index, (rect, direction, cursor)) in handles.into_iter().enumerate() {
        let handle = ui
            .interact(rect, Id::new(("window_resize", index)), Sense::drag())
            .on_hover_cursor(cursor);
        if handle.drag_started() {
            started = Some(direction);
        }
    }
    started
}

/// Smallest window that keeps the layout intact.
pub(crate) const MIN_WINDOW: Vec2 = Vec2::new(960.0, 680.0);
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::responsive::DESIGN_SIZE;

    fn design() -> (Rect, ResponsiveUi) {
        (
            Rect::from_min_size(Pos2::ZERO, DESIGN_SIZE),
            ResponsiveUi::from_size(DESIGN_SIZE),
        )
    }

    #[test]
    fn bands_match_the_figma_frame() {
        let (window, r) = design();
        let layout = window_layout(window, &r);
        assert_eq!(layout.top_bar.height(), 60.0);
        assert_eq!(layout.main.height(), 860.0);
        assert_eq!(layout.footer.height(), 104.0);
        assert_eq!(layout.footer.bottom(), window.bottom());
    }

    #[test]
    fn bands_scale_together_with_the_window() {
        let window = Rect::from_min_size(Pos2::ZERO, DESIGN_SIZE * 0.75);
        let r = ResponsiveUi::from_size(window.size());
        let layout = window_layout(window, &r);
        assert!((layout.top_bar.height() - 45.0).abs() < 0.01);
        assert!((layout.footer.height() - 78.0).abs() < 0.01);
        let total = layout.top_bar.height() + layout.main.height() + layout.footer.height();
        assert!((total - window.height()).abs() < 0.01);
    }

    #[test]
    fn footer_columns_are_equal_and_cover_the_footer() {
        let footer = Rect::from_min_size(pos2(0.0, 854.0), vec2(1440.0, 170.0));
        let [left, middle, right] = footer_columns(footer);
        assert_eq!(left.width(), middle.width());
        assert_eq!(middle.width(), right.width());
        assert_eq!(left.left(), footer.left());
        assert!((right.right() - footer.right()).abs() < 0.001);
        assert_eq!(middle.center().x, footer.center().x);
    }

    #[test]
    fn transport_buttons_are_centered_and_evenly_spaced() {
        let center = pos2(720.0, 900.0);
        let buttons = transport_buttons(center, vec2(40.0, 40.0), 8.0);
        assert_eq!(buttons.play.center(), center);
        assert_eq!(buttons.play.left() - buttons.previous.right(), 8.0);
        assert_eq!(buttons.next.left() - buttons.play.right(), 8.0);
        assert_eq!(buttons.previous.center().y, center.y);
    }

    #[test]
    fn grid_fits_seven_cards_in_the_design() {
        assert_eq!(grid_columns(1392.0, 183.0, 16.0), 7);
        assert_eq!(grid_columns(100.0, 183.0, 16.0), 1);
        assert_eq!(grid_columns(500.0, 0.0, 16.0), 1);
    }

    #[test]
    fn the_settings_modal_keeps_a_margin() {
        let (window, r) = design();
        assert_eq!(modal_rect(window, &r).min, pos2(40.0, 40.0));
    }
}
