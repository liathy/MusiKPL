//! A playlist's own screen: play it, pin it to Home, rename it, and manage
//! its songs: hover a song to remove it, drag it to change the order.

use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, Stroke, Ui};

use crate::{
    domain::{format_position, Cover, CoverArt, Playlist, Track},
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        responsive::ResponsiveUi,
        widgets::{
            self, elided, icon_button, item_id, mix, paint_icon_button, pill_button,
            pointer_amount, text_field, ButtonColors, FieldOptions, PillKind, PillSize,
        },
    },
};

#[derive(Debug, PartialEq)]
pub(crate) enum PlaylistAction {
    PlayAll,
    PlayFrom(usize),
    ToggleFavorite,
    /// Start renaming.
    Edit,
    /// Stop renaming, saving the name being typed.
    Done,
    Remove(usize),
    Move {
        from: usize,
        to: usize,
    },
    ToggleLike(Track),
    /// Choose a photo for the cover.
    ChangeCover,
    /// Go back to the default cover.
    RemoveCover,
    /// A suggested song: add it to the playlist.
    AddSuggestion(Track),
    /// A suggested song: play it.
    PlaySuggestion(Track),
    /// The suggestions were scrolled to their end.
    MoreSuggestions,
}

pub(crate) struct PlaylistView<'a> {
    pub(crate) playlist: &'a Playlist,
    /// The name is being edited.
    pub(crate) editing: bool,
    /// Jam guests request songs instead of playing them.
    pub(crate) guest: bool,
    pub(crate) covers: &'a CoverArt,
    pub(crate) liked: &'a dyn Fn(&str) -> bool,
    /// The song playing now, highlighted in the list.
    pub(crate) playing: Option<&'a str>,
    /// The photo the user chose, if any.
    pub(crate) cover: Option<Cover>,
    /// Songs to add, based on the ones already in it.
    pub(crate) suggestions: &'a [&'a Track],
    pub(crate) suggestions_loading: bool,
}

pub(crate) fn show(
    ui: &mut Ui,
    area: Rect,
    view: &PlaylistView<'_>,
    name_draft: &mut String,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<PlaylistAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
        ui.set_clip_rect(area);
        let output = egui::ScrollArea::vertical()
            .id_salt(("playlist_scroll", view.playlist.id))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(r.rem(1.5)))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        header(ui, view, name_draft, icons, &theme, r, &mut actions);
                        ui.add_space(r.rem(1.25));
                        if view.playlist.tracks.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(
                                        "This playlist is empty. Press + on any song, right-click it, or use the playlist button in the player bar to add it here.",
                                    )
                                    .font(fonts::body(r.rem(1.0)))
                                    .color(theme.secondary_text),
                                )
                                .wrap(),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("Drag songs to change the order")
                                    .font(fonts::body(r.rem(0.8125)))
                                    .color(theme.muted_text),
                            );
                        }
                        song_list(ui, view, icons, &theme, r, &mut actions);
                        suggestions(ui, view, icons, &theme, r, &mut actions);
                    });
            });
        let viewport = output.inner_rect.height();
        let content = output.content_size.y;
        let at_bottom =
            // A screen early, so the next batch is ready on arrival.
            content <= viewport || output.state.offset.y + viewport >= content - viewport;
        if at_bottom && !view.playlist.tracks.is_empty() && !view.suggestions_loading {
            actions.push(PlaylistAction::MoreSuggestions);
        }
    });
    actions
}

/// "Recommended songs": each with + to add it, and clicking plays it.
fn suggestions(
    ui: &mut Ui,
    view: &PlaylistView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<PlaylistAction>,
) {
    if view.playlist.tracks.is_empty() {
        return;
    }
    ui.add_space(r.rem(2.0));
    ui.label(
        egui::RichText::new("Recommended songs")
            .font(fonts::display(r.rem(1.5)))
            .color(theme.text),
    );
    ui.label(
        egui::RichText::new("Based on the songs in this playlist")
            .font(fonts::body(r.rem(0.875)))
            .color(theme.secondary_text),
    );
    ui.add_space(r.rem(0.5));
    ui.spacing_mut().item_spacing.y = r.rem(0.125);
    for track in view.suggestions {
        let (rect, row) =
            ui.allocate_exact_size(vec2(ui.available_width(), r.rem(4.0)), Sense::click());
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let hover = pointer_amount(ui, &row);
        ui.painter().rect_filled(
            rect,
            r.rem(0.625),
            mix(
                Color32::TRANSPARENT,
                Color32::from_rgba_unmultiplied(34, 34, 34, 153),
                hover,
            ),
        );
        let inner = rect.shrink2(r.rem2(0.75, 0.5));
        let cover_rect = Rect::from_min_size(
            pos2(inner.left() + r.rem(2.5), inner.center().y - r.rem(1.5)),
            r.rem2(3.0, 3.0),
        );
        widgets::cover(
            ui,
            cover_rect,
            view.covers.cover_for(track).as_ref(),
            icons,
            r.rem(0.375),
        );
        let add_rect = Rect::from_center_size(
            pos2(inner.right() - r.rem(2.5), inner.center().y),
            r.rem2(5.0, 2.0),
        );
        let text_rect = Rect::from_min_max(
            pos2(cover_rect.right() + r.rem(0.75), inner.top()),
            pos2(add_rect.left() - r.rem(5.0), inner.bottom()),
        );
        elided(
            ui.painter(),
            Rect::from_min_size(
                pos2(text_rect.left(), text_rect.center().y - r.rem(1.25)),
                vec2(text_rect.width(), r.rem(1.5)),
            ),
            &track.title,
            fonts::display(r.rem(1.0625)),
            theme.text,
            Align2::LEFT_CENTER,
        );
        elided(
            ui.painter(),
            Rect::from_min_size(
                pos2(text_rect.left(), text_rect.center().y + r.rem(0.25)),
                vec2(text_rect.width(), r.rem(1.125)),
            ),
            &track.channel,
            fonts::body(r.rem(0.8125)),
            theme.secondary_text,
            Align2::LEFT_CENTER,
        );
        ui.painter().text(
            pos2(add_rect.left() - r.rem(1.0), inner.center().y),
            Align2::RIGHT_CENTER,
            track
                .duration
                .map(format_position)
                .unwrap_or_else(|| "--:--".into()),
            fonts::body(r.rem(0.8125)),
            theme.secondary_text,
        );
        // "+ Add" pill.
        let add = ui.interact(add_rect, item_id("suggest_add", &track.id), Sense::click());
        let add_hover = widgets::hover_amount(ui, &add);
        ui.painter().rect_filled(
            add_rect,
            add_rect.height() / 2.0,
            mix(theme.surface, theme.text, add_hover),
        );
        ui.painter().text(
            add_rect.center(),
            Align2::CENTER_CENTER,
            "+ Add",
            fonts::display(r.rem(0.875)),
            mix(theme.text, Color32::BLACK, add_hover),
        );
        // Decided by where the pointer is, so + never plays the song.
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let on_add = pointer.is_some_and(|pointer| add_rect.contains(pointer));
        let clicked = row.clicked() || add.clicked();
        add.on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Add to this playlist");
        if clicked && on_add {
            actions.push(PlaylistAction::AddSuggestion((*track).clone()));
        } else if row
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(if view.guest {
                "Add to the Jam queue"
            } else {
                "Play"
            })
            .clicked()
        {
            actions.push(PlaylistAction::PlaySuggestion((*track).clone()));
        }
    }
    if view.suggestions_loading {
        ui.add_space(r.rem(0.5));
        ui.horizontal(|ui| {
            widgets::spinner(ui, r.rem(1.125));
            ui.label(
                egui::RichText::new("Finding songs for this playlist…")
                    .font(fonts::body(r.rem(0.875)))
                    .color(theme.secondary_text),
            );
        });
    }
}

/// The songs, with dragging to reorder: the dragged song dims, a line shows
/// where it will land, and letting go moves it there.
fn song_list(
    ui: &mut Ui,
    view: &PlaylistView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<PlaylistAction>,
) {
    ui.spacing_mut().item_spacing.y = r.rem(0.125);
    let drag_id = Id::new(("playlist_drag", view.playlist.id));
    let mut dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));
    let count = view.playlist.tracks.len();
    if dragging.is_some_and(|from| from >= count) {
        dragging = None;
    }
    let mut rows = Vec::with_capacity(count);
    for (index, track) in view.playlist.tracks.iter().enumerate() {
        let row = song_row(ui, index, track, view, dragging, icons, theme, r, actions);
        if row.drag_started() {
            dragging = Some(index);
        }
        rows.push(row.rect);
    }

    let Some(from) = dragging else {
        ui.data_mut(|data| data.remove::<usize>(drag_id));
        return;
    };
    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
    let pointer = ui.ctx().pointer_interact_pos();
    // Where it would land: the gap above the first row whose middle is
    // below the pointer.
    let slot = pointer.map_or(from, |pointer| {
        rows.iter()
            .filter(|rect| rect.center().y < pointer.y)
            .count()
    });
    let to = if slot > from { slot - 1 } else { slot };
    if to != from {
        let y = match slot {
            0 => rows[0].top() - r.rem(0.0625),
            slot => rows[slot - 1].bottom() + r.rem(0.0625),
        };
        let first = rows[0];
        ui.painter().line_segment(
            [pos2(first.left(), y), pos2(first.right(), y)],
            Stroke::new(r.rem(0.15), theme.text),
        );
    }
    if ui.input(|input| !input.pointer.any_down()) {
        if to != from {
            actions.push(PlaylistAction::Move { from, to });
        }
        ui.data_mut(|data| data.remove::<usize>(drag_id));
    } else {
        ui.data_mut(|data| data.insert_temp(drag_id, from));
        widgets::repaint_after(ui.ctx(), std::time::Duration::from_millis(16));
    }
}

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut Ui,
    view: &PlaylistView<'_>,
    name_draft: &mut String,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<PlaylistAction>,
) {
    let playlist = view.playlist;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(1.5);
        cover_picker(ui, view, icons, theme, r, actions);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = r.rem(0.5);
            ui.add_space(r.rem(0.75));
            if view.editing {
                let width = r.rem(26.0).min(ui.available_width());
                let field = text_field(
                    ui,
                    name_draft,
                    width,
                    FieldOptions {
                        id: "playlist_name",
                        hint: "Playlist name",
                        password: false,
                        enabled: true,
                    },
                    r,
                );
                if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    actions.push(PlaylistAction::Done);
                }
            } else {
                fonts::cover(ui.ctx(), &playlist.name);
                ui.label(
                    egui::RichText::new(&playlist.name)
                        .font(fonts::display(r.rem(2.25)))
                        .color(theme.text),
                );
            }
            let seconds: f64 = playlist.tracks.iter().filter_map(|t| t.duration).sum();
            let count = playlist.tracks.len();
            ui.label(
                egui::RichText::new(format!(
                    "{count} song{} · {}",
                    if count == 1 { "" } else { "s" },
                    total_length(seconds)
                ))
                .font(fonts::body(r.rem(0.9375)))
                .color(theme.secondary_text),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = r.rem(0.5);
                if view.editing {
                    if pill_button(ui, "Done", PillKind::Primary, PillSize::Medium, r).clicked() {
                        actions.push(PlaylistAction::Done);
                    }
                    return;
                }
                let can_play = !playlist.tracks.is_empty() && !view.guest;
                if pill_button(ui, "Play", PillKind::Primary, PillSize::Medium, r)
                    .on_hover_text(if view.guest {
                        "The host controls playback during a Jam"
                    } else {
                        "Play this playlist from the start"
                    })
                    .clicked()
                    && can_play
                {
                    actions.push(PlaylistAction::PlayAll);
                }
                let mut favorite = ButtonColors::round(theme);
                if playlist.favorite {
                    favorite.icon = theme.like;
                    favorite.hover_icon = theme.like;
                }
                if icon_button(
                    ui,
                    if playlist.favorite {
                        &icons.heart_filled
                    } else {
                        &icons.heart
                    },
                    r.rem2(2.5, 2.5),
                    r.rem2(1.25, 1.25),
                    r.rem(1.25),
                    favorite,
                )
                .on_hover_text(if playlist.favorite {
                    "Shown on Home. Click to remove it from Home"
                } else {
                    "Favorite: show this playlist on Home"
                })
                .clicked()
                {
                    actions.push(PlaylistAction::ToggleFavorite);
                }
                if icon_button(
                    ui,
                    &icons.edit,
                    r.rem2(2.5, 2.5),
                    r.rem2(1.125, 1.125),
                    r.rem(1.25),
                    ButtonColors::round(theme),
                )
                .on_hover_text("Rename")
                .clicked()
                {
                    actions.push(PlaylistAction::Edit);
                }
            });
        });
    });
}

/// The cover: the user's photo or the default playlist icon. Clicking it
/// chooses a photo; a chosen photo has an × to go back to the default.
fn cover_picker(
    ui: &mut Ui,
    view: &PlaylistView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<PlaylistAction>,
) {
    let (art, response) = ui.allocate_exact_size(r.rem2(9.0, 9.0), Sense::click());
    let rounding = r.rem(0.75);
    match &view.cover {
        Some(cover) => widgets::cover(ui, art, Some(cover), icons, rounding),
        None => default_cover(ui, art, rounding, icons, theme),
    }
    let hover = pointer_amount(ui, &response);
    if hover > 0.0 {
        ui.painter().rect_filled(
            art,
            rounding,
            Color32::from_black_alpha((150.0 * hover) as u8),
        );
        ui.painter().text(
            art.center(),
            Align2::CENTER_CENTER,
            if view.cover.is_some() {
                "Change photo"
            } else {
                "Choose photo"
            },
            fonts::display(r.rem(1.0)),
            Color32::WHITE.gamma_multiply(hover),
        );
    }
    let mut removed = false;
    if view.cover.is_some() && response.contains_pointer() {
        let size = r.rem(2.0);
        let close = Rect::from_min_size(
            pos2(art.right() - size - r.rem(0.375), art.top() + r.rem(0.375)),
            vec2(size, size),
        );
        let button = ui.interact(close, item_id("cover_remove", ""), Sense::click());
        let mut colors = ButtonColors::round(theme);
        colors.fill = Color32::from_black_alpha(178);
        paint_icon_button(
            ui,
            close,
            &button,
            &icons.close,
            r.rem2(0.875, 0.875),
            size / 2.0,
            colors,
        );
        removed = button
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Use the default cover")
            .clicked();
        if removed {
            actions.push(PlaylistAction::RemoveCover);
        }
    }
    if !removed
        && response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Choose a photo for this playlist")
            .clicked()
    {
        actions.push(PlaylistAction::ChangeCover);
    }
}

/// The default playlist cover: the playlist icon on grey.
pub(crate) fn default_cover(
    ui: &Ui,
    rect: Rect,
    rounding: f32,
    icons: &IconTextures,
    theme: &Theme,
) {
    ui.painter().rect_filled(rect, rounding, theme.surface);
    crate::ui::icons::paint_icon(
        ui.painter(),
        &icons.playlist,
        Rect::from_center_size(rect.center(), rect.size() * 0.4),
        theme.secondary_text,
    );
}

/// "42 min" or "1 h 5 min".
fn total_length(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// One song: number (a drag grip on hover), cover, title and artist, ♥,
/// and its length, which turns into a remove button on hover. Returns the
/// row's response, which senses drags for reordering.
#[allow(clippy::too_many_arguments)]
fn song_row(
    ui: &mut Ui,
    index: usize,
    track: &Track,
    view: &PlaylistView<'_>,
    dragging: Option<usize>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<PlaylistAction>,
) -> egui::Response {
    let (rect, row) = ui.allocate_exact_size(
        vec2(ui.available_width(), r.rem(4.0)),
        Sense::click_and_drag(),
    );
    if !ui.is_rect_visible(rect) {
        return row;
    }
    let dragged = dragging == Some(index);
    let hovered = dragging.is_none() && row.contains_pointer();
    let hover = if dragging.is_some() {
        0.0
    } else {
        pointer_amount(ui, &row)
    };
    let playing = view.playing == Some(track.id.as_str());
    let base = if playing || dragged {
        theme.surface
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(
        rect,
        r.rem(0.625),
        mix(
            base,
            Color32::from_rgba_unmultiplied(34, 34, 34, 153),
            if playing { 0.0 } else { hover },
        ),
    );
    let fade = if dragged { 0.4 } else { 1.0 };
    let inner = rect.shrink2(r.rem2(0.75, 0.5));
    let number_center = pos2(inner.left() + r.rem(1.0), inner.center().y);
    if hovered || dragged {
        grip(ui, number_center, theme.secondary_text, r);
    } else {
        ui.painter().text(
            number_center,
            Align2::CENTER_CENTER,
            (index + 1).to_string(),
            fonts::body(r.rem(0.9375)),
            if playing {
                theme.text
            } else {
                theme.muted_text
            },
        );
    }
    let cover_rect = Rect::from_min_size(
        pos2(inner.left() + r.rem(2.5), inner.center().y - r.rem(1.5)),
        r.rem2(3.0, 3.0),
    );
    widgets::cover(
        ui,
        cover_rect,
        view.covers.cover_for(track).as_ref(),
        icons,
        r.rem(0.375),
    );

    let button = r.rem2(2.0, 2.0);
    let text_rect = Rect::from_min_max(
        pos2(cover_rect.right() + r.rem(0.75), inner.top()),
        pos2(inner.right() - button.x * 2.0 - r.rem(1.0), inner.bottom()),
    );
    elided(
        ui.painter(),
        Rect::from_min_size(
            pos2(text_rect.left(), text_rect.center().y - r.rem(1.25)),
            vec2(text_rect.width(), r.rem(1.5)),
        ),
        &track.title,
        fonts::display(r.rem(1.0625)),
        theme.text.gamma_multiply(fade),
        Align2::LEFT_CENTER,
    );
    elided(
        ui.painter(),
        Rect::from_min_size(
            pos2(text_rect.left(), text_rect.center().y + r.rem(0.25)),
            vec2(text_rect.width(), r.rem(1.125)),
        ),
        &track.channel,
        fonts::body(r.rem(0.8125)),
        theme.secondary_text.gamma_multiply(fade),
        Align2::LEFT_CENTER,
    );

    // Trailing buttons, right to left: remove (on hover, in place of the
    // length), then ♥.
    let slot = |index: usize| {
        Rect::from_min_size(
            pos2(
                inner.right() - button.x * (index as f32 + 1.0) - r.rem(0.25) * index as f32,
                inner.center().y - button.y / 2.0,
            ),
            button,
        )
    };
    let mut button_clicked = false;
    let mut round_button = |ui: &mut Ui,
                            rect: Rect,
                            salt: &str,
                            icon: &egui::TextureHandle,
                            tint: Option<Color32>,
                            tooltip: &str|
     -> bool {
        let response = ui.interact(
            rect,
            item_id(salt, &format!("{index}:{}", track.id)),
            Sense::click(),
        );
        let mut colors = ButtonColors::round(theme);
        colors.fill = Color32::TRANSPARENT;
        if let Some(tint) = tint {
            colors.icon = tint;
            colors.hover_icon = tint;
        }
        paint_icon_button(
            ui,
            rect,
            &response,
            icon,
            r.rem2(1.0, 1.0),
            button.x / 2.0,
            colors,
        );
        let clicked = response
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(tooltip)
            .clicked();
        button_clicked |= clicked;
        clicked
    };
    if hovered {
        if round_button(
            ui,
            slot(0),
            "pl_remove",
            &icons.close,
            None,
            "Remove from playlist",
        ) {
            actions.push(PlaylistAction::Remove(index));
        }
    } else {
        ui.painter().text(
            pos2(inner.right(), inner.center().y),
            Align2::RIGHT_CENTER,
            track
                .duration
                .map(format_position)
                .unwrap_or_else(|| "--:--".into()),
            fonts::body(r.rem(0.8125)),
            theme.secondary_text.gamma_multiply(fade),
        );
    }
    let liked = (view.liked)(&track.id);
    if round_button(
        ui,
        slot(1),
        "pl_like",
        if liked {
            &icons.heart_filled
        } else {
            &icons.heart
        },
        liked.then_some(theme.like),
        if liked { "Unlike" } else { "Like" },
    ) {
        actions.push(PlaylistAction::ToggleLike(track.clone()));
    }

    let row = if dragging.is_none() {
        row.on_hover_cursor(CursorIcon::Grab)
            .on_hover_text(if view.guest {
                "Drag to change the order"
            } else {
                "Click to play from here, drag to change the order"
            })
    } else {
        row
    };
    if !view.guest && !button_clicked && row.clicked() {
        actions.push(PlaylistAction::PlayFrom(index));
    }
    row
}

/// Six dots: a handle that says "drag me".
fn grip(ui: &Ui, center: egui::Pos2, color: Color32, r: &ResponsiveUi) {
    let gap = r.rem(0.3125);
    for column in [-0.5_f32, 0.5] {
        for row in [-1.0_f32, 0.0, 1.0] {
            ui.painter()
                .circle_filled(center + vec2(column * gap, row * gap), r.rem(0.09), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_length_reads_naturally() {
        assert_eq!(total_length(0.0), "0 min");
        assert_eq!(total_length(42.0 * 60.0), "42 min");
        assert_eq!(total_length(65.0 * 60.0 + 20.0), "1 h 5 min");
    }
}
