use eframe::egui;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResponsiveLayout {
    pub(crate) header_height: f32,
    pub(crate) footer_height: f32,
    pub(crate) body_height: f32,
    pub(crate) sidebar_width: f32,
    pub(crate) queue_width: f32,
    pub(crate) main_width: f32,
    pub(crate) show_sidebar: bool,
    pub(crate) show_queue: bool,
}

pub(crate) fn responsive_layout(width: f32, height: f32, unit: f32) -> ResponsiveLayout {
    // Secondary panels collapse from the current content width so resizing and
    // DPI scaling cannot squeeze the main content into a negative rectangle.
    let header_height = (4.625 * unit).clamp(56.0, 76.0);
    let footer_height = (4.9375 * unit).clamp(76.0, 104.0);
    let show_sidebar = width >= 820.0;
    let show_queue = width >= 1120.0;
    let sidebar_width = if show_sidebar {
        (12.875 * unit).clamp(144.0, 220.0)
    } else {
        0.0
    };
    let queue_width = if show_queue {
        (26.75 * unit).clamp(240.0, 360.0)
    } else {
        0.0
    };
    let body_height = (height - header_height - footer_height).max(1.0);
    let main_width = (width - sidebar_width - queue_width).max(1.0);

    ResponsiveLayout {
        header_height,
        footer_height,
        body_height,
        sidebar_width,
        queue_width,
        main_width,
        show_sidebar,
        show_queue,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FooterColumns {
    pub(crate) left: f32,
    pub(crate) center: f32,
    pub(crate) right: f32,
}

pub(crate) fn footer_columns(width: f32) -> FooterColumns {
    // Compact layout: prioritize playback when the window is too narrow for
    // track details and volume to fit comfortably beside it.
    if width < 720.0 {
        return FooterColumns {
            left: 0.0,
            center: width,
            right: 0.0,
        };
    }

    // Keep the outer columns equal. This is important: equal left/right widths
    // mean the playback column's center stays at the center of the whole window,
    // just like a centered grid column in a CSS layout.
    let side = if width < 900.0 {
        (width * 0.22).clamp(120.0, 180.0)
    } else {
        (width * 0.25).clamp(180.0, 480.0)
    };

    FooterColumns {
        left: side,
        center: (width - side * 2.0).max(0.0),
        right: side,
    }
}

pub(crate) fn footer_padding(width: f32) -> f32 {
    (width * 0.02).clamp(8.0, 24.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FooterGeometry {
    pub(crate) left: egui::Rect,
    pub(crate) playback: egui::Rect,
    pub(crate) right: egui::Rect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FooterPlaybackRows {
    pub(crate) transport: egui::Rect,
    pub(crate) progress: egui::Rect,
}

pub(crate) fn footer_playback_rows(playback_rect: egui::Rect) -> FooterPlaybackRows {
    const TRANSPORT_HEIGHT: f32 = 30.0;
    const PROGRESS_HEIGHT: f32 = 20.0;
    const ROW_GAP: f32 = 10.0;

    // The playback rectangle is a full-width positioning reference. The
    // widgets themselves remain in compact rectangles so they cannot cover
    // the track details or volume controls.
    let transport_width = 120.0_f32.min(playback_rect.width());
    // Use the actual middle column width so the progress row fills the space
    // between the side sections without changing its centered position.
    let footer_width = playback_rect.width().max(0.0);
    let progress_width = footer_columns(footer_width).center.min(footer_width);

    // Treat the buttons and seek bar as one centered vertical control group.
    // Both rows use the footer midpoint as their horizontal midpoint.
    let content_height = TRANSPORT_HEIGHT + ROW_GAP + PROGRESS_HEIGHT;
    let content_top = playback_rect.center().y - content_height / 2.0;
    let content_top = content_top.max(playback_rect.top());
    let transport_top = content_top;
    let progress_top = transport_top + TRANSPORT_HEIGHT + ROW_GAP;
    let progress_top = progress_top.min(playback_rect.bottom() - PROGRESS_HEIGHT);

    let transport_bottom = transport_top + TRANSPORT_HEIGHT;
    debug_assert!((progress_top - transport_bottom - ROW_GAP).abs() < 0.1);

    FooterPlaybackRows {
        transport: egui::Rect::from_center_size(
            egui::pos2(
                playback_rect.center().x,
                transport_top + TRANSPORT_HEIGHT / 2.0,
            ),
            egui::vec2(transport_width, TRANSPORT_HEIGHT),
        ),
        progress: egui::Rect::from_center_size(
            egui::pos2(
                playback_rect.center().x,
                progress_top + PROGRESS_HEIGHT / 2.0,
            ),
            egui::vec2(progress_width, PROGRESS_HEIGHT),
        ),
    }
}

pub(crate) fn footer_geometry(footer_rect: egui::Rect) -> FooterGeometry {
    let columns = footer_columns(footer_rect.width());
    FooterGeometry {
        left: egui::Rect::from_min_size(
            footer_rect.min,
            egui::vec2(columns.left, footer_rect.height()),
        ),
        playback: egui::Rect::from_center_size(
            footer_rect.center(),
            egui::vec2(footer_rect.width(), footer_rect.height()),
        ),
        right: egui::Rect::from_min_size(
            egui::pos2(footer_rect.right() - columns.right, footer_rect.top()),
            egui::vec2(columns.right, footer_rect.height()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responsive_layout_collapses_secondary_panels_at_narrow_widths() {
        let compact = responsive_layout(800.0, 600.0, 12.0);
        assert!(!compact.show_sidebar);
        assert!(!compact.show_queue);
        assert_eq!(compact.main_width, 800.0);

        let desktop = responsive_layout(1280.0, 720.0, 12.0);
        assert!(desktop.show_sidebar);
        assert!(desktop.show_queue);
        assert!(desktop.main_width > 0.0);
        assert!(desktop.body_height > 0.0);
    }

    #[test]
    fn responsive_layout_fills_remaining_height_without_forcing_overflow() {
        for (width, height) in [
            (800.0, 600.0),
            (1280.0, 720.0),
            (1920.0, 1080.0),
            (1600.0, 600.0),
            (700.0, 1000.0),
        ] {
            let layout = responsive_layout(width, height, 12.0);
            assert!(layout.body_height > 0.0);
            assert!(layout.main_width > 0.0);
            assert!(layout.header_height + layout.footer_height <= height);
        }
    }

    #[test]
    fn footer_columns_keep_playback_centered_in_the_full_window() {
        for width in [800.0, 1280.0, 1920.0] {
            let columns = footer_columns(width);
            assert!((columns.left + columns.center + columns.right - width).abs() < f32::EPSILON);
            assert!((columns.left - columns.right).abs() < 0.001 || width < 900.0);
        }

        let compact = footer_columns(600.0);
        assert_eq!(compact.left, 0.0);
        assert_eq!(compact.right, 0.0);
        assert_eq!(compact.center, 600.0);

        let medium = footer_columns(800.0);
        assert!(medium.left > 0.0);
        assert!(medium.right > 0.0);
        assert!(medium.center > 0.0);
    }

    #[test]
    fn footer_geometry_centers_playback_against_the_full_footer() {
        for width in [500.0, 700.0, 720.0, 800.0, 1200.0, 1600.0] {
            let footer =
                egui::Rect::from_min_size(egui::pos2(37.0, 700.0), egui::vec2(width, 88.0));
            let geometry = footer_geometry(footer);
            let tolerance = 0.001;
            assert!((geometry.playback.center().x - footer.center().x).abs() < tolerance);
            assert!((geometry.playback.width() - footer.width()).abs() < tolerance);
            assert!(footer.contains(geometry.left.min));
            assert!(footer.contains(geometry.playback.min));
            assert!(footer.contains(geometry.right.min));
            assert!((geometry.right.right() - footer.right()).abs() < tolerance);
        }
    }

    #[test]
    fn footer_playback_rows_fit_and_stay_centered() {
        for size in [
            (500.0, 76.0),
            (720.0, 76.0),
            (1200.0, 88.0),
            (1920.0, 104.0),
        ] {
            let footer =
                egui::Rect::from_min_size(egui::pos2(37.0, 500.0), egui::vec2(size.0, size.1));
            let rows = footer_playback_rows(footer);
            let tolerance = 0.001;

            assert!((rows.transport.center().x - footer.center().x).abs() < tolerance);
            assert!((rows.progress.center().x - footer.center().x).abs() < tolerance);
            assert!(footer.contains(rows.transport.min));
            assert!(footer.contains(rows.transport.max));
            assert!(footer.contains(rows.progress.min));
            assert!(footer.contains(rows.progress.max));
            assert!(rows.transport.bottom() <= rows.progress.top() + tolerance);
            assert!((rows.progress.top() - rows.transport.bottom() - 10.0).abs() < tolerance);
            assert!((rows.transport.center().x - rows.progress.center().x).abs() < tolerance);
            assert!(rows.transport.width() <= rows.progress.width() + tolerance);
            assert!((rows.progress.width() - footer_columns(size.0).center).abs() < tolerance);
        }
    }
}
