use crate::ui::responsive::{LayoutMode, ResponsiveUi};
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

pub(crate) fn responsive_layout(size: egui::Vec2, responsive: &ResponsiveUi) -> ResponsiveLayout {
    let width = size.x.max(0.0);
    let height = size.y.max(0.0);
    // Secondary panels collapse from the current content width so resizing and
    // DPI scaling cannot squeeze the main content into a negative rectangle.
    let header_height = responsive.header_height;
    let footer_height = responsive.player_footer_height;
    let show_sidebar = responsive.mode != LayoutMode::Compact;
    let show_queue = responsive.mode == LayoutMode::Wide;
    let sidebar_width = responsive.sidebar_width;
    let queue_width = responsive.queue_width;
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
pub(crate) struct QueueRowColumns {
    pub(crate) index: f32,
    pub(crate) title: f32,
    pub(crate) remove: f32,
}

/// Split one queue row into index, title, and remove-button columns.
///
/// The index and remove widths are reserved in full first; the title gets
/// whatever is left after the two gaps, and collapses to zero rather than
/// taking space from the button.
pub(crate) fn queue_row_columns(
    row_width: f32,
    index_width: f32,
    remove_width: f32,
    gap: f32,
) -> QueueRowColumns {
    let index = index_width.max(0.0);
    let remove = remove_width.max(0.0);
    let title = (row_width - index - remove - gap.max(0.0) * 2.0).max(0.0);

    QueueRowColumns {
        index,
        title,
        remove,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FooterColumns {
    pub(crate) left: f32,
    pub(crate) center: f32,
    pub(crate) right: f32,
}

pub(crate) fn footer_columns(width: f32, responsive: &ResponsiveUi) -> FooterColumns {
    // Compact layout: prioritize playback when the window is too narrow for
    // track details and volume to fit comfortably beside it.
    if responsive.mode == LayoutMode::Compact || width < 720.0 {
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

pub(crate) fn footer_padding(width: f32, responsive: &ResponsiveUi) -> f32 {
    (width * 0.02).clamp(responsive.space_sm, responsive.space_lg)
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

pub(crate) fn footer_playback_rows(
    playback_rect: egui::Rect,
    responsive: &ResponsiveUi,
) -> FooterPlaybackRows {
    let transport_height = responsive.control_height.clamp(30.0, 38.0);
    let progress_height = responsive
        .control_height
        .mul_add(0.5, 0.0)
        .clamp(18.0, 24.0);
    let row_gap = responsive.space_sm.clamp(8.0, 12.0);

    // The playback rectangle is a full-width positioning reference. The
    // widgets themselves remain in compact rectangles so they cannot cover
    // the track details or volume controls.
    let transport_width = responsive
        .rem(7.5)
        .clamp(100.0, 140.0)
        .min(playback_rect.width());
    // Use the actual middle column width so the progress row fills the space
    // between the side sections without changing its centered position.
    let footer_width = playback_rect.width().max(0.0);
    let progress_width = footer_columns(footer_width, responsive)
        .center
        .min(footer_width);

    // Treat the buttons and seek bar as one centered vertical control group.
    // Both rows use the footer midpoint as their horizontal midpoint.
    let content_height = transport_height + row_gap + progress_height;
    let content_top = playback_rect.center().y - content_height / 2.0;
    let content_top = content_top.max(playback_rect.top());
    let transport_top = content_top;
    let progress_top = transport_top + transport_height + row_gap;
    let progress_top = progress_top.min(playback_rect.bottom() - progress_height);

    let transport_bottom = transport_top + transport_height;
    debug_assert!((progress_top - transport_bottom - row_gap).abs() < 0.1);

    FooterPlaybackRows {
        transport: egui::Rect::from_center_size(
            egui::pos2(
                playback_rect.center().x,
                transport_top + transport_height / 2.0,
            ),
            egui::vec2(transport_width, transport_height),
        ),
        progress: egui::Rect::from_center_size(
            egui::pos2(
                playback_rect.center().x,
                progress_top + progress_height / 2.0,
            ),
            egui::vec2(progress_width, progress_height),
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FooterTrackInfo {
    pub(crate) icon: egui::Rect,
    pub(crate) text: egui::Rect,
}

/// Place the track icon and its two-line text block inside `content`.
///
/// Both are centered on the vertical middle of `content`. The text takes the
/// width left after the icon and one `gap`, and never extends past
/// `content`'s right edge, so long titles must truncate.
pub(crate) fn footer_track_info(
    content: egui::Rect,
    icon_size: f32,
    gap: f32,
    text_height: f32,
) -> FooterTrackInfo {
    let center_y = content.center().y;
    let icon = egui::Rect::from_center_size(
        egui::pos2(content.left() + icon_size / 2.0, center_y),
        egui::Vec2::splat(icon_size),
    );
    let text_left = (icon.right() + gap).min(content.right());
    let text = egui::Rect::from_min_max(
        egui::pos2(text_left, center_y - text_height / 2.0),
        egui::pos2(content.right().max(text_left), center_y + text_height / 2.0),
    );

    FooterTrackInfo { icon, text }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FooterTransportButtons {
    pub(crate) previous: egui::Rect,
    pub(crate) play: egui::Rect,
    pub(crate) next: egui::Rect,
}

/// Place previous, play, and next around `center`.
///
/// The play button is centered exactly on `center`; the side buttons sit one
/// `gap` away on either side and share its vertical center, whatever their
/// individual sizes are.
pub(crate) fn footer_transport_buttons(
    center: egui::Pos2,
    side_size: egui::Vec2,
    play_size: egui::Vec2,
    gap: f32,
) -> FooterTransportButtons {
    let side_offset = play_size.x / 2.0 + gap + side_size.x / 2.0;

    FooterTransportButtons {
        previous: egui::Rect::from_center_size(
            egui::pos2(center.x - side_offset, center.y),
            side_size,
        ),
        play: egui::Rect::from_center_size(center, play_size),
        next: egui::Rect::from_center_size(egui::pos2(center.x + side_offset, center.y), side_size),
    }
}

pub(crate) fn footer_geometry(
    footer_rect: egui::Rect,
    responsive: &ResponsiveUi,
) -> FooterGeometry {
    let columns = footer_columns(footer_rect.width(), responsive);
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
        let compact_size = egui::vec2(800.0, 600.0);
        let compact = responsive_layout(compact_size, &ResponsiveUi::from_size(compact_size));
        assert!(!compact.show_sidebar);
        assert!(!compact.show_queue);
        assert_eq!(compact.main_width, 800.0);

        let desktop_size = egui::vec2(1280.0, 720.0);
        let desktop = responsive_layout(desktop_size, &ResponsiveUi::from_size(desktop_size));
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
            let size = egui::vec2(width, height);
            let layout = responsive_layout(size, &ResponsiveUi::from_size(size));
            assert!(layout.body_height > 0.0);
            assert!(layout.main_width > 0.0);
            assert!(layout.header_height + layout.footer_height <= height);
        }
    }

    #[test]
    fn queue_row_columns_fill_the_row_without_exceeding_it() {
        let (index, remove, gap) = (22.0, 64.0, 8.0);
        for row_width in [200.0, 329.6, 600.0] {
            let columns = queue_row_columns(row_width, index, remove, gap);
            let used = columns.index + columns.title + columns.remove + gap * 2.0;

            assert_eq!(columns.index, index);
            assert_eq!(columns.remove, remove);
            assert!(columns.title > 0.0);
            assert!((used - row_width).abs() < 0.001);
        }
    }

    #[test]
    fn queue_row_columns_keep_the_full_button_when_the_title_has_no_room() {
        let (index, remove, gap) = (22.0, 64.0, 8.0);
        let fixed = index + remove + gap * 2.0;

        let minimal = queue_row_columns(fixed + 1.0, index, remove, gap);
        assert!((minimal.title - 1.0).abs() < 0.001);
        assert_eq!(minimal.remove, remove);

        for row_width in [fixed, fixed - 30.0, 0.0, -5.0] {
            let columns = queue_row_columns(row_width, index, remove, gap);
            assert_eq!(columns.title, 0.0);
            assert_eq!(columns.index, index);
            assert_eq!(columns.remove, remove);
        }
    }

    #[test]
    fn footer_columns_keep_playback_centered_in_the_full_window() {
        for width in [800.0, 1280.0, 1920.0] {
            let responsive = ResponsiveUi::from_size(egui::vec2(width, 720.0));
            let columns = footer_columns(width, &responsive);
            assert!((columns.left + columns.center + columns.right - width).abs() < f32::EPSILON);
            assert!((columns.left - columns.right).abs() < 0.001 || width < 900.0);
        }

        let compact = footer_columns(600.0, &ResponsiveUi::from_size(egui::vec2(600.0, 720.0)));
        assert_eq!(compact.left, 0.0);
        assert_eq!(compact.right, 0.0);
        assert_eq!(compact.center, 600.0);

        let medium = footer_columns(900.0, &ResponsiveUi::from_size(egui::vec2(900.0, 720.0)));
        assert!(medium.left > 0.0);
        assert!(medium.right > 0.0);
        assert!(medium.center > 0.0);
    }

    #[test]
    fn footer_geometry_centers_playback_against_the_full_footer() {
        for width in [500.0, 700.0, 720.0, 800.0, 1200.0, 1600.0] {
            let footer =
                egui::Rect::from_min_size(egui::pos2(37.0, 700.0), egui::vec2(width, 88.0));
            let responsive = ResponsiveUi::from_size(egui::vec2(width, 720.0));
            let geometry = footer_geometry(footer, &responsive);
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
    fn footer_transport_buttons_share_a_center_line_and_do_not_overlap() {
        let center = egui::pos2(640.0, 678.0);
        for (side, play, gap) in [(32.0, 32.0, 8.0), (36.0, 38.0, 8.0), (42.0, 44.0, 8.0)] {
            let buttons = footer_transport_buttons(
                center,
                egui::vec2(side, side - 4.0),
                egui::vec2(play, play),
                gap,
            );
            let tolerance = 0.001;

            assert!((buttons.play.center() - center).length() < tolerance);
            assert!((buttons.previous.center().y - center.y).abs() < tolerance);
            assert!((buttons.next.center().y - center.y).abs() < tolerance);
            assert!((buttons.play.left() - buttons.previous.right() - gap).abs() < tolerance);
            assert!((buttons.next.left() - buttons.play.right() - gap).abs() < tolerance);
            assert!(
                (center.x - buttons.previous.left() - (buttons.next.right() - center.x)).abs()
                    < tolerance
            );
        }
    }

    #[test]
    fn footer_track_info_is_vertically_centered_and_stays_in_its_column() {
        for width in [0.0, 20.0, 180.0, 480.0] {
            let content =
                egui::Rect::from_min_size(egui::pos2(12.0, 644.0), egui::vec2(width, 76.0));
            let info = footer_track_info(content, 18.0, 8.0, 33.0);
            let tolerance = 0.001;

            assert!((info.icon.center().y - content.center().y).abs() < tolerance);
            assert!((info.text.center().y - content.center().y).abs() < tolerance);
            assert!((info.text.height() - 33.0).abs() < tolerance);
            assert!(info.text.width() >= 0.0);
            assert!(info.text.right() <= content.right().max(info.text.left()) + tolerance);
            if width >= 180.0 {
                assert!((info.text.left() - info.icon.right() - 8.0).abs() < tolerance);
                assert!((info.text.right() - content.right()).abs() < tolerance);
            }
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
            let responsive = ResponsiveUi::from_size(egui::vec2(size.0, 720.0));
            let rows = footer_playback_rows(footer, &responsive);
            let tolerance = 0.001;

            assert!((rows.transport.center().x - footer.center().x).abs() < tolerance);
            assert!((rows.progress.center().x - footer.center().x).abs() < tolerance);
            assert!(footer.contains(rows.transport.min));
            assert!(footer.contains(rows.transport.max));
            assert!(footer.contains(rows.progress.min));
            assert!(footer.contains(rows.progress.max));
            assert!(rows.transport.bottom() <= rows.progress.top() + tolerance);
            let expected_gap = responsive.space_sm.clamp(8.0, 12.0);
            assert!(
                (rows.progress.top() - rows.transport.bottom() - expected_gap).abs() < tolerance
            );
            assert!((rows.transport.center().x - rows.progress.center().x).abs() < tolerance);
            assert!(rows.transport.width() <= rows.progress.width() + tolerance);
            assert!(
                (rows.progress.width() - footer_columns(size.0, &responsive).center).abs()
                    < tolerance
            );
        }
    }
}
