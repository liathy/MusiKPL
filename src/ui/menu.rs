//! Pieces of the menu: cards that expand and collapse, the playlist list,
//! and the hold-to-delete button.

use std::time::Duration;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, Stroke, TextureHandle, Ui,
};

use crate::{
    domain::Cover,
    theme::Theme,
    ui::{
        fonts,
        icons::{paint_icon, IconTextures},
        responsive::ResponsiveUi,
        widgets::{self, elided, hover_amount, item_id, mix, pointer_amount, repaint_after},
    },
};

/// How long the delete button must be held.
pub(crate) const HOLD_TO_DELETE: f64 = 3.0;

/// A card with a clickable header that shows or hides its body. Returns
/// whether the header (not one of `extra` buttons) was clicked, and the
/// index of a clicked `extra` button.
pub(crate) struct CardResult {
    pub(crate) toggled: bool,
    pub(crate) extra_clicked: Option<usize>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn section_card(
    ui: &mut Ui,
    id: &str,
    icon: &TextureHandle,
    title: &str,
    summary: &str,
    expanded: bool,
    extra: &[(&TextureHandle, &str)],
    icons: &IconTextures,
    r: &ResponsiveUi,
    body: impl FnOnce(&mut Ui),
) -> CardResult {
    let theme = Theme::default();
    let mut result = CardResult {
        toggled: false,
        extra_clicked: None,
    };
    egui::Frame::none()
        .fill(Color32::BLACK)
        .stroke(Stroke::new(1.0_f32, theme.border))
        .rounding(r.rem(0.75))
        .inner_margin(egui::Margin::same(r.rem(0.75)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (header, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), r.rem(2.5)), Sense::click());
            let hover = hover_amount(ui, &response);
            ui.painter().rect_filled(
                header.expand(r.rem(0.25)),
                r.rem(0.5),
                mix(Color32::TRANSPARENT, theme.panel, hover),
            );
            paint_icon(
                ui.painter(),
                icon,
                Rect::from_center_size(
                    pos2(header.left() + r.rem(0.75), header.center().y),
                    r.rem2(1.125, 1.125),
                ),
                theme.icon,
            );

            // Chevron on the right, pointing down when open.
            let chevron = Rect::from_center_size(
                pos2(header.right() - r.rem(0.75), header.center().y),
                r.rem2(1.0, 1.0),
            );
            let turn = ui
                .ctx()
                .animate_bool_with_time(Id::new(("card_open", id)), expanded, 0.15);
            egui::Image::new(icon_source(&icons.chevron))
                .rotate(std::f32::consts::PI * (turn - 1.0) / 2.0, vec2(0.5, 0.5))
                .tint(theme.secondary_text)
                .paint_at(ui, chevron);

            // Extra round buttons left of the chevron.
            let mut right = chevron.left() - r.rem(0.5);
            for (index, (texture, tooltip)) in extra.iter().enumerate() {
                let size = r.rem(2.0);
                let rect = Rect::from_min_size(
                    pos2(right - size, header.center().y - size / 2.0),
                    vec2(size, size),
                );
                right = rect.left() - r.rem(0.25);
                let button = ui.interact(rect, Id::new(("card_extra", id, index)), Sense::click());
                let t = hover_amount(ui, &button);
                ui.painter().circle_filled(
                    rect.center(),
                    size / 2.0,
                    mix(theme.surface, theme.hover, t),
                );
                paint_icon(
                    ui.painter(),
                    texture,
                    Rect::from_center_size(rect.center(), r.rem2(1.0, 1.0)),
                    theme.text,
                );
                if button
                    .on_hover_cursor(CursorIcon::PointingHand)
                    .on_hover_text(*tooltip)
                    .clicked()
                {
                    result.extra_clicked = Some(index);
                }
            }

            let text_left = header.left() + r.rem(1.75);
            let text_rect = Rect::from_min_max(
                pos2(text_left, header.top()),
                pos2(right - r.rem(0.25), header.bottom()),
            );
            let title_galley = ui.painter().layout_no_wrap(
                title.to_owned(),
                fonts::display(r.rem(1.25)),
                theme.text,
            );
            let title_width = title_galley.size().x.min(text_rect.width());
            ui.painter().galley(
                pos2(
                    text_rect.left(),
                    text_rect.center().y - title_galley.size().y / 2.0,
                ),
                title_galley,
                theme.text,
            );
            let summary_rect = Rect::from_min_max(
                pos2(text_rect.left() + title_width + r.rem(0.5), text_rect.top()),
                text_rect.max,
            );
            if summary_rect.width() > r.rem(1.0) {
                elided(
                    ui.painter(),
                    summary_rect,
                    summary,
                    fonts::body(r.rem(0.8125)),
                    theme.muted_text,
                    Align2::LEFT_CENTER,
                );
            }
            if result.extra_clicked.is_none()
                && response.on_hover_cursor(CursorIcon::PointingHand).clicked()
            {
                result.toggled = true;
            }
            if expanded {
                ui.add_space(r.rem(0.5));
                body(ui);
            }
        });
    result
}

pub(crate) use musikpl_core::domain::PlaylistTarget;

/// The "Add to playlist" list, for right-click menus and the player bar.
/// Playlists that already have the song show a check and can't be picked.
/// Which songs are downloaded or downloading, shared through egui's data
/// for the song menus.
#[derive(Clone, Default)]
pub(crate) struct DownloadMarks(
    pub(crate) std::sync::Arc<std::collections::HashMap<String, musikpl_core::engine::DownloadMark>>,
);

pub(crate) const DOWNLOAD_MARKS: &str = "download_marks";

pub(crate) fn add_to_playlist_menu(
    ui: &mut Ui,
    playlists: &crate::domain::Playlists,
    track_id: &str,
    r: &ResponsiveUi,
) -> Option<PlaylistTarget> {
    let theme = Theme::default();
    let mut picked = None;
    ui.set_min_width(r.rem(14.0));
    // Download, from the same menu (the app shares which songs are
    // downloaded each frame).
    if !track_id.is_empty() {
        let marks = ui
            .ctx()
            .data(|data| data.get_temp::<DownloadMarks>(egui::Id::new(DOWNLOAD_MARKS)))
            .unwrap_or_default();
        let label = match marks.0.get(track_id) {
            Some(musikpl_core::engine::DownloadMark::Done) => "✓ Downloaded (remove)".to_owned(),
            Some(musikpl_core::engine::DownloadMark::Downloading(done)) => {
                format!("Downloading {}% (cancel)", done / 10)
            }
            Some(_) => "Waiting to download (cancel)".to_owned(),
            None => "Download".to_owned(),
        };
        if ui
            .add(
                egui::Button::new(egui::RichText::new(label).font(fonts::body(r.rem(0.9375))))
                    .frame(false),
            )
            .clicked()
        {
            picked = Some(PlaylistTarget::Download);
        }
        ui.separator();
    }
    ui.label(
        egui::RichText::new("Add to playlist")
            .font(fonts::body(r.rem(0.8125)))
            .color(theme.muted_text),
    );
    for playlist in playlists.iter() {
        let has = playlists.contains(playlist.id, track_id);
        let label = if has {
            format!("✓ {}", playlist.name)
        } else {
            playlist.name.clone()
        };
        let button = ui.add_enabled(
            !has,
            egui::Button::new(egui::RichText::new(label).font(fonts::body(r.rem(0.9375))))
                .frame(false),
        );
        if button.clicked() {
            picked = Some(PlaylistTarget::Existing(playlist.id));
        }
    }
    ui.separator();
    if ui
        .add(
            egui::Button::new(
                egui::RichText::new("+ New playlist").font(fonts::body(r.rem(0.9375))),
            )
            .frame(false),
        )
        .clicked()
    {
        picked = Some(PlaylistTarget::New);
    }
    if picked.is_some() {
        ui.close_menu();
    }
    picked
}

fn icon_source(texture: &TextureHandle) -> egui::ImageSource<'static> {
    egui::ImageSource::Texture(egui::load::SizedTexture::from_handle(texture))
}

/// One playlist in the menu.
pub(crate) struct PlaylistItem<'a> {
    pub(crate) id: u64,
    pub(crate) name: &'a str,
    pub(crate) songs: usize,
    pub(crate) favorite: bool,
    /// Its screen is open.
    pub(crate) selected: bool,
    /// The photo the user chose; None shows the default icon.
    pub(crate) cover: Option<Cover>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PlaylistItemAction {
    Open(u64),
    Delete(u64),
    /// Like or unlike: liked playlists show on Home.
    ToggleFavorite(u64),
}

pub(crate) fn playlist_list(
    ui: &mut Ui,
    playlists: &[PlaylistItem<'_>],
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<PlaylistItemAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.spacing_mut().item_spacing.y = r.rem(0.125);
    if playlists.is_empty() {
        ui.add(
            egui::Label::new(
                egui::RichText::new("No playlists yet. Press + to make one.")
                    .font(fonts::body(r.rem(0.8125)))
                    .color(theme.muted_text),
            )
            .wrap(),
        );
    }
    for playlist in playlists {
        let (rect, row) =
            ui.allocate_exact_size(vec2(ui.available_width(), r.rem(3.0)), Sense::click());
        let hover = pointer_amount(ui, &row);
        let fill = if playlist.selected {
            theme.surface
        } else {
            mix(
                Color32::TRANSPARENT,
                Color32::from_rgba_unmultiplied(34, 34, 34, 153),
                hover,
            )
        };
        ui.painter().rect_filled(rect, r.rem(0.5), fill);
        // Cover thumbnail: the chosen photo, or the default icon.
        let icon = Rect::from_min_size(
            pos2(rect.left() + r.rem(0.375), rect.center().y - r.rem(1.125)),
            r.rem2(2.25, 2.25),
        );
        match &playlist.cover {
            Some(cover) => widgets::cover(ui, icon, Some(cover), icons, r.rem(0.375)),
            None => crate::ui::playlist::default_cover(ui, icon, r.rem(0.375), icons, &theme),
        }
        let delete = Rect::from_center_size(
            pos2(rect.right() - r.rem(1.25), rect.center().y),
            r.rem2(2.0, 2.0),
        );
        // ♥: like (show on Home) or unlike. Always shown when liked,
        // otherwise while the row is hovered.
        let heart_rect = Rect::from_center_size(
            pos2(delete.left() - r.rem(1.125), rect.center().y),
            r.rem2(1.75, 1.75),
        );
        let heart = ui.interact(
            heart_rect,
            item_id("like_playlist", &playlist.id.to_string()),
            Sense::click(),
        );
        if playlist.favorite || row.contains_pointer() {
            let heart_hover = hover_amount(ui, &heart);
            ui.painter().circle_filled(
                heart_rect.center(),
                heart_rect.width() / 2.0,
                mix(Color32::TRANSPARENT, theme.surface, heart_hover),
            );
            paint_icon(
                ui.painter(),
                if playlist.favorite {
                    &icons.heart_filled
                } else {
                    &icons.heart
                },
                Rect::from_center_size(heart_rect.center(), r.rem2(0.9375, 0.9375)),
                if playlist.favorite {
                    theme.like
                } else {
                    mix(theme.secondary_text, theme.like, heart_hover)
                },
            );
        }
        let text_right = heart_rect.left() - r.rem(0.25);
        let text = Rect::from_min_max(
            pos2(icon.right() + r.rem(0.625), rect.top()),
            pos2(text_right, rect.bottom()),
        );
        elided(
            ui.painter(),
            Rect::from_min_size(
                pos2(text.left(), rect.center().y - r.rem(1.125)),
                vec2(text.width(), r.rem(1.375)),
            ),
            playlist.name,
            fonts::display(r.rem(1.0)),
            theme.text,
            Align2::LEFT_CENTER,
        );
        elided(
            ui.painter(),
            Rect::from_min_size(
                pos2(text.left(), rect.center().y + r.rem(0.125)),
                vec2(text.width(), r.rem(1.0)),
            ),
            &format!(
                "{} song{}",
                playlist.songs,
                if playlist.songs == 1 { "" } else { "s" }
            ),
            fonts::body(r.rem(0.75)),
            theme.muted_text,
            Align2::LEFT_CENTER,
        );
        let deleted = hold_to_delete(
            ui,
            delete,
            item_id("delete_playlist", &playlist.id.to_string()),
            row.contains_pointer(),
            "Hold for 3 seconds to delete this playlist",
            icons,
            r,
        );
        // Routed by where the pointer is, so ♥ never also opens the playlist.
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let on_heart = pointer.is_some_and(|pointer| heart_rect.contains(pointer));
        let clicked = row.clicked() || heart.clicked();
        heart
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(if playlist.favorite {
                "Unlike: remove from Home"
            } else {
                "Like: show on Home"
            });
        if deleted {
            actions.push(PlaylistItemAction::Delete(playlist.id));
        } else if clicked && on_heart {
            actions.push(PlaylistItemAction::ToggleFavorite(playlist.id));
        } else if row
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Open playlist")
            .clicked()
        {
            actions.push(PlaylistItemAction::Open(playlist.id));
        }
    }
    actions
}

/// A trash button that only acts after being held for three seconds; a ring
/// fills up while it is held. Faint until its row is hovered.
fn hold_to_delete(
    ui: &mut Ui,
    rect: Rect,
    id: Id,
    row_hovered: bool,
    tooltip: &str,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> bool {
    let theme = Theme::default();
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let held = response.is_pointer_button_down_on();
    let key = id.with("held_since");
    let now = ui.input(|input| input.time);
    let since: Option<f64> = ui.data(|data| data.get_temp(key));
    let mut progress = 0.0_f32;
    let mut fire = false;
    if held {
        match since {
            None => ui.data_mut(|data| data.insert_temp(key, now)),
            Some(start) if start.is_finite() => {
                progress = ((now - start) / HOLD_TO_DELETE).clamp(0.0, 1.0) as f32;
                if progress >= 1.0 {
                    fire = true;
                    // Fire once per press.
                    ui.data_mut(|data| data.insert_temp(key, f64::INFINITY));
                }
            }
            Some(_) => {}
        }
        repaint_after(ui.ctx(), Duration::from_millis(16));
    } else if since.is_some() {
        ui.data_mut(|data| data.remove::<f64>(key));
    }

    let hover = hover_amount(ui, &response);
    let painter = ui.painter();
    let center = rect.center();
    let radius = rect.width() / 2.0;
    if held || hover > 0.0 {
        painter.circle_filled(
            center,
            radius,
            mix(
                Color32::TRANSPARENT,
                theme.surface,
                hover.max(held as u8 as f32),
            ),
        );
    }
    if progress > 0.0 {
        let steps = 32;
        let points: Vec<_> = (0..=steps)
            .map(|step| {
                let angle = -std::f32::consts::FRAC_PI_2
                    + std::f32::consts::TAU * progress * step as f32 / steps as f32;
                center + (radius - r.rem(0.125)) * vec2(angle.cos(), angle.sin())
            })
            .collect();
        painter.add(egui::Shape::line(
            points,
            Stroke::new(r.rem(0.15), theme.danger),
        ));
    }
    let resting = if row_hovered {
        theme.secondary_text
    } else {
        theme.muted_text.gamma_multiply(0.6)
    };
    paint_icon(
        painter,
        &icons.close,
        Rect::from_center_size(center, r.rem2(1.0, 1.0)),
        mix(mix(resting, theme.text, hover), theme.danger, progress),
    );
    response
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(tooltip);
    fire
}

/// Whether a friend can be reached in MusiKPL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FriendStatus {
    Online,
    Offline,
    /// Not checked yet.
    Unknown,
    /// Added from Last.fm, but their friend code wasn't added yet.
    NoCode,
}

pub(crate) struct FriendItem<'a> {
    pub(crate) name: &'a str,
    /// Their Last.fm picture; their initial is drawn without one.
    pub(crate) avatar: Option<crate::domain::Cover>,
    pub(crate) status: FriendStatus,
    /// They invited us (shown as a Join button when invites are quiet).
    pub(crate) invited_us: bool,
}

pub(crate) struct FriendsView<'a> {
    pub(crate) items: Vec<FriendItem<'a>>,
    /// This device hosts a Jam, so friends can be invited.
    pub(crate) hosting: bool,
    /// Invites show in the list (not as a pop-up).
    pub(crate) quiet_invites: bool,
    pub(crate) adding: bool,
    pub(crate) notice: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FriendAction {
    Open(usize),
    Remove(usize),
    Invite(usize),
    Join(usize),
    CopyCode,
    StartAdding,
    CancelAdding,
    AddCode,
}

pub(crate) fn friends_list(
    ui: &mut Ui,
    view: &FriendsView<'_>,
    code_input: &mut String,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<FriendAction> {
    use crate::ui::widgets::{pill_button, text_field, FieldOptions, PillKind, PillSize};
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.spacing_mut().item_spacing.y = r.rem(0.375);
    let note = |ui: &mut Ui, text: &str, color: Color32| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .font(fonts::body(r.rem(0.8125)))
                    .color(color),
            )
            .wrap(),
        );
    };
    if let Some(notice) = view.notice {
        note(ui, notice, theme.warning);
    }
    if view.adding {
        let width = ui.available_width();
        text_field(
            ui,
            code_input,
            width,
            FieldOptions {
                id: "friend_code",
                hint: "Paste a friend code",
                password: false,
                enabled: true,
            },
            r,
        );
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = r.rem(0.5);
            if pill_button(ui, "Add", PillKind::Primary, PillSize::Small, r).clicked() {
                actions.push(FriendAction::AddCode);
            }
            if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Small, r).clicked() {
                actions.push(FriendAction::CancelAdding);
            }
        });
    } else {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = r.rem(0.5);
            if pill_button(ui, "Copy my code", PillKind::Secondary, PillSize::Small, r)
                .on_hover_text("Send it to a friend; they add it, and you add theirs")
                .clicked()
            {
                actions.push(FriendAction::CopyCode);
            }
            if pill_button(ui, "Add a code", PillKind::Secondary, PillSize::Small, r).clicked() {
                actions.push(FriendAction::StartAdding);
            }
        });
    }
    if view.items.is_empty() {
        note(
            ui,
            "Swap friend codes, or open someone's Last.fm profile from Search and press Add friend.",
            theme.muted_text,
        );
    }
    for (index, friend) in view.items.iter().enumerate() {
        let (rect, row) =
            ui.allocate_exact_size(vec2(ui.available_width(), r.rem(3.0)), Sense::click());
        let hover = pointer_amount(ui, &row);
        ui.painter().rect_filled(
            rect,
            r.rem(0.5),
            mix(
                Color32::TRANSPARENT,
                Color32::from_rgba_unmultiplied(34, 34, 34, 153),
                hover,
            ),
        );
        let photo = Rect::from_min_size(
            pos2(rect.left() + r.rem(0.375), rect.center().y - r.rem(1.125)),
            r.rem2(2.25, 2.25),
        );
        crate::ui::artist::round_photo(ui, photo, friend.name, friend.avatar.as_ref(), icons);
        // Online dot on the photo.
        if friend.status == FriendStatus::Online {
            let dot = pos2(photo.right() - r.rem(0.25), photo.bottom() - r.rem(0.25));
            ui.painter().circle_filled(dot, r.rem(0.3125), theme.panel);
            ui.painter()
                .circle_filled(dot, r.rem(0.21875), theme.success);
        }
        let remove = Rect::from_center_size(
            pos2(rect.right() - r.rem(1.125), rect.center().y),
            r.rem2(1.75, 1.75),
        );
        // A Join or Invite button left of the remove button.
        let button_label = if friend.invited_us && view.quiet_invites {
            Some("Join")
        } else if view.hosting && friend.status == FriendStatus::Online {
            Some("Invite")
        } else {
            None
        };
        let button_rect = button_label.map(|label| {
            let width = r.rem(0.875) * label.len() as f32 * 0.55 + r.rem(1.5);
            Rect::from_center_size(
                pos2(remove.left() - r.rem(0.25) - width / 2.0, rect.center().y),
                vec2(width, r.rem(1.75)),
            )
        });
        let text_right = button_rect.map_or(remove.left(), |button| button.left()) - r.rem(0.25);
        let text = Rect::from_min_max(
            pos2(photo.right() + r.rem(0.625), rect.top()),
            pos2(text_right, rect.bottom()),
        );
        elided(
            ui.painter(),
            Rect::from_min_size(
                pos2(text.left(), rect.center().y - r.rem(1.125)),
                vec2(text.width(), r.rem(1.375)),
            ),
            friend.name,
            fonts::display(r.rem(1.0)),
            theme.text,
            Align2::LEFT_CENTER,
        );
        let (status, color) = match friend.status {
            _ if friend.invited_us && view.quiet_invites => ("Invited you to a Jam", theme.warning),
            FriendStatus::Online => ("Online in MusiKPL", theme.success),
            FriendStatus::Offline => ("Offline", theme.muted_text),
            FriendStatus::Unknown => ("Checking…", theme.muted_text),
            FriendStatus::NoCode => ("Needs their friend code", theme.muted_text),
        };
        elided(
            ui.painter(),
            Rect::from_min_size(
                pos2(text.left(), rect.center().y + r.rem(0.125)),
                vec2(text.width(), r.rem(1.0)),
            ),
            status,
            fonts::body(r.rem(0.75)),
            color,
            Align2::LEFT_CENTER,
        );

        let pointer = ui.input(|input| input.pointer.interact_pos());
        let over = |rect: Rect| pointer.is_some_and(|pointer| rect.contains(pointer));
        let mut on_button = false;
        if let (Some(label), Some(button)) = (button_label, button_rect) {
            let response = ui.interact(
                button,
                item_id("friend_button", &index.to_string()),
                Sense::click(),
            );
            let t = hover_amount(ui, &response);
            ui.painter().rect_filled(
                button,
                button.height() / 2.0,
                mix(theme.text, theme.primary_hover, t),
            );
            ui.painter().text(
                button.center(),
                Align2::CENTER_CENTER,
                label,
                fonts::display(r.rem(0.875)),
                Color32::BLACK,
            );
            let clicked = (response.clicked() || row.clicked()) && over(button);
            response.on_hover_cursor(CursorIcon::PointingHand);
            if clicked {
                actions.push(if label == "Join" {
                    FriendAction::Join(index)
                } else {
                    FriendAction::Invite(index)
                });
            }
            on_button = over(button);
        }
        let removed = hold_to_delete(
            ui,
            remove,
            item_id("remove_friend", &index.to_string()),
            row.contains_pointer(),
            "Hold for 3 seconds to remove this friend",
            icons,
            r,
        );
        if removed {
            actions.push(FriendAction::Remove(index));
        } else if !on_button
            && !over(remove)
            && row
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Open profile")
                .clicked()
        {
            actions.push(FriendAction::Open(index));
        }
    }
    actions
}
/// "Ana invited you to a Jam" with Join and Not now, floating above the
/// player bar. Returns Some(true) for Join, Some(false) for Not now.
pub(crate) fn invite_popup(
    ctx: &egui::Context,
    area: Rect,
    name: &str,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Option<bool> {
    let text = format!("{name} invited you to a Jam");
    ask_popup(
        ctx,
        area,
        ("jam_invite_popup", name, &text, "Join"),
        icons,
        r,
    )
}

/// A question floating above the player bar, with a photo for `name`, a
/// yes button and Not now. Returns Some(true) for yes, Some(false) for Not
/// now.
pub(crate) fn ask_popup(
    ctx: &egui::Context,
    area: Rect,
    (key, name, text, yes): (&str, &str, &str, &str),
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Option<bool> {
    use crate::ui::widgets::{pill_button, PillKind, PillSize};
    let theme = Theme::default();
    let width = r.rem(22.0).min(area.width() - r.rem(3.0));
    egui::Area::new(Id::new(key))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(area.right() - r.rem(1.5), area.bottom() - r.rem(1.5)))
        .pivot(Align2::RIGHT_BOTTOM)
        .show(ctx, |ui| {
            let mut answer = None;
            egui::Frame::none()
                .fill(theme.panel)
                .stroke(Stroke::new(1.0_f32, theme.text))
                .rounding(r.rem(0.75))
                .inner_margin(egui::Margin::same(r.rem(1.0)))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = r.rem(0.75);
                        let (photo, _) = ui.allocate_exact_size(r.rem2(2.75, 2.75), Sense::hover());
                        crate::ui::artist::round_photo(ui, photo, name, None, icons);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = r.rem(0.5);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(text)
                                        .font(fonts::display(r.rem(1.0625)))
                                        .color(theme.text),
                                )
                                .wrap(),
                            );
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = r.rem(0.5);
                                if pill_button(ui, yes, PillKind::Primary, PillSize::Small, r)
                                    .clicked()
                                {
                                    answer = Some(true);
                                }
                                if pill_button(
                                    ui,
                                    "Not now",
                                    PillKind::Secondary,
                                    PillSize::Small,
                                    r,
                                )
                                .clicked()
                                {
                                    answer = Some(false);
                                }
                            });
                        });
                    });
                });
            answer
        })
        .inner
}
