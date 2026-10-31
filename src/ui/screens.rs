//! The main area: search results as a grid of cover cards, plus the error
//! toast that floats above it.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, Id, Order, Rect, Sense, Stroke, Ui,
};

use crate::{
    domain::{CoverArt, Track},
    theme::Theme,
    ui::{
        fonts,
        icons::{paint_icon, IconTextures},
        layout::grid_columns,
        responsive::ResponsiveUi,
        widgets::{self, elided, hover_amount, item_id, mix, pointer_amount},
    },
};

#[derive(Debug)]
pub(crate) enum ResultAction {
    Play(Track),
    AddToMix(Track),
}

pub(crate) struct ResultsView<'a> {
    pub(crate) results: &'a [Track],
    /// The query the results are for.
    pub(crate) query: &'a str,
    pub(crate) searching: bool,
    /// Jam guests request songs from the host instead of playing them.
    pub(crate) guest: bool,
    pub(crate) covers: &'a CoverArt,
    pub(crate) in_mix: &'a dyn Fn(&str) -> bool,
}

pub(crate) fn show_results(
    ui: &mut Ui,
    area: Rect,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<ResultAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
        if view.results.is_empty() {
            empty_state(ui, area, view, &theme, r);
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(r.rem(1.5)))
                    .show(ui, |ui| {
                        header(ui, view, &theme, r);
                        ui.add_space(r.rem(1.25));
                        grid(ui, view, icons, &theme, r, &mut actions);
                    });
            });
    });
    actions
}

fn header(ui: &mut Ui, view: &ResultsView<'_>, theme: &Theme, r: &ResponsiveUi) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.75);
        ui.label(
            egui::RichText::new(format!("Results for “{}”", view.query))
                .font(fonts::display(r.rem(1.75)))
                .color(theme.text),
        );
        let count = view.results.len();
        ui.label(
            egui::RichText::new(format!("{count} song{}", if count == 1 { "" } else { "s" }))
                .font(fonts::body(r.rem(0.9375)))
                .color(theme.secondary_text),
        );
        if view.searching {
            widgets::spinner(ui, r.rem(1.125));
        }
    });
}

fn grid(
    ui: &mut Ui,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ResultAction>,
) {
    let card = vec2(r.rem(11.4375), r.rem(15.3125));
    let gap = r.rem(1.0);
    let columns = grid_columns(ui.available_width(), card.x, gap);
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    for row in view.results.chunks(columns) {
        ui.horizontal(|ui| {
            for track in row {
                result_card(ui, card, track, view, icons, theme, r, actions);
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn result_card(
    ui: &mut Ui,
    size: egui::Vec2,
    track: &Track,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ResultAction>,
) {
    let (rect, card) = ui.allocate_exact_size(size, Sense::click());
    let hover = pointer_amount(ui, &card);
    let rounding = r.rem(0.625);
    ui.painter().rect(
        rect,
        rounding,
        mix(theme.panel, Color32::from_rgb(24, 24, 24), hover),
        Stroke::new(1.0_f32, mix(theme.border, theme.hover, hover)),
    );

    let inner = rect.shrink(r.rem(0.75));
    let cover_rect = Rect::from_min_size(inner.min, egui::Vec2::splat(inner.width()));
    widgets::cover(
        ui,
        cover_rect,
        view.covers.cover_for(track).as_ref(),
        icons,
        r.rem(0.5),
    );

    let title_rect = Rect::from_min_size(
        pos2(inner.left(), cover_rect.bottom() + r.rem(0.625)),
        vec2(inner.width(), r.rem(1.75)),
    );
    elided(
        ui.painter(),
        title_rect,
        &track.title,
        fonts::display(r.rem(1.125)),
        theme.text,
        Align2::LEFT_CENTER,
    );
    let author_rect = title_rect.translate(vec2(0.0, title_rect.height()));
    let author_rect = Rect::from_min_size(author_rect.min, vec2(inner.width(), r.rem(1.375)));
    elided(
        ui.painter(),
        author_rect,
        &track.channel,
        fonts::body(r.rem(0.875)),
        theme.secondary_text,
        Align2::LEFT_CENTER,
    );

    // + Add to mix, on the cover's bottom-right corner.
    let button_size = r.rem(2.0);
    let button_rect = Rect::from_min_size(
        cover_rect.max - egui::Vec2::splat(button_size + r.rem(0.5)),
        egui::Vec2::splat(button_size),
    );
    let added = !view.guest && (view.in_mix)(&track.id);
    let add = ui.interact(
        button_rect,
        item_id("add_to_mix", &track.id),
        Sense::click(),
    );
    let add_hover = hover_amount(ui, &add);
    let light = if added { 1.0 } else { add_hover };
    let painter = ui.painter();
    painter.circle(
        button_rect.center(),
        button_size / 2.0,
        mix(Color32::from_black_alpha(178), theme.text, light),
        Stroke::new(1.0_f32, Color32::from_white_alpha(38)),
    );
    paint_icon(
        painter,
        if added { &icons.added } else { &icons.add },
        Rect::from_center_size(button_rect.center(), r.rem2(1.0, 1.0)),
        mix(theme.text, Color32::BLACK, light),
    );
    let tooltip = match (view.guest, added) {
        (true, _) => "Request from the host",
        (false, true) => "In your mix",
        (false, false) => "Add to mix",
    };
    if add
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tooltip)
        .clicked()
    {
        if !added {
            actions.push(ResultAction::AddToMix(track.clone()));
        }
    } else if card
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if view.guest {
            "Request from the host"
        } else {
            "Play"
        })
        .clicked()
    {
        actions.push(ResultAction::Play(track.clone()));
    }
}

fn empty_state(ui: &mut Ui, area: Rect, view: &ResultsView<'_>, theme: &Theme, r: &ResponsiveUi) {
    let (title, detail) = if view.searching {
        (
            format!("Searching for “{}”…", view.query),
            "Looking on YouTube".to_owned(),
        )
    } else if view.query.is_empty() {
        (
            "Search for music".to_owned(),
            "Type a song, artist, album, or YouTube link above and press Enter.".to_owned(),
        )
    } else {
        (
            format!("No results for “{}”", view.query),
            "Try another search.".to_owned(),
        )
    };
    let center = area.center();
    let painter = ui.painter();
    painter.text(
        center - vec2(0.0, r.rem(1.0)),
        Align2::CENTER_CENTER,
        title,
        fonts::display(r.rem(2.0)),
        theme.text,
    );
    painter.text(
        center + vec2(0.0, r.rem(1.25)),
        Align2::CENTER_CENTER,
        detail,
        fonts::body(r.rem(1.0)),
        theme.secondary_text,
    );
    if view.searching {
        let spinner = Rect::from_center_size(center + vec2(0.0, r.rem(3.5)), r.rem2(1.5, 1.5));
        ui.put(spinner, |ui: &mut Ui| widgets::spinner(ui, r.rem(1.5)));
    }
}

/// A dismissable error card at the bottom-left of the main area. Returns
/// true when the user closed it.
pub(crate) fn error_toast(
    ctx: &egui::Context,
    main: Rect,
    message: &str,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> bool {
    let theme = Theme::default();
    let width = r.rem(26.0).min(main.width() - r.rem(3.0));
    let font = fonts::body(r.rem(0.9375));
    egui::Area::new(Id::new("error_toast"))
        .order(Order::Foreground)
        .fixed_pos(pos2(main.left() + r.rem(1.5), main.bottom() - r.rem(1.5)))
        .pivot(Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            let frame = egui::Frame::none()
                .fill(theme.panel)
                .stroke(Stroke::new(1.0_f32, theme.danger))
                .rounding(r.rem(0.75))
                .inner_margin(egui::Margin::symmetric(r.rem(1.0), r.rem(0.75)));
            let mut closed = false;
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal_top(|ui| {
                    let button = r.rem(1.75);
                    let gap = r.rem(0.75);
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.allocate_ui(vec2(width - button - gap, 0.0), |ui| {
                        ui.set_width(width - button - gap);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message)
                                    .font(font.clone())
                                    .color(theme.text),
                            )
                            .wrap(),
                        );
                    });
                    let close = widgets::icon_button(
                        ui,
                        &icons.close,
                        vec2(button, button),
                        r.rem2(0.875, 0.875),
                        button / 2.0,
                        widgets::ButtonColors::round(&theme),
                    );
                    closed = close.on_hover_text("Dismiss").clicked();
                });
            });
            closed
        })
        .inner
}
