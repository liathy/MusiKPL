use crate::{
    domain::{format_duration, format_position, Track},
    theme::Theme,
    ui::{icons::*, layout::*, responsive::ResponsiveUi},
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
    responsive: &ResponsiveUi,
) -> Vec<FooterAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    egui::TopBottomPanel::bottom("app_footer")
        .frame(
            egui::Frame::none()
                .fill(egui::Color32::BLACK)
                .inner_margin(0.0),
        )
        .exact_height(responsive.player_footer_height)
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
                    let geometry = footer_geometry(footer_rect, responsive);
                    let padding = footer_padding(footer_rect.width(), responsive);

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
                                responsive,
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
                                responsive,
                                &mut actions,
                            );
                            render_volume(
                                ui,
                                geometry.right,
                                padding,
                                state.volume,
                                theme,
                                responsive,
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
    responsive: &ResponsiveUi,
) {
    if rect.width() <= 0.0 {
        return;
    }

    let (title, subtitle) = match current {
        Some(track) => (track.title.as_str(), track.channel.as_str()),
        None => ("Nothing playing", "Search for music to begin"),
    };
    let title_font = egui::FontId::proportional(responsive.font_body_emphasis);
    let subtitle_font = egui::TextStyle::Small.resolve(ui.style());
    let line_gap = responsive.space_xs;
    let text_height = ui
        .fonts(|fonts| fonts.row_height(&title_font) + line_gap + fonts.row_height(&subtitle_font));
    // Both rectangles are derived from the footer's own center line, and the
    // text is confined to what is left of the column so it truncates instead
    // of running under the playback controls.
    let info = footer_track_info(
        rect.shrink2(egui::vec2(padding, 0.0)),
        responsive.icon_size,
        ui.spacing().item_spacing.x,
        text_height,
    );

    ui.put(
        info.icon,
        egui::Image::from_texture((icons.music.id(), info.icon.size()))
            .fit_to_exact_size(info.icon.size())
            .tint(theme.accent),
    );
    if info.text.width() <= 0.0 {
        return;
    }
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(info.text)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.spacing_mut().item_spacing.y = line_gap;
            ui.add(
                egui::Label::new(
                    egui::RichText::new(title)
                        .font(title_font)
                        .color(theme.text),
                )
                .truncate(),
            );
            ui.add(
                egui::Label::new(
                    egui::RichText::new(subtitle)
                        .font(subtitle_font)
                        .color(theme.secondary_text),
                )
                .truncate(),
            );
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
    responsive: &ResponsiveUi,
    actions: &mut Vec<FooterAction>,
) {
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(playback_rect)
            .layout(egui::Layout::top_down(egui::Align::Center)),
        |ui| {
            let rows = footer_playback_rows(playback_rect, responsive);
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

            // Each button gets an explicit rectangle on the row's center line.
            // A single-pass horizontal row cannot re-center earlier buttons once
            // the taller play button grows it, which left them at different heights.
            let button_padding = ui.spacing().button_padding;
            let side_icon = responsive.icon_size;
            let play_icon_size = responsive.icon_size + 2.0;
            let side_size = egui::Vec2::splat(responsive.button_min_size.x.clamp(28.0, 32.0))
                .max(egui::Vec2::splat(side_icon) + button_padding * 2.0);
            let play_size = egui::vec2(
                responsive.control_height.clamp(32.0, 40.0),
                responsive.control_height.clamp(30.0, 40.0),
            )
            .max(egui::Vec2::splat(play_icon_size) + button_padding * 2.0);
            let buttons = footer_transport_buttons(
                rows.transport.center(),
                side_size,
                play_size,
                ui.spacing().item_spacing.x,
            );

            if !guest
                && ui
                    .put(
                        buttons.previous,
                        egui::Button::image(design_icon(&icons.previous, side_icon)),
                    )
                    .on_hover_text("Previous")
                    .clicked()
            {
                actions.push(FooterAction::Previous);
            }

            let play_icon = if paused { &icons.play } else { &icons.pause };
            if ui
                .put(buttons.play, |ui: &mut egui::Ui| {
                    ui.add_enabled(
                        !guest,
                        egui::Button::image(design_icon(play_icon, play_icon_size)),
                    )
                })
                .clicked()
            {
                actions.push(FooterAction::TogglePause);
            }

            if !guest
                && ui
                    .put(
                        buttons.next,
                        egui::Button::image(design_icon(&icons.next, side_icon)),
                    )
                    .on_hover_text("Next")
                    .clicked()
            {
                actions.push(FooterAction::Next);
            }
            let elapsed = format_position(position as f64);
            let total = format_duration(duration);
            let (elapsed_width, total_width) = ui.fonts(|fonts| {
                let font = egui::FontId::proportional(responsive.font_body);
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
            let content_height = responsive
                .control_height
                .mul_add(0.5, 0.0)
                .clamp(18.0, 24.0);
            let content_top = rows.progress.center().y - content_height / 2.0;
            let elapsed_rect = egui::Rect::from_min_size(
                egui::pos2(content_left, content_top),
                egui::vec2(elapsed_width, content_height),
            );
            let slider_rect = egui::Rect::from_min_size(
                egui::pos2(elapsed_rect.right() + gap, content_top),
                egui::vec2(slider_width, content_height),
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
                            .size(responsive.font_body)
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
                    style.spacing.slider_rail_height = responsive.space_xs.clamp(4.0, 6.0);

                    slider_ui.add_sized(
                        egui::vec2(slider_width, content_height),
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
                            .size(responsive.font_body)
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
    responsive: &ResponsiveUi,
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
            ui.label(
                egui::RichText::new("Volume")
                    .size(responsive.font_body)
                    .color(theme.text),
            );
            ui.add_space(padding * 0.5);
            let slider_width = (ui.available_width() - padding * 2.0)
                .clamp(responsive.rem(2.5), responsive.rem(8.75));
            let mut volume = volume;
            let visuals = ui.visuals_mut();
            visuals.widgets.inactive.bg_fill = theme.border;
            visuals.widgets.inactive.weak_bg_fill = theme.border;
            visuals.widgets.hovered.bg_fill = theme.secondary_text;
            visuals.widgets.active.bg_fill = theme.accent;
            visuals.slider_trailing_fill = true;
            let style = ui.style_mut();
            style.spacing.slider_width = slider_width;
            style.spacing.slider_rail_height = responsive.space_xs.clamp(4.0, 6.0);

            if ui
                .add_sized(
                    egui::vec2(slider_width, responsive.control_height.clamp(22.0, 32.0)),
                    egui::Slider::new(&mut volume, 0..=100).show_value(true),
                )
                .changed()
            {
                actions.push(FooterAction::SetVolume(volume));
            }
        },
    );
}
