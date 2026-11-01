//! Home: rows of songs picked from what the user listens to most, plus new
//! music to discover. On first run it asks for a few favorite genres.

use eframe::egui::{self, pos2, Align2, Color32, CursorIcon, Rect, Sense, Ui};

use crate::{
    domain::Track,
    recommendations::home::{display_genre, STARTER_GENRES},
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        responsive::ResponsiveUi,
        screens::{card_size, song_card, CardContext, ResultAction},
        widgets::{self, hover_amount, mix, pill_button, ButtonColors, PillKind, PillSize},
    },
};

#[derive(Debug)]
pub(crate) enum HomeAction {
    Card(ResultAction),
    Refresh,
    ToggleGenre(String),
    FinishGenres,
    SkipGenres,
    OpenPlaylist(u64),
    /// The row was scrolled to its end: show or find more songs.
    MoreInRow(String),
    /// Home was scrolled to the bottom: add more rows.
    MoreRows,
}

pub(crate) struct HomeRowView<'a> {
    pub(crate) title: &'a str,
    /// The songs shown so far.
    pub(crate) tracks: &'a [Track],
    /// A favorite playlist's row: its title opens the playlist.
    pub(crate) playlist: Option<u64>,
    /// More songs are being found for this row.
    pub(crate) loading: bool,
}

pub(crate) struct HomeView<'a> {
    pub(crate) rows: Vec<HomeRowView<'a>>,
    /// Home is being rebuilt in the background.
    pub(crate) building: bool,
    /// Showing the genre picker, with the genres picked so far.
    pub(crate) picking_genres: Option<&'a [String]>,
    pub(crate) cards: CardContext<'a>,
    /// More rows are being built.
    pub(crate) loading_rows: bool,
}

/// How close to the end (in pixels) counts as reaching it.
fn near_end(offset: f32, viewport: f32, content: f32, margin: f32) -> bool {
    content > viewport && offset + viewport >= content - margin
}

pub(crate) fn show(
    ui: &mut Ui,
    area: Rect,
    view: &HomeView<'_>,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<HomeAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
        ui.set_clip_rect(area);
        let output = egui::ScrollArea::vertical()
            .id_salt("home_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(r.rem(1.5)))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if let Some(picks) = view.picking_genres {
                            genre_picker(ui, picks, &theme, r, &mut actions);
                            return;
                        }
                        header(ui, view, icons, &theme, r, &mut actions);
                        ui.add_space(r.rem(1.0));
                        if view.rows.is_empty() {
                            empty(ui, view.building, &theme, r);
                        }
                        for (index, row) in view.rows.iter().enumerate() {
                            song_row(ui, index, row, view, icons, &theme, r, &mut actions);
                        }
                        if view.loading_rows {
                            ui.horizontal(|ui| {
                                widgets::spinner(ui, r.rem(1.125));
                                ui.label(
                                    egui::RichText::new("Finding more for you…")
                                        .font(fonts::body(r.rem(0.9375)))
                                        .color(theme.secondary_text),
                                );
                            });
                        }
                    });
            });
        // Also when everything fits: more rows fill the screen.
        let reached_bottom = output.content_size.y <= output.inner_rect.height()
            || near_end(
                output.state.offset.y,
                output.inner_rect.height(),
                output.content_size.y,
                // Start a screen early, so new rows are ready on arrival.
                output.inner_rect.height(),
            );
        if reached_bottom && view.picking_genres.is_none() && !view.rows.is_empty() {
            actions.push(HomeAction::MoreRows);
        }
    });
    actions
}

fn header(
    ui: &mut Ui,
    view: &HomeView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<HomeAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.75);
        ui.label(
            egui::RichText::new("Home")
                .font(fonts::display(r.rem(1.75)))
                .color(theme.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if view.building {
                ui.label(
                    egui::RichText::new("Refreshing…")
                        .font(fonts::body(r.rem(0.875)))
                        .color(theme.secondary_text),
                );
                widgets::spinner(ui, r.rem(1.125));
            } else {
                let refresh = widgets::icon_button(
                    ui,
                    &icons.refresh,
                    r.rem2(2.25, 2.25),
                    r.rem2(1.125, 1.125),
                    r.rem(1.125),
                    ButtonColors::round(theme),
                );
                if refresh.on_hover_text("Refresh Home").clicked() {
                    actions.push(HomeAction::Refresh);
                }
            }
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn song_row(
    ui: &mut Ui,
    index: usize,
    row: &HomeRowView<'_>,
    view: &HomeView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<HomeAction>,
) {
    fonts::cover(ui.ctx(), row.title);
    let title = egui::RichText::new(row.title)
        .font(fonts::display(r.rem(1.375)))
        .color(theme.text);
    match row.playlist {
        Some(id) => {
            let title = ui
                .add(egui::Label::new(title).sense(egui::Sense::click()))
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Open playlist");
            if title.clicked() {
                actions.push(HomeAction::OpenPlaylist(id));
            }
        }
        None => {
            ui.label(title);
        }
    }
    ui.add_space(r.rem(0.5));
    let card = card_size(r);
    let output = egui::ScrollArea::horizontal()
        .id_salt(("home_row", index, row.title))
        .auto_shrink([false, true])
        // Rows scroll by dragging or Shift + wheel, without a bar.
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = r.rem(1.0);
                for track in row.tracks {
                    if let Some(action) = song_card(ui, card, track, &view.cards, icons, theme, r) {
                        actions.push(HomeAction::Card(action));
                    }
                }
                if row.loading {
                    let (rect, _) = ui.allocate_exact_size(card, egui::Sense::hover());
                    ui.put(
                        egui::Rect::from_center_size(rect.center(), r.rem2(1.5, 1.5)),
                        |ui: &mut Ui| widgets::spinner(ui, r.rem(1.5)),
                    );
                }
            });
        });
    if near_end(
        output.state.offset.x,
        output.inner_rect.width(),
        output.content_size.x,
        // A few cards early, so the next batch is ready on arrival.
        card.x * 3.0,
    ) {
        actions.push(HomeAction::MoreInRow(row.title.to_owned()));
    }
    ui.add_space(r.rem(1.5));
}

fn empty(ui: &mut Ui, building: bool, theme: &Theme, r: &ResponsiveUi) {
    ui.add_space(r.rem(4.0));
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = r.rem(0.75);
        let (title, detail) = if building {
            (
                "Building your Home…",
                "Finding songs you'll like. This takes a few seconds.",
            )
        } else {
            (
                "Nothing here yet",
                "Search and play some music: Home learns from what you listen to.",
            )
        };
        ui.label(
            egui::RichText::new(title)
                .font(fonts::display(r.rem(1.75)))
                .color(theme.text),
        );
        ui.label(
            egui::RichText::new(detail)
                .font(fonts::body(r.rem(1.0)))
                .color(theme.secondary_text),
        );
        if building {
            widgets::spinner(ui, r.rem(1.5));
        }
    });
}

fn genre_picker(
    ui: &mut Ui,
    picks: &[String],
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<HomeAction>,
) {
    ui.spacing_mut().item_spacing.y = r.rem(0.5);
    ui.label(
        egui::RichText::new("What do you like to listen to?")
            .font(fonts::display(r.rem(2.0)))
            .color(theme.text),
    );
    ui.label(
        egui::RichText::new(
            "Pick a few genres. Home starts from them, then learns from what you play, skip and like.",
        )
        .font(fonts::body(r.rem(1.0)))
        .color(theme.secondary_text),
    );
    ui.add_space(r.rem(1.0));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = r.rem2(0.5, 0.5);
        for genre in STARTER_GENRES {
            let selected = picks.iter().any(|pick| pick == genre);
            if chip(ui, &display_genre(genre), selected, theme, r).clicked() {
                actions.push(HomeAction::ToggleGenre((*genre).to_owned()));
            }
        }
    });
    ui.add_space(r.rem(1.5));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.75);
        let ready = !picks.is_empty();
        let label = if ready {
            format!("Continue with {}", picks.len())
        } else {
            "Pick at least one".to_owned()
        };
        if pill_button(ui, &label, PillKind::Primary, PillSize::Medium, r).clicked() && ready {
            actions.push(HomeAction::FinishGenres);
        }
        if pill_button(ui, "Skip", PillKind::Secondary, PillSize::Medium, r)
            .on_hover_text("Home will learn from what you play instead")
            .clicked()
        {
            actions.push(HomeAction::SkipGenres);
        }
    });
}

/// A rounded, toggleable genre label.
fn chip(
    ui: &mut Ui,
    label: &str,
    selected: bool,
    theme: &Theme,
    r: &ResponsiveUi,
) -> egui::Response {
    let font = fonts::body(r.rem(0.9375));
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme.text);
    let size = galley.size() + r.rem2(1.0, 0.5) * 2.0;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hover = hover_amount(ui, &response);
        let fill = if selected {
            theme.text
        } else {
            mix(theme.surface, theme.hover, hover)
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let text = if selected { Color32::BLACK } else { theme.text };
        ui.painter().text(
            pos2(rect.center().x, rect.center().y),
            Align2::CENTER_CENTER,
            label,
            fonts::body(r.rem(0.9375)),
            text,
        );
    }
    response.on_hover_cursor(CursorIcon::PointingHand)
}
