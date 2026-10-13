use crate::{
    domain::{format_duration, format_position, Track},
    theme::Theme,
    ui::{icons::*, layout::*},
};
use eframe::egui;

#[derive(Debug, PartialEq)]
pub(crate) enum FooterAction {
    Previous,
    TogglePause,
    Next,
    Seek(f64),
    SetVolume(u8),
}

pub(crate) struct FooterState<'a> {
    pub(crate) current: Option<&'a Track>,
    pub(crate) paused: bool,
    pub(crate) position: f64,
    pub(crate) duration: f64,
    pub(crate) volume: u8,
    pub(crate) guest: bool,
}

pub(crate) fn show(
    ctx: &egui::Context,
    icons: &IconTextures,
    state: FooterState<'_>,
) -> Vec<FooterAction> {
    let theme = Theme::default();
    let available = ctx.available_rect().size();
    let unit = (available.x / 120.0).clamp(12.0, 16.0);
    let footer_height = responsive_layout(available.x, available.y, unit).footer_height;
    let mut actions = Vec::new();

    egui::TopBottomPanel::bottom("app_footer")
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::BLACK)
                .inner_margin(0.0),
        )
        .exact_height(footer_height)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(egui::Color32::BLACK)
                .show(ui, |ui| {
                    let footer_rect = ui.available_rect_before_wrap();
                    ui.set_min_size(footer_rect.size());
                    ui.painter().line_segment(
                        [footer_rect.left_top(), footer_rect.right_top()],
                        egui::Stroke::new(1.0_f32, theme.border),
                    );
                    let geometry = footer_geometry(footer_rect);
                    let padding = footer_padding(footer_rect.width());

                    ui.allocate_new_ui(
                        egui::UiBuilder::new()
                            .max_rect(footer_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            render_track_info(
                                ui,
                                geometry.left,
                                padding,
                                icons,
                                state.current,
                                theme,
                            );
                            render_playback(
                                ui,
                                geometry.playback,
                                icons,
                                state.current,
                                state.paused,
                                state.position,
                                state.duration,
                                state.guest,
                                theme,
                                &mut actions,
                            );
                            render_volume(
                                ui,
                                geometry.right,
                                padding,
                                state.volume,
                                theme,
                                &mut actions,
                            );
                        },
                    );
                });
        });

    actions
}

fn render_track_info(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    padding: f32,
    icons: &IconTextures,
    current: Option<&Track>,
    theme: Theme,
) {
    if rect.width() <= 0.0 {
        return;
    }

    let content_rect = rect.shrink2(egui::vec2(padding, 0.0));
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.horizontal(|ui| {
                ui.add(
                    egui::Image::from_texture((icons.music.id(), egui::vec2(28.0, 28.0)))
                        .fit_to_exact_size(egui::vec2(28.0, 28.0))
                        .tint(theme.accent),
                );
                ui.vertical(|ui| {
                    if let Some(track) = current {
                        ui.label(
                            egui::RichText::new(&track.title)
                                .size(14.0)
                                .color(theme.text),
                        );
                        ui.small(egui::RichText::new(&track.channel).color(theme.secondary_text));
                    } else {
                        ui.label(
                            egui::RichText::new("Nothing playing")
                                .size(14.0)
                                .color(theme.text),
                        );
                        ui.small("Search for music to begin");
                    }
                });
            });
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn render_playback(
    ui: &mut egui::Ui,
    playback_rect: egui::Rect,
    icons: &IconTextures,
    current: Option<&Track>,
    paused: bool,
    position: f64,
    duration: f64,
    guest: bool,
    theme: Theme,
    actions: &mut Vec<FooterAction>,
) {
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(playback_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            let rows = footer_playback_rows(playback_rect);
            let duration = current
                .and_then(|track| track.duration)
                .unwrap_or(duration)
                .max(0.0);
            let mut position = if duration > 0.0 {
                position.min(duration) as f32
            } else {
                0.0
            };
            // A zero-length slider range is rendered by egui with its handle at
            // the midpoint. Use a harmless disabled visual range while no track
            // is loaded so it stays at 00:00.
            let slider_end = duration.max(1.0) as f32;

            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(rows.transport)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |ui| {
                    let button_width = 28.0 + 36.0 + 28.0 + ui.spacing().item_spacing.x * 2.0;
                    let button_rect = egui::Rect::from_center_size(
                        rows.transport.center(),
                        egui::vec2(button_width, 30.0),
                    );
                    ui.allocate_new_ui(
                        egui::UiBuilder::new()
                            .max_rect(button_rect)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            ui.horizontal(|ui| {
                                if !guest
                                    && icon_button(
                                        ui,
                                        &icons.previous,
                                        20.0,
                                        egui::vec2(28.0, 28.0),
                                        "Previous",
                                    )
                                    .clicked()
                                {
                                    actions.push(FooterAction::Previous);
                                }

                                let play_icon = if paused { &icons.play } else { &icons.pause };
                                if ui
                                    .add_enabled(
                                        !guest,
                                        egui::Button::image(design_icon(play_icon, 22.0))
                                            .min_size(egui::vec2(36.0, 30.0)),
                                    )
                                    .clicked()
                                {
                                    actions.push(FooterAction::TogglePause);
                                }

                                if !guest
                                    && icon_button(
                                        ui,
                                        &icons.next,
                                        20.0,
                                        egui::vec2(28.0, 28.0),
                                        "Next",
                                    )
                                    .clicked()
                                {
                                    actions.push(FooterAction::Next);
                                }
                            });
                        },
                    );
                },
            );

            let elapsed = format_position(position as f64);
            let total = format_duration(duration);
            let (elapsed_width, total_width) = ui.fonts(|fonts| {
                let font = egui::FontId::proportional(12.0);
                let elapsed_width = fonts
                    .layout_no_wrap(elapsed.clone(), font.clone(), theme.secondary_text)
                    .size()
                    .x;
                let total_width = fonts
                    .layout_no_wrap(total.clone(), font, theme.secondary_text)
                    .size()
                    .x;
                (elapsed_width, total_width)
            });
            let gap = ui.spacing().item_spacing.x;
            let slider_width = (rows.progress.width() - elapsed_width - total_width - gap * 2.0)
                .max(40.0)
                .min(rows.progress.width());
            let content_width = elapsed_width + gap + slider_width + gap + total_width;
            let content_left = rows.progress.center().x - content_width / 2.0;
            let content_top = rows.progress.center().y - 10.0;
            let elapsed_rect = egui::Rect::from_min_size(
                egui::pos2(content_left, content_top),
                egui::vec2(elapsed_width, 20.0),
            );
            let slider_rect = egui::Rect::from_min_size(
                egui::pos2(elapsed_rect.right() + gap, content_top),
                egui::vec2(slider_width, 20.0),
            );
            let total_rect = egui::Rect::from_min_size(
                egui::pos2(slider_rect.right() + gap, content_top),
                egui::vec2(total_width, 20.0),
            );

            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(elapsed_rect)
                    .layout(egui::Layout::right_to_left(egui::Align::Center)),
                |time_ui| {
                    time_ui.label(
                        egui::RichText::new(elapsed)
                            .size(12.0)
                            .color(theme.secondary_text),
                    );
                },
            );

            let slider_response = ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(slider_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |slider_ui| {
                    let visuals = slider_ui.visuals_mut();
                    visuals.widgets.inactive.bg_fill = theme.border;
                    visuals.widgets.inactive.weak_bg_fill = theme.border;
                    visuals.widgets.hovered.bg_fill = theme.secondary_text;
                    visuals.widgets.active.bg_fill = theme.accent;
                    visuals.slider_trailing_fill = true;
                    let style = slider_ui.style_mut();
                    style.spacing.slider_width = slider_width;
                    style.spacing.slider_rail_height = 6.0;

                    slider_ui.add_sized(
                        egui::vec2(slider_width, 20.0),
                        egui::Slider::new(&mut position, 0.0..=slider_end).show_value(false),
                    )
                },
            );

            debug_assert!((slider_response.inner.rect.width() - slider_width).abs() < 0.5);

            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(total_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
                |time_ui| {
                    time_ui.label(
                        egui::RichText::new(total)
                            .size(12.0)
                            .color(theme.secondary_text),
                    );
                },
            );

            if slider_response.inner.changed() && !guest {
                actions.push(FooterAction::Seek(position as f64));
            }
        },
    );
}

fn render_volume(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    padding: f32,
    volume: u8,
    theme: Theme,
    actions: &mut Vec<FooterAction>,
) {
    if rect.width() <= 0.0 {
        return;
    }

    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
        |ui| {
            ui.add_space(padding);
            ui.label(egui::RichText::new("Volume").size(12.0).color(theme.text));
            ui.add_space(padding * 0.5);
            let slider_width = (ui.available_width() - padding * 2.0).clamp(40.0, 140.0);
            let mut volume = volume;
            let visuals = ui.visuals_mut();
            visuals.widgets.inactive.bg_fill = theme.border;
            visuals.widgets.inactive.weak_bg_fill = theme.border;
            visuals.widgets.hovered.bg_fill = theme.secondary_text;
            visuals.widgets.active.bg_fill = theme.accent;
            visuals.slider_trailing_fill = true;
            let style = ui.style_mut();
            style.spacing.slider_width = slider_width;
            style.spacing.slider_rail_height = 6.0;

            if ui
                .add_sized(
                    egui::vec2(slider_width, 22.0),
                    egui::Slider::new(&mut volume, 0..=100).show_value(true),
                )
                .changed()
            {
                actions.push(FooterAction::SetVolume(volume));
            }
        },
    );
}
