//! The player bar: current song, playback controls, volume, and the queue
//! button, in three equal columns.

use eframe::egui::{self, pos2, vec2, Align2, CursorIcon, Id, Rect, Sense, Ui};

use crate::{
    domain::{format_position, Cover, Playlists, Track},
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        layout::{footer_columns, transport_buttons},
        menu::{add_to_playlist_menu, PlaylistTarget},
        responsive::ResponsiveUi,
        widgets::{self, marquee, paint_icon_button, slider, ButtonColors},
    },
};

#[derive(Debug, PartialEq)]
pub(crate) enum FooterAction {
    Previous,
    TogglePause,
    Next,
    Seek(f64),
    SetVolume(u8),
    ToggleQueue,
    ToggleLike,
    AddToPlaylist(PlaylistTarget),
    ToggleMute,
    /// The artist's name was clicked: open their Last.fm page.
    OpenArtist,
}

pub(crate) struct FooterState<'a> {
    /// The song playing, or being loaded.
    pub(crate) track: Option<&'a Track>,
    pub(crate) loading: bool,
    pub(crate) cover: Option<Cover>,
    pub(crate) paused: bool,
    pub(crate) position: f64,
    pub(crate) duration: f64,
    pub(crate) volume: u8,
    /// Jam guests follow the host and cannot control playback.
    pub(crate) guest: bool,
    pub(crate) queue_open: bool,
    pub(crate) liked: bool,
    pub(crate) playlists: &'a Playlists,
}

pub(crate) fn show(
    ui: &mut Ui,
    footer: Rect,
    icons: &IconTextures,
    state: &FooterState<'_>,
    r: &ResponsiveUi,
) -> Vec<FooterAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.painter().line_segment(
        [footer.left_top(), footer.right_top()],
        egui::Stroke::new(1.0_f32, theme.border),
    );
    let [info, playback, volume] = footer_columns(footer);
    track_info(ui, info, icons, state, &theme, r, &mut actions);
    controls(ui, playback, icons, state, &theme, r, &mut actions);
    volume_and_queue(ui, volume, icons, state, &theme, r, &mut actions);
    actions
}

fn track_info(
    ui: &mut Ui,
    column: Rect,
    icons: &IconTextures,
    state: &FooterState<'_>,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<FooterAction>,
) {
    let content = column.shrink2(vec2(r.rem(1.5), 0.0));
    let cover_size = r.rem(3.875);
    let cover_rect = Rect::from_min_size(
        pos2(content.left(), content.center().y - cover_size / 2.0),
        egui::Vec2::splat(cover_size),
    );
    widgets::cover(ui, cover_rect, state.cover.as_ref(), icons, r.rem(0.5));

    // ♥ Like, at the right of the column.
    let heart_size = r.rem(2.25);
    let heart_rect = Rect::from_center_size(
        pos2(content.right() - heart_size / 2.0, content.center().y),
        egui::Vec2::splat(heart_size),
    );
    let text_right = if state.track.is_some() {
        let heart = ui.interact(heart_rect, Id::new("footer_like"), Sense::click());
        let mut colors = ButtonColors::ghost(theme);
        if state.liked {
            colors.icon = theme.like;
            colors.hover_icon = theme.like;
        } else {
            colors.hover_icon = theme.like;
        }
        paint_icon_button(
            ui,
            heart_rect,
            &heart,
            if state.liked {
                &icons.heart_filled
            } else {
                &icons.heart
            },
            r.rem2(1.25, 1.25),
            heart_size / 2.0,
            colors,
        );
        if heart
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(if state.liked { "Unlike" } else { "Like" })
            .clicked()
        {
            actions.push(FooterAction::ToggleLike);
        }
        // Add to playlist, left of the heart.
        let add_rect = heart_rect.translate(egui::vec2(-(heart_size + r.rem(0.25)), 0.0));
        let add = ui.interact(add_rect, Id::new("footer_add_to_playlist"), Sense::click());
        paint_icon_button(
            ui,
            add_rect,
            &add,
            &icons.playlist_add,
            r.rem2(1.25, 1.25),
            heart_size / 2.0,
            ButtonColors::ghost(theme),
        );
        let popup = Id::new("footer_playlist_popup");
        let add = add
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Add to playlist");
        if add.clicked() {
            ui.memory_mut(|memory| memory.toggle_popup(popup));
        }
        if let Some(track) = state.track {
            let picked = egui::popup::popup_above_or_below_widget(
                ui,
                popup,
                &add,
                egui::AboveOrBelow::Above,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| add_to_playlist_menu(ui, state.playlists, &track.id, r),
            )
            .flatten();
            if let Some(target) = picked {
                ui.memory_mut(|memory| memory.close_popup());
                actions.push(FooterAction::AddToPlaylist(target));
            }
        }
        add_rect.left() - r.rem(0.5)
    } else {
        content.right()
    };

    let text_left = cover_rect.right() + r.rem(0.625);
    let title_height = r.rem(2.3125);
    let subtitle_height = r.rem(1.375);
    let top = content.center().y - (title_height + subtitle_height) / 2.0;
    let title_rect = Rect::from_min_max(
        pos2(text_left, top),
        pos2(text_right.max(text_left), top + title_height),
    );
    let subtitle_rect = Rect::from_min_size(
        pos2(text_left, title_rect.bottom()),
        vec2(title_rect.width(), subtitle_height),
    );
    let (title, subtitle, subtitle_color) = match state.track {
        Some(track) if state.loading => (track.title.as_str(), "Loading…", theme.secondary_text),
        Some(track) => (track.title.as_str(), track.channel.as_str(), theme.text),
        None => (
            "Nothing playing",
            "Search for music to begin",
            theme.secondary_text,
        ),
    };
    marquee(
        ui,
        title_rect,
        title,
        fonts::display(r.rem(1.5)),
        theme.text,
        r,
    );
    // The artist's name opens their Last.fm page.
    let mut subtitle_color = subtitle_color;
    if state.track.is_some() && !state.loading {
        let font = fonts::body(r.rem(0.875));
        let width = ui
            .painter()
            .layout_no_wrap(subtitle.to_owned(), font, subtitle_color)
            .size()
            .x
            .min(subtitle_rect.width());
        let name_rect = Rect::from_min_size(subtitle_rect.min, vec2(width, subtitle_rect.height()));
        let name = ui.interact(name_rect, Id::new("footer_artist"), Sense::click());
        if name.hovered() {
            subtitle_color = theme.secondary_text;
            ui.painter().line_segment(
                [
                    pos2(name_rect.left(), name_rect.center().y + r.rem(0.5)),
                    pos2(name_rect.right(), name_rect.center().y + r.rem(0.5)),
                ],
                egui::Stroke::new(1.0_f32, subtitle_color),
            );
        }
        if name
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Open artist")
            .clicked()
        {
            actions.push(FooterAction::OpenArtist);
        }
    }
    marquee(
        ui,
        subtitle_rect,
        subtitle,
        fonts::body(r.rem(0.875)),
        subtitle_color,
        r,
    );
}

fn controls(
    ui: &mut Ui,
    column: Rect,
    icons: &IconTextures,
    state: &FooterState<'_>,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<FooterAction>,
) {
    let size = r.rem2(2.5, 2.5);
    let buttons = transport_buttons(
        pos2(column.center().x, column.center().y - r.rem(1.03)),
        size,
        r.rem(0.5),
    );
    let enabled = !state.guest;
    let button = |ui: &mut Ui, id: &str, rect: Rect| {
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        ui.interact(rect, Id::new(id), sense)
    };

    let previous = button(ui, "footer_previous", buttons.previous);
    let play = button(ui, "footer_play", buttons.play);
    let next = button(ui, "footer_next", buttons.next);
    let mut ghost = ButtonColors::ghost(theme);
    let mut primary = ButtonColors::primary(theme);
    if !enabled {
        ghost.hover_fill = ghost.fill;
        ghost.icon = theme.muted_text;
        ghost.hover_icon = theme.muted_text;
        primary.hover_fill = primary.fill;
        primary.icon = theme.muted_text;
        primary.hover_icon = theme.muted_text;
    }
    let round = size.x / 2.0;
    paint_icon_button(
        ui,
        buttons.previous,
        &previous,
        &icons.previous,
        r.rem2(1.5, 1.375),
        round,
        ghost,
    );
    paint_icon_button(
        ui,
        buttons.next,
        &next,
        &icons.next,
        r.rem2(1.5, 1.375),
        round,
        ghost,
    );
    // The play triangle is nudged right so it looks centered.
    let (icon, icon_size, nudge) = if state.paused || state.track.is_none() {
        (&icons.play, r.rem2(1.0625, 1.2875), r.rem(0.15625))
    } else {
        (&icons.pause, r.rem2(1.25, 1.25), 0.0)
    };
    let t = widgets::hover_amount(ui, &play);
    ui.painter().rect_filled(
        buttons.play,
        round,
        widgets::mix(primary.fill, primary.hover_fill, t),
    );
    crate::ui::icons::paint_icon(
        ui.painter(),
        icon,
        Rect::from_center_size(buttons.play.center() + vec2(nudge, 0.0), icon_size),
        widgets::mix(primary.icon, primary.hover_icon, t),
    );

    if enabled {
        if previous.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            actions.push(FooterAction::Previous);
        }
        if play.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            actions.push(FooterAction::TogglePause);
        }
        if next.on_hover_cursor(CursorIcon::PointingHand).clicked() {
            actions.push(FooterAction::Next);
        }
    }

    // Elapsed time, seek bar, total time.
    let bar_width = column.width() - r.rem(1.25);
    let bar = Rect::from_center_size(
        pos2(column.center().x, column.center().y + r.rem(1.5625)),
        vec2(bar_width, r.rem(1.4375)),
    );
    let time_font = fonts::body(r.rem(0.875));
    let has_duration = state.duration > 0.0;
    let elapsed = format_position(if has_duration { state.position } else { 0.0 });
    let total = if has_duration {
        format_position(state.duration)
    } else {
        "--:--".into()
    };
    let gap = r.rem(0.75);
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), time_font.clone(), theme.secondary_text)
            .size()
            .x
    };
    let time_width = measure("00:00").max(measure(&elapsed)).max(measure(&total));
    let painter = ui.painter();
    painter.text(
        pos2(bar.left() + time_width, bar.center().y),
        Align2::RIGHT_CENTER,
        &elapsed,
        time_font.clone(),
        theme.secondary_text,
    );
    painter.text(
        pos2(bar.right() - time_width, bar.center().y),
        Align2::LEFT_CENTER,
        &total,
        time_font,
        theme.secondary_text,
    );
    let slider_rect = Rect::from_min_max(
        pos2(bar.left() + time_width + gap, bar.top()),
        pos2(bar.right() - time_width - gap, bar.bottom()),
    );
    let mut position = if has_duration {
        state.position.min(state.duration) as f32
    } else {
        0.0
    };
    let seek = ui
        .allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(slider_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                slider(
                    ui,
                    slider_rect.width(),
                    &mut position,
                    0.0..=(state.duration.max(1.0) as f32),
                    enabled && has_duration,
                    false,
                    r,
                )
            },
        )
        .inner;
    if seek.changed() {
        actions.push(FooterAction::Seek(f64::from(position)));
    }
}

fn volume_and_queue(
    ui: &mut Ui,
    column: Rect,
    icons: &IconTextures,
    state: &FooterState<'_>,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<FooterAction>,
) {
    let group_width = r.rem(11.125);
    let queue_size = r.rem(3.75);
    let gap = r.rem(1.5625);
    // Against the right edge, mirroring the song info on the left.
    let left = column.right() - r.rem(1.5) - queue_size - gap - group_width;
    let center_y = column.center().y;

    // Speaker icon (its waves follow the level; click to mute) and slider,
    // like Spotify.
    let icon_size = r.rem(2.25);
    let slider_width = group_width - icon_size - r.rem(0.5);
    let icon_rect = Rect::from_center_size(
        pos2(left + icon_size / 2.0, center_y),
        egui::Vec2::splat(icon_size),
    );
    let speaker = ui.interact(icon_rect, Id::new("footer_mute"), Sense::click());
    let (speaker_icon, tooltip) = match state.volume {
        0 => (&icons.volume_off, "Unmute"),
        1..=49 => (&icons.volume_low, "Mute"),
        _ => (&icons.volume_high, "Mute"),
    };
    paint_icon_button(
        ui,
        icon_rect,
        &speaker,
        speaker_icon,
        r.rem2(1.375, 1.375),
        icon_size / 2.0,
        ButtonColors::ghost(theme),
    );
    if speaker
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tooltip)
        .clicked()
    {
        actions.push(FooterAction::ToggleMute);
    }
    let slider_rect = Rect::from_min_size(
        pos2(icon_rect.right() + r.rem(0.5), center_y - r.rem(0.5)),
        vec2(slider_width, r.rem(1.0)),
    );
    let mut volume = f32::from(state.volume);
    let changed = ui
        .allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(slider_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| slider(ui, slider_width, &mut volume, 0.0..=100.0, true, true, r),
        )
        .inner
        .changed();
    if changed {
        actions.push(FooterAction::SetVolume(
            volume.round().clamp(0.0, 100.0) as u8
        ));
    }
    // Queue button.
    let queue_rect = Rect::from_center_size(
        pos2(left + group_width + gap + queue_size / 2.0, center_y),
        egui::Vec2::splat(queue_size),
    );
    let queue = ui.interact(queue_rect, Id::new("footer_queue"), Sense::click());
    let mut colors = ButtonColors::ghost(theme);
    if state.queue_open {
        colors.fill = theme.surface;
    }
    paint_icon_button(
        ui,
        queue_rect,
        &queue,
        &icons.queue,
        r.rem2(3.0, 2.1875),
        r.rem(0.875),
        colors,
    );
    if queue
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Queue")
        .clicked()
    {
        actions.push(FooterAction::ToggleQueue);
    }
}
