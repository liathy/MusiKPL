use crate::{
    domain::Track,
    theme::Theme,
    ui::icons::{icon_button, IconTextures},
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
    raised: egui::Color32,
) -> bool {
    let mut search_clicked = false;
    egui::TopBottomPanel::top("app_header")
        .exact_height(header_height)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::BLACK)
                .show(ui, |ui| {
                    ui.allocate_ui_with_layout(
                        ui.available_size(),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let button_width = 63.0;
                                    let gap = ui.spacing().item_spacing.x;
                                    let search_width = ui.available_width().clamp(180.0, 520.0);
                                    let total_width = search_width + gap + button_width;
                                    ui.add_space(
                                        ((ui.available_width() - total_width) / 2.0).max(0.0),
                                    );

                                    let response = ui.add_sized(
                                        [search_width, 28.0],
                                        egui::TextEdit::singleline(search)
                                            .hint_text("Search YouTube")
                                            .horizontal_align(egui::Align::Center),
                                    );
                                    search_clicked = ui
                                        .add_sized(
                                            [button_width, 28.0],
                                            egui::Button::new("Search").fill(raised),
                                        )
                                        .clicked();
                                    if response.lost_focus()
                                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                    {
                                        search_clicked = true;
                                    }
                                },
                            );
                        },
                    );
                });
        });
    search_clicked
}

pub(crate) fn show_home(
    ctx: &egui::Context,
    icons: &IconTextures,
    current: Option<&Track>,
    results: &[Track],
) -> Vec<HomeAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .fill(theme.surface)
                    .rounding(14.0)
                    .inner_margin(egui::Margin::symmetric(19.0, 25.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new("NOW PLAYING")
                                .size(15.0)
                                .color(theme.accent),
                        );
                        ui.add_space(15.0);
                        ui.horizontal(|ui| {
                            egui::Frame::none()
                                .fill(theme.raised)
                                .rounding(8.0)
                                .inner_margin(egui::Margin::symmetric(23.0, 20.0))
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Image::from_texture((
                                            icons.music.id(),
                                            egui::vec2(32.0, 48.0),
                                        ))
                                        .fit_to_exact_size(egui::vec2(32.0, 48.0))
                                        .tint(theme.accent),
                                    );
                                });
                            ui.vertical(|ui| {
                                if let Some(track) = current {
                                    ui.label(
                                        egui::RichText::new(&track.title)
                                            .size(18.0)
                                            .color(theme.text),
                                    );
                                    ui.label(
                                        egui::RichText::new(&track.channel)
                                            .size(12.0)
                                            .color(theme.secondary_text),
                                    );
                                    ui.add_space(8.0);
                                    ui.small(
                                        egui::RichText::new("Audio only playback")
                                            .size(8.0)
                                            .color(theme.secondary_text),
                                    );
                                } else {
                                    ui.label(
                                        egui::RichText::new("Choose something to play")
                                            .size(18.0)
                                            .color(theme.text),
                                    );
                                    ui.label(
                                        egui::RichText::new("Search YouTube to start your mix")
                                            .size(12.0)
                                            .color(theme.secondary_text),
                                    );
                                }
                            });
                        });
                    });

                ui.add_space(18.0);
                ui.label(
                    egui::RichText::new("SEARCH RESULTS")
                        .size(9.0)
                        .color(theme.text),
                );
                if results.is_empty() {
                    ui.small(
                        egui::RichText::new(
                            "Search for a song, artist, album, or YouTube URL to begin.",
                        )
                        .size(9.0)
                        .color(theme.secondary_text),
                    );
                }
                for track in results {
                    egui::Frame::none()
                        .fill(theme.surface)
                        .rounding(8.0)
                        .inner_margin(8.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if icon_button(
                                    ui,
                                    &icons.play,
                                    16.0,
                                    egui::vec2(47.0, 28.0),
                                    "Play",
                                )
                                .clicked()
                                {
                                    actions.push(HomeAction::PlaySearch(track.clone()));
                                }
                                ui.vertical(|ui| {
                                    ui.label(
                                        egui::RichText::new(&track.title)
                                            .size(11.0)
                                            .color(theme.text),
                                    );
                                    ui.small(
                                        egui::RichText::new(format!(
                                            "{} · {}",
                                            track.channel,
                                            track.duration_text()
                                        ))
                                        .size(8.0)
                                        .color(theme.secondary_text),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add_sized(
                                                [72.0, 28.0],
                                                egui::Button::new("Add to mix").fill(theme.accent),
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
) -> Vec<QueueAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    egui::SidePanel::right("app_queue")
        .resizable(false)
        .exact_width(queue_width)
        .show(ctx, |ui| {
            ui.add_space(17.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("UP NEXT").size(12.0).color(theme.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(if auto_mix { "MIX ON" } else { "MANUAL" })
                            .size(9.0)
                            .color(theme.accent),
                    );
                });
            });
            ui.separator();
            if queue.is_empty() {
                ui.small(
                    egui::RichText::new("Your mix will appear here as songs are added.")
                        .size(9.0)
                        .color(theme.secondary_text),
                );
            }
            let mut remove = None;
            for (index, track) in queue.iter().enumerate() {
                egui::Frame::none()
                    .fill(theme.surface)
                    .rounding(8.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!("{:02}", index + 1))
                                    .size(11.0)
                                    .color(theme.accent),
                            );
                            if ui
                                .selectable_label(
                                    false,
                                    egui::RichText::new(&track.title).size(11.0),
                                )
                                .clicked()
                            {
                                actions.push(QueueAction::Play(track.clone()));
                            }
                            if ui
                                .add_sized([51.0, 17.0], egui::Button::new("Remove"))
                                .clicked()
                            {
                                remove = Some(index);
                            }
                        });
                        ui.small(
                            egui::RichText::new(format!(
                                "{} · {}",
                                track.channel,
                                track.duration_text()
                            ))
                            .size(8.0)
                            .color(theme.secondary_text),
                        );
                    });
            }
            if let Some(index) = remove {
                actions.push(QueueAction::Remove(index));
            }
        });

    actions
}
