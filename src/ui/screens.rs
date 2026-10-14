use crate::{
    domain::Track,
    theme::Theme,
    ui::{
        apply_text_sizes,
        icons::{icon_button, IconTextures},
        layout::queue_row_columns,
        responsive::ResponsiveUi,
    },
};
use eframe::egui;

#[derive(Debug)]
pub(crate) enum HomeAction {
    PlaySearch(Track),
    AddToQueue(Track),
}

#[derive(Debug)]
pub(crate) enum QueueAction {
    Play(Track),
    Remove(usize),
}

pub(crate) fn show_header(
    ctx: &egui::Context,
    header_height: f32,
    search: &mut String,
    responsive: &ResponsiveUi,
) -> bool {
    let mut search_clicked = false;
    egui::TopBottomPanel::top("app_header")
        .exact_height(header_height)
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::BLACK)
                .inner_margin(responsive.rem(0.625)),
        )
        .show(ctx, |ui| {
            apply_text_sizes(ui, responsive);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                let gap = responsive.rem(1.25);
                let search_margin = egui::vec2(responsive.rem(0.5), responsive.rem(0.1875));
                let button_padding = egui::vec2(responsive.rem(0.8125), responsive.rem(0.4375));
                ui.spacing_mut().item_spacing.x = gap;
                ui.spacing_mut().button_padding = button_padding;
                let visuals = ui.visuals_mut();
                visuals.extreme_bg_color = SEARCH_BOX_FILL;
                let rounding = egui::Rounding::same(responsive.rem(0.5));
                visuals.widgets.inactive.rounding = rounding;
                visuals.widgets.hovered.rounding = rounding;
                visuals.widgets.active.rounding = rounding;

                // The button keeps its content width; the search box takes the
                // rest of the row up to its design width, and the pair is centered.
                let button_width =
                    text_width(ui, "Search", egui::TextStyle::Button.resolve(ui.style()))
                        + button_padding.x * 2.0;
                let available = ui.available_width();
                let search_width = (available - gap - button_width)
                    .min(responsive.rem(59.0))
                    .max(0.0);
                ui.add_space(((available - search_width - gap - button_width) / 2.0).max(0.0));

                let response = ui.add(
                    egui::TextEdit::singleline(search)
                        .hint_text("Search YouTube")
                        .margin(egui::Margin::symmetric(search_margin.x, search_margin.y))
                        .desired_width((search_width - search_margin.x * 2.0).max(0.0)),
                );
                search_clicked = ui
                    .add(
                        egui::Button::new("Search")
                            .fill(SEARCH_BUTTON_FILL)
                            .rounding(responsive.rem(1.25)),
                    )
                    .clicked();
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    search_clicked = true;
                }
            });
        });
    search_clicked
}
pub(crate) fn show_home(
    ctx: &egui::Context,
    icons: &IconTextures,
    current: Option<&Track>,
    results: &[Track],
    guest: bool,
    responsive: &ResponsiveUi,
) -> Vec<HomeAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .fill(theme.surface)
                    .rounding(responsive.space_md.clamp(8.0, 14.0))
                    .inner_margin(egui::Margin::symmetric(
                        responsive.space_lg.clamp(16.0, 24.0),
                        responsive.space_xl.clamp(20.0, 28.0),
                    ))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("NOW PLAYING")
                                .size(responsive.font_heading)
                                .color(theme.accent),
                        );
                        ui.add_space(responsive.space_md);
                        ui.horizontal(|ui| {
                            egui::Frame::none()
                                .fill(theme.raised)
                                .rounding(responsive.space_sm)
                                .inner_margin(egui::Margin::same(responsive.space_lg))
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Image::from_texture((
                                            icons.music.id(),
                                            egui::vec2(
                                                responsive.artwork_size * 0.67,
                                                responsive.artwork_size,
                                            ),
                                        ))
                                        .fit_to_exact_size(egui::vec2(
                                            responsive.artwork_size * 0.67,
                                            responsive.artwork_size,
                                        ))
                                        .tint(theme.accent),
                                    );
                                });
                            ui.vertical(|ui| {
                                if let Some(track) = current {
                                    ui.label(
                                        egui::RichText::new(&track.title)
                                            .size(responsive.font_title)
                                            .color(theme.text),
                                    );
                                    ui.label(
                                        egui::RichText::new(&track.channel)
                                            .size(responsive.font_body)
                                            .color(theme.secondary_text),
                                    );
                                    ui.add_space(responsive.space_sm);
                                    ui.small(
                                        egui::RichText::new("Audio only playback")
                                            .size(responsive.font_caption)
                                            .color(theme.secondary_text),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new("Choose something to play")
                                            .size(responsive.font_title)
                                            .color(theme.text),
                                    );
                                    ui.label(
                                        egui::RichText::new("Search YouTube to start your mix")
                                            .size(responsive.font_body)
                                            .color(theme.secondary_text),
                                    );
                                }
                            });
                        });
                    });

                ui.add_space(responsive.space_lg);
                ui.label(
                    egui::RichText::new("SEARCH RESULTS")
                        .size(responsive.font_caption)
                        .color(theme.text),
                );
                if results.is_empty() {
                    ui.small(
                        egui::RichText::new(
                            "Search for a song, artist, album, or YouTube URL to begin.",
                        )
                        .size(responsive.font_caption)
                        .color(theme.secondary_text),
                    );
                }
                for track in results {
                    egui::Frame::none()
                        .fill(theme.surface)
                        .rounding(responsive.space_sm)
                        .inner_margin(responsive.space_sm)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // A Jam guest cannot start playback; the
                                // button on the right sends a request instead.
                                if !guest
                                    && icon_button(
                                        ui,
                                        &icons.play,
                                        responsive.icon_size,
                                        egui::vec2(
                                            responsive.rem(3.0).clamp(42.0, 54.0),
                                            responsive.control_height,
                                        ),
                                        "Play",
                                    )
                                    .clicked()
                                {
                                    actions.push(HomeAction::PlaySearch(track.clone()));
                                }
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&track.title)
                                            .size(responsive.font_body)
                                            .color(theme.text),
                                    );
                                    ui.small(
                                        egui::RichText::new(format!(
                                            "{} · {}",
                                            track.channel,
                                            track.duration_text()
                                        ))
                                        .size(responsive.font_caption)
                                        .color(theme.secondary_text),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_sized(
                                                [
                                                    responsive.rem(4.5).clamp(64.0, 80.0),
                                                    responsive.control_height,
                                                ],
                                                egui::Button::new(if guest {
                                                    "Request"
                                                } else {
                                                    "Add to mix"
                                                })
                                                .fill(theme.accent),
                                            )
                                            .clicked()
                                        {
                                            actions.push(HomeAction::AddToQueue(track.clone()));
                                        }
                                    },
                                );
                            });
                        });
                }
            });
    });

    actions
}

pub(crate) fn show_queue_panel(
    ctx: &egui::Context,
    queue_width: f32,
    auto_mix: bool,
    queue: &[Track],
    responsive: &ResponsiveUi,
) -> Vec<QueueAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    egui::SidePanel::right("app_queue")
        .resizable(false)
        .exact_width(queue_width)
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin {
                left: responsive.rem(0.3125),
                right: responsive.rem(0.4375),
                top: responsive.rem(1.0625),
                bottom: responsive.rem(0.625),
            }),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("UP NEXT")
                        .size(responsive.font_body)
                        .color(theme.text),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(if auto_mix { "MIX ON" } else { "MANUAL" })
                            .size(responsive.font_caption)
                            .color(theme.accent),
                    );
                });
            });
            ui.add_space(responsive.rem(0.75));
            ui.add(egui::Separator::default().spacing(0.0));
            ui.add_space(responsive.rem(0.5));
            if queue.is_empty() {
                ui.small(
                    egui::RichText::new("Your mix will appear here as songs are added.")
                        .size(responsive.font_caption)
                        .color(theme.secondary_text),
                );
            }
            let mut remove = None;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let row_padding = egui::vec2(responsive.rem(0.5), responsive.rem(0.625));
                    let row_gap = responsive.rem(0.875);
                    let button_padding = egui::vec2(responsive.rem(0.5625), responsive.rem(0.1875));
                    ui.spacing_mut().item_spacing.y = responsive.rem(0.4375);
                    ui.spacing_mut().button_padding = button_padding;
                    // Fixed columns are measured from their real content so the
                    // title is the only part of a row that flexes.
                    let remove_width =
                        text_width(ui, "Remove", egui::TextStyle::Button.resolve(ui.style()))
                            + button_padding.x * 2.0;
                    let list_width = ui.available_width();
                    for (index, track) in queue.iter().enumerate() {
                        let index_text = format!("{:02}", index + 1);
                        let index_width = text_width(
                            ui,
                            &index_text,
                            egui::FontId::proportional(responsive.font_body),
                        );
                        let columns = queue_row_columns(
                            list_width - row_padding.x * 2.0,
                            index_width,
                            remove_width,
                            row_gap,
                        );
                        // The row hugs its content; a long title wraps inside
                        // `columns.title` and makes the row taller instead of wider.
                        egui::Frame::none()
                            .fill(QUEUE_ROW_FILL)
                            .rounding(responsive.rem(0.5))
                            .inner_margin(egui::Margin::symmetric(row_padding.x, row_padding.y))
                            .show(ui, |ui| {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = row_gap;
                                    ui.label(
                                        egui::RichText::new(index_text)
                                            .size(responsive.font_body)
                                            .color(theme.accent),
                                    );
                                    ui.vertical(|ui| {
                                        ui.set_max_width(columns.title);
                                        // A wrapping title fills the row, like a
                                        // stretched flex item, so the card spans the list.
                                        let title_font =
                                            egui::FontId::proportional(responsive.font_body);
                                        if text_width(ui, &track.title, title_font) > columns.title
                                        {
                                            ui.set_min_width(columns.title);
                                        }
                                        ui.spacing_mut().item_spacing.y = responsive.rem(0.5);
                                        if ui
                                            .add(
                                                egui::Label::new(
                                                    egui::RichText::new(&track.title)
                                                        .size(responsive.font_body)
                                                        .color(theme.text),
                                                )
                                                .wrap()
                                                .sense(egui::Sense::click()),
                                            )
                                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                                            .clicked()
                                        {
                                            actions.push(QueueAction::Play(track.clone()));
                                        }
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(format!(
                                                    "{} · {}",
                                                    track.channel,
                                                    track.duration_text()
                                                ))
                                                .size(responsive.font_caption)
                                                .color(theme.secondary_text),
                                            )
                                            .wrap(),
                                        );
                                    });
                                    if ui
                                        .add(
                                            egui::Button::new("Remove")
                                                .fill(QUEUE_REMOVE_FILL)
                                                .rounding(responsive.rem(0.3125)),
                                        )
                                        .clicked()
                                    {
                                        remove = Some(index);
                                    }
                                });
                            });
                    }
                });
            if let Some(index) = remove {
                actions.push(QueueAction::Remove(index));
            }
        });

    actions
}

// Header and queue card colors from the design.
const SEARCH_BOX_FILL: egui::Color32 = egui::Color32::from_rgb(0x29, 0x27, 0x2D);
const SEARCH_BUTTON_FILL: egui::Color32 = egui::Color32::from_rgb(0x25, 0x23, 0x2A);
const QUEUE_ROW_FILL: egui::Color32 = egui::Color32::from_rgb(0x1C, 0x1B, 0x1F);
const QUEUE_REMOVE_FILL: egui::Color32 = egui::Color32::from_rgb(0x34, 0x31, 0x39);

fn text_width(ui: &egui::Ui, text: &str, font: egui::FontId) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
        .size()
        .x
}
