//! The lyrics view: the song's lines, the one being sung lit up and kept in
//! view as the song plays. Only the lines on screen are drawn.

use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, Ui};
use musikpl_core::engine::LyricsStatus;

use crate::{
    theme::Theme,
    ui::{
        fonts,
        responsive::ResponsiveUi,
        widgets::{pill_button, PillKind, PillSize},
    },
};

#[derive(Debug, PartialEq)]
pub(crate) enum LyricsAction {
    Seek(f64),
    Retry,
}

pub(crate) struct LyricsView<'a> {
    pub(crate) status: &'a LyricsStatus,
    /// The line being sung (synced lyrics).
    pub(crate) line: Option<usize>,
    pub(crate) title: &'a str,
}

/// After scrolling by hand, the view goes back to following the song this
/// long after the last scroll.
const FOLLOW_AGAIN: f64 = 3.0;

pub(crate) fn show(
    ui: &mut Ui,
    area: Rect,
    view: &LyricsView<'_>,
    r: &ResponsiveUi,
) -> Option<LyricsAction> {
    let theme = Theme::default();
    ui.painter().rect_filled(area, 0.0, theme.background);
    let lyrics = match view.status {
        LyricsStatus::Found(lyrics) => lyrics,
        LyricsStatus::Idle => {
            message(ui, area, "Play a song to see its lyrics.", r);
            return None;
        }
        LyricsStatus::Loading => {
            message(ui, area, "Looking for the lyrics…", r);
            return None;
        }
        LyricsStatus::Missing => {
            message(ui, area, "No lyrics found for this song.", r);
            return None;
        }
        LyricsStatus::Failed(error) => {
            message(ui, area, error, r);
            let button = Rect::from_center_size(
                area.center() + vec2(0.0, r.rem(2.5)),
                vec2(r.rem(7.0), r.rem(2.25)),
            );
            let retry = ui
                .allocate_new_ui(egui::UiBuilder::new().max_rect(button), |ui| {
                    pill_button(ui, "Try again", PillKind::Secondary, PillSize::Small, r).clicked()
                })
                .inner;
            return retry.then_some(LyricsAction::Retry);
        }
    };
    if lyrics.instrumental && !lyrics.is_synced() && lyrics.plain.is_empty() {
        message(ui, area, "♪ Instrumental ♪", r);
        return None;
    }

    let synced = lyrics.is_synced();
    let texts: Vec<&str> = if synced {
        lyrics
            .synced
            .iter()
            .map(|line| line.text.as_str())
            .collect()
    } else {
        lyrics.plain.iter().map(String::as_str).collect()
    };
    let margin = r.rem(3.0);
    let width = (area.width() - 2.0 * margin).max(r.rem(10.0));
    let size = if synced { r.rem(1.875) } else { r.rem(1.25) };
    let gap = if synced { r.rem(1.0) } else { r.rem(0.375) };
    let painter = ui.painter_at(area);

    // Lay out every line (egui caches the shaping), noting where each sits.
    let galleys: Vec<_> = texts
        .iter()
        .map(|text| {
            // An empty line is a pause: show a note.
            let text = if text.is_empty() && synced {
                "♪"
            } else {
                text
            };
            painter.layout(text.to_owned(), fonts::display(size), Color32::WHITE, width)
        })
        .collect();
    let mut tops = Vec::with_capacity(galleys.len());
    let mut y = 0.0;
    for galley in &galleys {
        tops.push(y);
        y += galley.size().y + gap;
    }
    let total = y;

    // Follow the song, unless the user scrolled a moment ago.
    let id = Id::new("lyrics_scroll");
    let now = ui.input(|input| input.time);
    let (mut manual, mut scrolled_at) = ui.data(|data| {
        data.get_temp::<(f32, f64)>(id)
            .unwrap_or((0.0, f64::NEG_INFINITY))
    });
    let follow_at = view
        .line
        .and_then(|line| tops.get(line).zip(galleys.get(line)))
        .map_or(0.0, |(top, galley)| {
            top + galley.size().y / 2.0 - area.height() * 0.35
        });
    let hovered = ui.rect_contains_pointer(area);
    let wheel = if hovered {
        ui.input(|input| input.smooth_scroll_delta.y)
    } else {
        0.0
    };
    let following = synced && now - scrolled_at > FOLLOW_AGAIN;
    if wheel != 0.0 {
        if following {
            manual = ui
                .ctx()
                .animate_value_with_time(id.with("follow"), follow_at, 0.0);
        }
        manual -= wheel;
        scrolled_at = now;
    }
    let max = (total - area.height() * 0.5).max(0.0);
    manual = manual.clamp(-area.height() * 0.35, max);
    ui.data_mut(|data| data.insert_temp(id, (manual, scrolled_at)));
    let offset = if synced && now - scrolled_at > FOLLOW_AGAIN {
        ui.ctx()
            .animate_value_with_time(id.with("follow"), follow_at, 0.35)
    } else {
        // Keep the follow animation where the view is, to resume smoothly.
        ui.ctx()
            .animate_value_with_time(id.with("follow"), manual, 0.0);
        manual
    };
    if synced && now - scrolled_at <= FOLLOW_AGAIN {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(FOLLOW_AGAIN));
    }

    let top = area.top() + r.rem(2.0) - offset;
    let mut action = None;
    for (index, galley) in galleys.into_iter().enumerate() {
        let y = top + tops[index];
        let height = galley.size().y;
        if y + height < area.top() || y > area.bottom() {
            continue;
        }
        let color = match view.line {
            _ if !synced => theme.text,
            Some(line) if line == index => Color32::WHITE,
            Some(line) if index < line => theme.secondary_text,
            _ => theme.muted_text,
        };
        let rect = Rect::from_min_size(pos2(area.left() + margin, y), galley.size());
        if synced {
            let response = ui
                .interact(rect, id.with(index), Sense::click())
                .on_hover_cursor(CursorIcon::PointingHand);
            if response.clicked() {
                action = Some(LyricsAction::Seek(lyrics.synced[index].at));
                ui.data_mut(|data| data.insert_temp(id, (manual, f64::NEG_INFINITY)));
            }
            let color = if response.hovered() {
                theme.text
            } else {
                color
            };
            painter.galley_with_override_text_color(rect.min, galley, color);
        } else {
            painter.galley_with_override_text_color(rect.min, galley, color);
        }
    }
    if !synced {
        painter.text(
            pos2(area.left() + margin, area.bottom() - r.rem(1.0)),
            Align2::LEFT_BOTTOM,
            format!("{} · lyrics aren't timed for this song", view.title),
            fonts::body(r.rem(0.8125)),
            theme.muted_text,
        );
    }
    action
}

fn message(ui: &mut Ui, area: Rect, text: &str, r: &ResponsiveUi) {
    ui.painter().text(
        area.center(),
        Align2::CENTER_CENTER,
        text,
        fonts::display(r.rem(1.375)),
        Theme::default().secondary_text,
    );
}
