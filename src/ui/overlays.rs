//! Panels: the menu and queue, docked beside the main area so the app stays
//! usable while they are open, and the settings modal, which fades in over a
//! backdrop that closes it when clicked.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, Id, Order, Rect, Sense, Shadow, Stroke, Ui,
};

use crate::{
    domain::{Cover, CoverArt, Playlists, Track},
    theme::Theme,
    ui::{
        artist::{artist_card, compact},
        fonts,
        icons::{paint_icon, IconTextures},
        jam::{self, JamAction, JamPanelState, JamView},
        layout::{modal_rect, Side},
        menu::{
            add_to_playlist_menu, friends_list, playlist_list, section_card, FriendAction,
            FriendStatus, FriendsView, PlaylistItem, PlaylistItemAction, PlaylistTarget,
        },
        responsive::ResponsiveUi,
        widgets::{
            self, caps_label, elided, hover_amount, icon_button, item_id, mix, pill_button,
            pointer_amount, spinner, text_field, toggle, ButtonColors, FieldOptions, PillKind,
            PillSize,
        },
    },
};

/// Seconds for a panel to slide or fade in.
const OPEN_TIME: f32 = 0.2;

/// How far a panel's open animation has progressed.
pub(crate) fn open_amount(ctx: &egui::Context, name: &str, open: bool) -> f32 {
    ctx.animate_bool_with_time(Id::new(("overlay", name)), open, OPEN_TIME)
}

/// A full-window layer under a panel. Returns true when it was clicked.
fn backdrop(ctx: &egui::Context, name: &str, window: Rect, dim: f32) -> bool {
    egui::Area::new(Id::new(("backdrop", name)))
        .order(Order::Middle)
        .fixed_pos(window.min)
        .show(ctx, |ui| {
            let response = ui.allocate_rect(window, Sense::click());
            ui.painter()
                .rect_filled(window, 0.0, Color32::from_black_alpha((dim * 255.0) as u8));
            response.clicked()
        })
        .inner
}

/// A rounded, shadowed panel in the foreground. `content` gets a `Ui` for
/// the inside of the panel, less `padding`.
fn panel<R>(
    ctx: &egui::Context,
    name: &str,
    rect: Rect,
    opacity: f32,
    padding: f32,
    r: &ResponsiveUi,
    content: impl FnOnce(&mut Ui) -> R,
) -> R {
    let theme = Theme::default();
    egui::Area::new(Id::new(("panel", name)))
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_opacity(opacity);
            // The panel owns its whole rectangle, so clicks inside it never
            // reach the backdrop.
            let (rect, _) = ui.allocate_exact_size(rect.size(), Sense::click());
            let rounding = r.rem(1.0);
            let shadow = Shadow {
                offset: vec2(0.0, r.rem(0.75)),
                blur: r.rem(2.0),
                spread: 0.0,
                color: Color32::from_black_alpha(150),
            };
            ui.painter().add(shadow.as_shape(rect, rounding));
            ui.painter().rect(
                rect,
                rounding,
                theme.panel,
                Stroke::new(1.0_f32, theme.border),
            );
            ui.allocate_new_ui(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink(padding))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.set_clip_rect(rect);
                    content(ui)
                },
            )
            .inner
        })
        .inner
}

/// A side panel docked in the main area: the rest of the app stays usable
/// beside it. `rect` may extend past `visible` while it slides in.
fn docked<R>(
    ui: &mut Ui,
    rect: Rect,
    visible: Rect,
    side: Side,
    padding: f32,
    content: impl FnOnce(&mut Ui) -> R,
) -> R {
    let theme = Theme::default();
    let clip = rect.intersect(visible);
    let painter = ui.painter_at(clip);
    painter.rect_filled(rect, 0.0, theme.panel);
    let edge = match side {
        Side::Left => [rect.right_top(), rect.right_bottom()],
        Side::Right => [rect.left_top(), rect.left_bottom()],
    };
    painter.line_segment(edge, Stroke::new(1.0_f32, theme.border));
    ui.allocate_new_ui(
        egui::UiBuilder::new()
            .max_rect(rect.shrink(padding))
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_clip_rect(clip);
            content(ui)
        },
    )
    .inner
}

/// Width of the docked menu and queue.
pub(crate) fn menu_width(r: &ResponsiveUi) -> f32 {
    r.rem(18.75)
}

pub(crate) fn queue_width(r: &ResponsiveUi) -> f32 {
    r.rem(25.0)
}

/// "Queue"/"Menu" heading with round buttons on the right.
fn drawer_header(
    ui: &mut Ui,
    title: &str,
    buttons: &[(&egui::TextureHandle, egui::Vec2, &str)],
    r: &ResponsiveUi,
) -> Option<usize> {
    let theme = Theme::default();
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(fonts::display(r.rem(1.75)))
                .color(theme.text),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = r.rem(0.3125);
            // Laid out right to left, so the last button is drawn first.
            for (index, (icon, icon_size, tooltip)) in buttons.iter().enumerate().rev() {
                let response = icon_button(
                    ui,
                    icon,
                    r.rem2(2.25, 2.25),
                    *icon_size,
                    r.rem(1.125),
                    ButtonColors::round(&theme),
                );
                if response.on_hover_text(*tooltip).clicked() {
                    clicked = Some(index);
                }
            }
        });
    });
    clicked
}

// ── Menu ────────────────────────────────────────────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MenuAction {
    Close,
    OpenSettings,
    Jam(JamAction),
    CheckForUpdates,
    InstallUpdate,
    NewPlaylist,
    Playlist(PlaylistItemAction),
    TogglePlaylists,
    ToggleJam,
    ToggleArtists,
    ToggleFriends,
    Friend(FriendAction),
    OpenProfile,
    ConnectLastFm,
    OpenArtist(String),
}

/// The Last.fm account card at the top of the menu.
pub(crate) struct MenuProfile<'a> {
    /// Empty when not connected.
    pub(crate) username: &'a str,
    pub(crate) scrobbles: Option<u64>,
    pub(crate) avatar: Option<Cover>,
}

/// Where updating MusiKPL stands, for the bottom of the menu.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UpdateView<'a> {
    Idle,
    Checking,
    UpToDate,
    Available { version: &'a str },
    Downloading { percent: u32 },
    Failed(&'a str),
}

pub(crate) struct MenuView<'a> {
    pub(crate) jam: JamView<'a>,
    pub(crate) jam_open: bool,
    pub(crate) playlists: Vec<PlaylistItem<'a>>,
    pub(crate) playlists_open: bool,
    pub(crate) profile: MenuProfile<'a>,
    /// Bookmarked artists, with their photos.
    pub(crate) artists: Vec<(&'a str, Option<Cover>)>,
    pub(crate) artists_open: bool,
    pub(crate) friends: FriendsView<'a>,
    pub(crate) friends_open: bool,
    pub(crate) update: UpdateView<'a>,
}

/// A few words on the Jam for its collapsed card.
fn jam_summary(view: &JamView<'_>) -> String {
    match view.state {
        JamPanelState::Inactive => "Listen together".into(),
        JamPanelState::Connecting => "Connecting…".into(),
        JamPanelState::Hosting { listeners: 0 } => "Live · waiting for friends".into(),
        JamPanelState::Hosting { listeners } => format!("Live · {listeners} listening"),
        JamPanelState::Joined => format!("With {}", view.host_name),
    }
}

/// The menu, docked on the left of `main`. `open` slides it in.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show_menu(
    ui: &mut Ui,
    main: Rect,
    open: f32,
    icons: &IconTextures,
    view: &MenuView<'_>,
    jam_invite: &mut String,
    friend_code: &mut String,
    r: &ResponsiveUi,
) -> Vec<MenuAction> {
    let mut actions = Vec::new();
    let width = menu_width(r);
    let left = main.left() - width * (1.0 - open);
    let rect = Rect::from_min_size(pos2(left, main.top()), vec2(width, main.height()));
    docked(ui, rect, main, Side::Left, r.rem(1.25), |ui| {
        ui.set_opacity(open);
        {
            ui.spacing_mut().item_spacing.y = r.rem(1.0);
            match drawer_header(
                ui,
                "Menu",
                &[
                    (&icons.settings, r.rem2(0.9375, 0.9375), "Settings"),
                    (&icons.close, r.rem2(1.125, 1.125), "Close"),
                ],
                r,
            ) {
                Some(0) => actions.push(MenuAction::OpenSettings),
                Some(_) => actions.push(MenuAction::Close),
                None => {}
            }
            let update_height = r.rem(5.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .max_height((ui.available_height() - update_height).max(0.0))
                // Scrolls with the wheel, without a bar.
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = r.rem(0.75);
                    // Last.fm account: the profile, or a way to connect.
                    let width = ui.available_width();
                    let card = if view.profile.username.is_empty() {
                        artist_card(
                            ui,
                            width,
                            "Last.fm",
                            "Connect to see your stats",
                            None,
                            icons,
                            r,
                        )
                        .on_hover_text("Connect your Last.fm account")
                    } else {
                        let subtitle = view.profile.scrobbles.map_or_else(
                            || "Your profile".to_owned(),
                            |scrobbles| format!("{} scrobbles", compact(scrobbles)),
                        );
                        artist_card(
                            ui,
                            width,
                            view.profile.username,
                            &subtitle,
                            view.profile.avatar.as_ref(),
                            icons,
                            r,
                        )
                        .on_hover_text("Your Last.fm profile")
                    };
                    if card.clicked() {
                        actions.push(if view.profile.username.is_empty() {
                            MenuAction::ConnectLastFm
                        } else {
                            MenuAction::OpenProfile
                        });
                    }

                    let count = view.playlists.len();
                    let mut playlist_actions = Vec::new();
                    let card = section_card(
                        ui,
                        "playlists",
                        &icons.playlist,
                        "Playlists",
                        &count.to_string(),
                        view.playlists_open,
                        &[(&icons.add, "New playlist")],
                        icons,
                        r,
                        |ui| playlist_actions = playlist_list(ui, &view.playlists, icons, r),
                    );
                    if card.extra_clicked.is_some() {
                        actions.push(MenuAction::NewPlaylist);
                    } else if card.toggled {
                        actions.push(MenuAction::TogglePlaylists);
                    }
                    actions.extend(playlist_actions.into_iter().map(MenuAction::Playlist));

                    let mut opened_artist = None;
                    let card = section_card(
                        ui,
                        "artists",
                        &icons.artist,
                        "Artists",
                        &view.artists.len().to_string(),
                        view.artists_open,
                        &[],
                        icons,
                        r,
                        |ui| {
                            ui.spacing_mut().item_spacing.y = r.rem(0.25);
                            if view.artists.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(
                                            "Bookmark artists from their page to keep them here.",
                                        )
                                        .font(fonts::body(r.rem(0.8125)))
                                        .color(Theme::default().muted_text),
                                    )
                                    .wrap(),
                                );
                            }
                            let width = ui.available_width();
                            for (name, photo) in &view.artists {
                                if artist_card(ui, width, name, "Artist", photo.as_ref(), icons, r)
                                    .clicked()
                                {
                                    opened_artist = Some((*name).to_owned());
                                }
                            }
                        },
                    );
                    if card.toggled {
                        actions.push(MenuAction::ToggleArtists);
                    }
                    if let Some(name) = opened_artist {
                        actions.push(MenuAction::OpenArtist(name));
                    }

                    let online = view
                        .friends
                        .items
                        .iter()
                        .filter(|friend| friend.status == FriendStatus::Online)
                        .count();
                    let invites = view.friends.items.iter().filter(|f| f.invited_us).count();
                    let summary = match (invites, online) {
                        (0, 0) => view.friends.items.len().to_string(),
                        (0, online) => format!("{online} online"),
                        (invites, _) => {
                            format!("{invites} invite{}", if invites == 1 { "" } else { "s" })
                        }
                    };
                    let mut friend_actions = Vec::new();
                    let card = section_card(
                        ui,
                        "friends",
                        &icons.friends,
                        "Friends",
                        &summary,
                        view.friends_open,
                        &[],
                        icons,
                        r,
                        |ui| {
                            friend_actions = friends_list(ui, &view.friends, friend_code, icons, r)
                        },
                    );
                    if card.toggled {
                        actions.push(MenuAction::ToggleFriends);
                    }
                    actions.extend(friend_actions.into_iter().map(MenuAction::Friend));

                    let mut jam_actions = Vec::new();
                    let card = section_card(
                        ui,
                        "jam",
                        &icons.music,
                        "Jam",
                        &jam_summary(&view.jam),
                        view.jam_open,
                        &[],
                        icons,
                        r,
                        |ui| jam_actions = jam::show(ui, &view.jam, jam_invite, r),
                    );
                    if card.toggled {
                        actions.push(MenuAction::ToggleJam);
                    }
                    actions.extend(jam_actions.into_iter().map(MenuAction::Jam));
                });
            divider(ui);
            update_section(ui, &view.update, r, &mut actions);
        }
    });
    actions
}

fn divider(ui: &mut Ui) {
    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(line, 0.0, Theme::default().border);
}

/// "Check for updates", or "Install update" once a newer release exists.
fn update_section(
    ui: &mut Ui,
    view: &UpdateView<'_>,
    r: &ResponsiveUi,
    actions: &mut Vec<MenuAction>,
) {
    let theme = Theme::default();
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
    ui.spacing_mut().item_spacing.y = r.rem(0.5);
    match view {
        UpdateView::Available { version } => {
            if pill_button(ui, "Install update", PillKind::Primary, PillSize::Small, r)
                .on_hover_text("Download it, then MusiKPL restarts. Your data stays.")
                .clicked()
            {
                actions.push(MenuAction::InstallUpdate);
            }
            note(
                ui,
                &format!("Version {version} is available"),
                theme.success,
            );
        }
        UpdateView::Downloading { percent } => {
            ui.horizontal(|ui| {
                spinner(ui, r.rem(1.0));
                note(
                    ui,
                    &format!("Downloading update… {percent}%"),
                    theme.secondary_text,
                );
            });
            let (bar, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), r.rem(0.25)), Sense::hover());
            ui.painter()
                .rect_filled(bar, bar.height() / 2.0, theme.surface);
            let mut done = bar;
            done.set_width(bar.width() * (*percent as f32 / 100.0).clamp(0.0, 1.0));
            ui.painter()
                .rect_filled(done, bar.height() / 2.0, theme.text);
        }
        UpdateView::Checking => {
            ui.horizontal(|ui| {
                spinner(ui, r.rem(1.0));
                note(ui, "Checking for updates…", theme.secondary_text);
            });
        }
        other => {
            if pill_button(
                ui,
                "Check for updates",
                PillKind::Secondary,
                PillSize::Small,
                r,
            )
            .clicked()
            {
                actions.push(MenuAction::CheckForUpdates);
            }
            match other {
                UpdateView::UpToDate => note(
                    ui,
                    concat!("You're up to date (v", env!("CARGO_PKG_VERSION"), ")"),
                    theme.secondary_text,
                ),
                UpdateView::Failed(error) => note(ui, error, theme.danger),
                _ => note(
                    ui,
                    concat!("MusiKPL v", env!("CARGO_PKG_VERSION")),
                    theme.muted_text,
                ),
            }
        }
    }
}

// ── Queue ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum QueueAction {
    Close,
    Play(Track),
    Remove(usize),
    AddToPlaylist(Track, PlaylistTarget),
    ToggleLike(Track),
}

pub(crate) struct QueueView<'a> {
    pub(crate) now_playing: Option<&'a Track>,
    pub(crate) queue: &'a [Track],
    pub(crate) mixing: bool,
    pub(crate) guest: bool,
    pub(crate) covers: &'a CoverArt,
    pub(crate) playlists: &'a Playlists,
    pub(crate) liked: &'a dyn Fn(&str) -> bool,
}

/// The queue, docked on the right of `main`. `open` slides it in.
pub(crate) fn show_queue(
    ui: &mut Ui,
    main: Rect,
    open: f32,
    icons: &IconTextures,
    view: &QueueView<'_>,
    r: &ResponsiveUi,
) -> Vec<QueueAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    let width = queue_width(r);
    let left = main.right() - width * open;
    let rect = Rect::from_min_size(pos2(left, main.top()), vec2(width, main.height()));
    docked(ui, rect, main, Side::Right, r.rem(1.25), |ui| {
        ui.set_opacity(open);
        {
            ui.spacing_mut().item_spacing.y = r.rem(0.75);
            if drawer_header(
                ui,
                "Queue",
                &[(&icons.close, r.rem2(1.125, 1.125), "Close")],
                r,
            )
            .is_some()
            {
                actions.push(QueueAction::Close);
            }
            let label = |ui: &mut Ui, text: &str| {
                ui.label(
                    egui::RichText::new(text)
                        .font(fonts::body(r.rem(0.875)))
                        .color(theme.secondary_text),
                );
            };
            label(ui, "Now playing");
            match view.now_playing {
                Some(track) => {
                    queue_item(ui, track, None, true, view, icons, r, &mut actions);
                }
                None => {
                    ui.label(
                        egui::RichText::new("Nothing is playing")
                            .font(fonts::body(r.rem(0.9375)))
                            .color(theme.muted_text),
                    );
                }
            }
            ui.add_space(r.rem(0.5));
            ui.horizontal(|ui| {
                label(ui, "Next up");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let count = view.queue.len();
                    let text = if view.mixing {
                        "Building your mix…".to_owned()
                    } else {
                        format!("{count} song{}", if count == 1 { "" } else { "s" })
                    };
                    label(ui, &text);
                    if view.mixing {
                        spinner(ui, r.rem(1.0));
                    }
                });
            });
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = r.rem(0.125);
                    if view.queue.is_empty() {
                        let message = if view.guest {
                            "Songs you and the host add appear here. Press + on a song to add it."
                        } else {
                            "Songs you add, and Auto Mix picks, appear here."
                        };
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message)
                                    .font(fonts::body(r.rem(0.875)))
                                    .color(theme.muted_text),
                            )
                            .wrap(),
                        );
                    }
                    for (index, track) in view.queue.iter().enumerate() {
                        queue_item(ui, track, Some(index), false, view, icons, r, &mut actions);
                    }
                });
        }
    });
    actions
}

/// One queue row. Clicking plays it; hovering shows like, add to playlist,
/// and remove buttons in place of the length.
#[allow(clippy::too_many_arguments)]
fn queue_item(
    ui: &mut Ui,
    track: &Track,
    index: Option<usize>,
    now_playing: bool,
    view: &QueueView<'_>,
    icons: &IconTextures,
    r: &ResponsiveUi,
    actions: &mut Vec<QueueAction>,
) {
    let theme = Theme::default();
    // Always senses clicks, so the right-click menu works on every row.
    let (rect, row) =
        ui.allocate_exact_size(vec2(ui.available_width(), r.rem(4.0)), Sense::click());
    let hover = pointer_amount(ui, &row);
    let mut added_to = None;
    row.context_menu(|ui| {
        added_to = add_to_playlist_menu(ui, view.playlists, &track.id, r);
    });
    if let Some(target) = added_to {
        actions.push(QueueAction::AddToPlaylist(track.clone(), target));
    }
    let base = if now_playing {
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
            if now_playing { 0.0 } else { hover },
        ),
    );
    let inner = rect.shrink2(r.rem2(0.5, 0.5));
    let cover_rect = Rect::from_min_size(
        pos2(inner.left(), inner.center().y - r.rem(1.5)),
        r.rem2(3.0, 3.0),
    );
    widgets::cover(
        ui,
        cover_rect,
        view.covers.cover_for(track).as_ref(),
        icons,
        r.rem(0.375),
    );

    // On hover: ♥, add to playlist, and (for the host) remove. Otherwise
    // the length, with a ♥ for liked songs.
    // While this row's "add to playlist" menu is open, its buttons stay,
    // even with the pointer over the menu instead of the row.
    let menu_open = {
        let key = format!(
            "{}:{}",
            index.map_or("now".into(), |i| i.to_string()),
            track.id
        );
        let popup = item_id("queue_playlist_popup", &key);
        ui.memory(|memory| memory.is_popup_open(popup))
    };
    let hovered = row.contains_pointer() || menu_open;
    let removable = index.is_some() && !view.guest;
    let button = r.rem(1.75);
    let gap = r.rem(0.25);
    let buttons = if hovered {
        2 + usize::from(removable)
    } else {
        0
    };
    let trailing = if hovered {
        buttons as f32 * (button + gap)
    } else {
        r.rem(4.25)
    };
    let text_left = cover_rect.right() + r.rem(0.75);
    let text_rect = Rect::from_min_max(
        pos2(text_left, inner.top()),
        pos2(inner.right() - trailing - r.rem(0.5), inner.bottom()),
    );
    let painter = ui.painter();
    elided(
        painter,
        Rect::from_min_size(
            pos2(text_rect.left(), text_rect.center().y - r.rem(1.25)),
            vec2(text_rect.width(), r.rem(1.5)),
        ),
        &track.title,
        fonts::display(r.rem(1.0)),
        theme.text,
        Align2::LEFT_CENTER,
    );
    elided(
        painter,
        Rect::from_min_size(
            pos2(text_rect.left(), text_rect.center().y + r.rem(0.25)),
            vec2(text_rect.width(), r.rem(1.125)),
        ),
        &track.channel,
        fonts::body(r.rem(0.8125)),
        theme.secondary_text,
        Align2::LEFT_CENTER,
    );

    let liked = (view.liked)(&track.id);
    let key = format!(
        "{}:{}",
        index.map_or("now".into(), |i| i.to_string()),
        track.id
    );
    let slot = |n: usize| {
        Rect::from_min_size(
            pos2(
                inner.right() - button * (n as f32 + 1.0) - gap * n as f32,
                inner.center().y - button / 2.0,
            ),
            vec2(button, button),
        )
    };
    let pointer = ui.input(|input| input.pointer.interact_pos());
    let over = |rect: Rect| pointer.is_some_and(|pointer| rect.contains(pointer));
    let mut on_button = false;
    if hovered {
        let mut next = 0;
        if let (true, Some(index)) = (removable, index) {
            let rect = slot(next);
            next += 1;
            let remove = ui.interact(rect, item_id("queue_remove", &key), Sense::click());
            widgets::paint_icon_button(
                ui,
                rect,
                &remove,
                &icons.close,
                r.rem2(0.875, 0.875),
                button / 2.0,
                ButtonColors::round(&theme),
            );
            let clicked = remove.clicked() || row.clicked();
            remove
                .on_hover_cursor(CursorIcon::PointingHand)
                .on_hover_text("Remove from queue");
            if clicked && over(rect) {
                actions.push(QueueAction::Remove(index));
                return;
            }
            on_button |= over(rect);
        }

        let rect = slot(next);
        next += 1;
        let add = ui.interact(rect, item_id("queue_playlist", &key), Sense::click());
        widgets::paint_icon_button(
            ui,
            rect,
            &add,
            &icons.playlist_add,
            r.rem2(1.0, 1.0),
            button / 2.0,
            ButtonColors::round(&theme),
        );
        let popup = item_id("queue_playlist_popup", &key);
        if (add.clicked() || row.clicked()) && over(rect) {
            ui.memory_mut(|memory| memory.toggle_popup(popup));
        }
        on_button |= over(rect);
        let add = add
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Add to playlist");
        let picked = egui::popup::popup_below_widget(
            ui,
            popup,
            &add,
            egui::PopupCloseBehavior::CloseOnClickOutside,
            |ui| add_to_playlist_menu(ui, view.playlists, &track.id, r),
        )
        .flatten();
        if let Some(target) = picked {
            ui.memory_mut(|memory| memory.close_popup());
            actions.push(QueueAction::AddToPlaylist(track.clone(), target));
        }

        let rect = slot(next);
        let heart = ui.interact(rect, item_id("queue_like", &key), Sense::click());
        let mut colors = ButtonColors::round(&theme);
        if liked {
            colors.icon = theme.like;
            colors.hover_icon = theme.like;
        }
        widgets::paint_icon_button(
            ui,
            rect,
            &heart,
            if liked {
                &icons.heart_filled
            } else {
                &icons.heart
            },
            r.rem2(0.9375, 0.9375),
            button / 2.0,
            colors,
        );
        let clicked = heart.clicked() || row.clicked();
        heart
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text(if liked { "Unlike" } else { "Like" });
        if clicked && over(rect) {
            actions.push(QueueAction::ToggleLike(track.clone()));
            return;
        }
        on_button |= over(rect);
    } else {
        ui.painter().text(
            pos2(inner.right(), inner.center().y),
            Align2::RIGHT_CENTER,
            track.duration_text(),
            fonts::body(r.rem(0.8125)),
            theme.secondary_text,
        );
        if liked {
            paint_icon(
                ui.painter(),
                &icons.heart_filled,
                Rect::from_center_size(
                    pos2(inner.right() - r.rem(3.25), inner.center().y),
                    r.rem2(0.75, 0.75),
                ),
                theme.like,
            );
        }
    }
    let clickable = index.is_some() && !view.guest;
    if clickable && !on_button && row.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        actions.push(QueueAction::Play(track.clone()));
    }
}
// ── Settings ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsPage {
    #[default]
    General,
    LastFm,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SettingsAction {
    Close,
    Page(SettingsPage),
    SetStartWithWindows(bool),
    SetAutoMix(bool),
    SetCloseToTray(bool),
    SetInvitePopups(bool),
    LastFmConnect,
    LastFmCancel,
    LastFmDisconnect,
    OpenUrl(String),
    ClearHistory,
    PickGenres,
}

/// Where the Last.fm connection stands.
pub(crate) enum LastFmState<'a> {
    Disconnected,
    /// Getting a login token from Last.fm.
    Starting,
    /// Waiting for the user to approve MusiKPL in the browser.
    AwaitingApproval {
        url: &'a str,
    },
    Connected {
        username: &'a str,
    },
}

pub(crate) struct SettingsView<'a> {
    pub(crate) page: SettingsPage,
    pub(crate) start_with_windows: bool,
    pub(crate) auto_mix: bool,
    pub(crate) close_to_tray: bool,
    pub(crate) invite_popups: bool,
    pub(crate) lastfm: LastFmState<'a>,
    /// MusiKPL carries its own key pair: connecting needs no keys.
    pub(crate) one_click: bool,
    /// The latest problem to show on the current page.
    pub(crate) error: Option<&'a str>,
}

const API_ACCOUNT_URL: &str = "https://www.last.fm/api/account/create";

#[allow(clippy::too_many_arguments)]
pub(crate) fn show_settings(
    ctx: &egui::Context,
    window: Rect,
    open: f32,
    icons: &IconTextures,
    view: &SettingsView<'_>,
    api_key: &mut String,
    api_secret: &mut String,
    r: &ResponsiveUi,
) -> Vec<SettingsAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    if backdrop(ctx, "settings", window, 0.7 * open) {
        actions.push(SettingsAction::Close);
    }
    let rect = modal_rect(window, r);
    let rise = r.rem(1.0) * (1.0 - open);
    panel(
        ctx,
        "settings",
        rect.translate(vec2(0.0, rise)),
        open,
        0.0,
        r,
        |ui| {
            let area = ui.max_rect();
            let sidebar = Rect::from_min_size(area.min, vec2(r.rem(17.5), area.height()));
            let content = Rect::from_min_max(pos2(sidebar.right(), area.top()), area.max);

            // Sidebar: black, with a divider on its right edge.
            let rounding = egui::Rounding {
                nw: r.rem(1.0),
                sw: r.rem(1.0),
                ..egui::Rounding::ZERO
            };
            ui.painter().rect_filled(sidebar, rounding, Color32::BLACK);
            ui.painter().line_segment(
                [sidebar.right_top(), sidebar.right_bottom()],
                Stroke::new(1.0_f32, theme.border),
            );
            ui.allocate_new_ui(
                egui::UiBuilder::new().max_rect(Rect::from_min_max(
                    sidebar.min + r.rem2(1.5, 2.0),
                    sidebar.max - r.rem2(1.5, 1.5),
                )),
                |ui| settings_sidebar(ui, view.page, r, &mut actions),
            );

            // Close and ESC hint, top right.
            let close_rect = Rect::from_min_size(
                pos2(
                    content.right() - r.rem(2.0) - r.rem(2.25),
                    content.top() + r.rem(2.5),
                ),
                r.rem2(2.25, 2.25),
            );
            let close = ui.interact(close_rect, Id::new("settings_close"), Sense::click());
            widgets::paint_icon_button(
                ui,
                close_rect,
                &close,
                &icons.close,
                r.rem2(1.125, 1.125),
                r.rem(1.125),
                ButtonColors::round(&theme),
            );
            ui.painter().text(
                pos2(close_rect.center().x, close_rect.bottom() + r.rem(0.25)),
                Align2::CENTER_TOP,
                "ESC",
                fonts::body(r.rem(0.75)),
                theme.muted_text,
            );
            if close.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                actions.push(SettingsAction::Close);
            }

            let body = Rect::from_min_max(
                content.min + r.rem2(3.5, 2.5),
                pos2(
                    close_rect.left() - r.rem(1.5),
                    content.bottom() - r.rem(2.5),
                ),
            );
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(body), |ui| {
                ui.spacing_mut().item_spacing.y = r.rem(1.5);
                match view.page {
                    SettingsPage::General => general_page(ui, view, r, &mut actions),
                    SettingsPage::LastFm => {
                        lastfm_page(ui, view, api_key, api_secret, r, &mut actions)
                    }
                }
            });
        },
    );
    actions
}

fn settings_sidebar(
    ui: &mut Ui,
    page: SettingsPage,
    r: &ResponsiveUi,
    actions: &mut Vec<SettingsAction>,
) {
    let theme = Theme::default();
    ui.spacing_mut().item_spacing.y = r.rem(0.25);
    ui.label(
        egui::RichText::new("Settings")
            .font(fonts::display(r.rem(1.75)))
            .color(theme.text),
    );
    ui.add_space(r.rem(0.75));
    caps_label(ui, "APP SETTINGS", r);
    if nav_item(ui, "General", page == SettingsPage::General, r) {
        actions.push(SettingsAction::Page(SettingsPage::General));
    }
    ui.add_space(r.rem(0.75));
    caps_label(ui, "INTEGRATIONS", r);
    if nav_item(ui, "Last.fm", page == SettingsPage::LastFm, r) {
        actions.push(SettingsAction::Page(SettingsPage::LastFm));
    }
    let bottom = ui.max_rect().bottom();
    ui.painter().text(
        pos2(ui.max_rect().left(), bottom),
        Align2::LEFT_BOTTOM,
        concat!("MusiKPL v", env!("CARGO_PKG_VERSION")),
        fonts::body(r.rem(0.75)),
        theme.muted_text,
    );
}

fn nav_item(ui: &mut Ui, label: &str, selected: bool, r: &ResponsiveUi) -> bool {
    let theme = Theme::default();
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), r.rem(2.5)), Sense::click());
    let hover = hover_amount(ui, &response);
    let fill = if selected {
        theme.surface
    } else {
        mix(
            Color32::TRANSPARENT,
            Color32::from_rgba_unmultiplied(34, 34, 34, 153),
            hover,
        )
    };
    ui.painter().rect_filled(rect, r.rem(0.5), fill);
    ui.painter().text(
        pos2(rect.left() + r.rem(0.75), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::body(r.rem(1.0)),
        if selected {
            theme.text
        } else {
            mix(Color32::from_white_alpha(178), theme.text, hover)
        },
    );
    response.on_hover_cursor(CursorIcon::PointingHand).clicked() && !selected
}

fn page_heading(ui: &mut Ui, title: &str, description: Option<&str>, r: &ResponsiveUi) {
    let theme = Theme::default();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = r.rem(0.25);
        ui.label(
            egui::RichText::new(title)
                .font(fonts::display(r.rem(2.0)))
                .color(theme.text),
        );
        if let Some(description) = description {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(description)
                        .font(fonts::body(r.rem(0.9375)))
                        .color(theme.secondary_text),
                )
                .wrap(),
            );
        }
    });
}

fn error_line(ui: &mut Ui, error: Option<&str>, r: &ResponsiveUi) {
    if let Some(error) = error {
        ui.add(
            egui::Label::new(
                egui::RichText::new(error)
                    .font(fonts::body(r.rem(0.875)))
                    .color(Theme::default().danger),
            )
            .wrap(),
        );
    }
}

fn general_page(
    ui: &mut Ui,
    view: &SettingsView<'_>,
    r: &ResponsiveUi,
    actions: &mut Vec<SettingsAction>,
) {
    page_heading(ui, "General", None, r);
    if switch_card(
        ui,
        if cfg!(windows) {
            "Start with Windows"
        } else {
            "Start on login"
        },
        "Open MusiKPL automatically when you sign in.",
        view.start_with_windows,
        r,
    ) {
        actions.push(SettingsAction::SetStartWithWindows(
            !view.start_with_windows,
        ));
    }
    if switch_card(
        ui,
        "Auto Mix",
        "Keep the queue filled with songs like the one playing, leaning toward your taste.",
        view.auto_mix,
        r,
    ) {
        actions.push(SettingsAction::SetAutoMix(!view.auto_mix));
    }
    if switch_card(
        ui,
        "Jam invites as pop-ups",
        "When a friend invites you to a Jam, show a card you can't miss. Off: the invite waits quietly in Friends, in the menu.",
        view.invite_popups,
        r,
    ) {
        actions.push(SettingsAction::SetInvitePopups(!view.invite_popups));
    }
    // The tray is a Windows feature here.
    if cfg!(windows)
        && switch_card(
            ui,
            "Close to tray",
            "Closing hides MusiKPL to the system tray and the music keeps playing. Click the tray icon to bring it back; right-click it to quit.",
            view.close_to_tray,
            r,
        )
    {
        actions.push(SettingsAction::SetCloseToTray(!view.close_to_tray));
    }
    history_card(ui, r, actions);
    error_line(ui, view.error, r);
}

/// A setting with a title, a description, and an on/off switch. Returns
/// true when the switch was clicked.
fn switch_card(ui: &mut Ui, title: &str, description: &str, on: bool, r: &ResponsiveUi) -> bool {
    let theme = Theme::default();
    let mut clicked = false;
    egui::Frame::none()
        .fill(Color32::BLACK)
        .stroke(Stroke::new(1.0_f32, theme.border))
        .rounding(r.rem(0.75))
        .inner_margin(egui::Margin::symmetric(r.rem(1.25), r.rem(1.0)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let toggle_width = r.rem(2.75);
                ui.allocate_ui(
                    vec2(ui.available_width() - toggle_width - r.rem(1.5), r.rem(3.0)),
                    |ui| {
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = r.rem(0.125);
                            ui.label(
                                egui::RichText::new(title)
                                    .font(fonts::display(r.rem(1.125)))
                                    .color(theme.text),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(description)
                                        .font(fonts::body(r.rem(0.875)))
                                        .color(theme.secondary_text),
                                )
                                .wrap(),
                            );
                        });
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clicked = toggle(ui, on, r).clicked();
                });
            });
        });
    clicked
}

/// Listening history, with a two-step Clear so one stray click can't wipe it.
fn history_card(ui: &mut Ui, r: &ResponsiveUi, actions: &mut Vec<SettingsAction>) {
    let theme = Theme::default();
    let confirm_id = Id::new("confirm_clear_history");
    let confirming = ui.data(|data| data.get_temp::<bool>(confirm_id).unwrap_or(false));
    egui::Frame::none()
        .fill(Color32::BLACK)
        .stroke(Stroke::new(1.0_f32, theme.border))
        .rounding(r.rem(0.75))
        .inner_margin(egui::Margin::symmetric(r.rem(1.25), r.rem(1.0)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = r.rem(0.5);
                ui.label(
                    egui::RichText::new("Listening history")
                        .font(fonts::display(r.rem(1.125)))
                        .color(theme.text),
                );
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "MusiKPL remembers what you play, skip and like on this PC to build Home and lean Auto Mix toward your taste. It never leaves your computer.",
                        )
                        .font(fonts::body(r.rem(0.875)))
                        .color(theme.secondary_text),
                    )
                    .wrap(),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = r.rem(0.75);
                    if confirming {
                        if pill_button(ui, "Yes, clear it", PillKind::Primary, PillSize::Small, r)
                            .clicked()
                        {
                            actions.push(SettingsAction::ClearHistory);
                            ui.data_mut(|data| data.insert_temp(confirm_id, false));
                        }
                        if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Small, r)
                            .clicked()
                        {
                            ui.data_mut(|data| data.insert_temp(confirm_id, false));
                        }
                    } else if pill_button(
                        ui,
                        "Clear history and likes",
                        PillKind::Secondary,
                        PillSize::Small,
                        r,
                    )
                    .clicked()
                    {
                        ui.data_mut(|data| data.insert_temp(confirm_id, true));
                    }
                    if pill_button(
                        ui,
                        "Pick favorite genres",
                        PillKind::Secondary,
                        PillSize::Small,
                        r,
                    )
                    .on_hover_text("Choose the genres Home starts from")
                    .clicked()
                    {
                        actions.push(SettingsAction::PickGenres);
                    }
                });
            });
        });
}

fn lastfm_page(
    ui: &mut Ui,
    view: &SettingsView<'_>,
    api_key: &mut String,
    api_secret: &mut String,
    r: &ResponsiveUi,
    actions: &mut Vec<SettingsAction>,
) {
    let theme = Theme::default();
    page_heading(
        ui,
        "Last.fm",
        Some("Connect your Last.fm account to scrobble what you listen to, love songs with ♥, and see your profile and stats in MusiKPL."),
        r,
    );
    let editable = matches!(view.lastfm, LastFmState::Disconnected);
    let width = r.rem(30.0).min(ui.available_width());
    // With MusiKPL's own key built in, connecting takes one click; typing
    // a key of your own is optional.
    let own_keys_id = Id::new("lastfm_own_keys");
    let mut own_keys = ui.data(|data| data.get_temp::<bool>(own_keys_id).unwrap_or(false))
        || !api_key.trim().is_empty();
    if view.one_click && editable {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = r.rem(0.75);
            if pill_button(
                ui,
                "Connect with Last.fm",
                PillKind::Primary,
                PillSize::Medium,
                r,
            )
            .on_hover_text("Opens Last.fm in your browser to approve MusiKPL")
            .clicked()
            {
                actions.push(SettingsAction::LastFmConnect);
            }
            if pill_button(
                ui,
                "Create a Last.fm account",
                PillKind::Secondary,
                PillSize::Medium,
                r,
            )
            .on_hover_text("Opens Last.fm's sign-up page; come back and press Connect")
            .clicked()
            {
                actions.push(SettingsAction::OpenUrl(crate::lastfm::JOIN_URL.into()));
            }
        });
        let toggle_label = if own_keys {
            "Hide my own API key"
        } else {
            "Use my own API key instead"
        };
        if ui
            .add(
                egui::Label::new(
                    egui::RichText::new(toggle_label)
                        .font(fonts::body(r.rem(0.8125)))
                        .color(theme.secondary_text)
                        .underline(),
                )
                .sense(Sense::click()),
            )
            .on_hover_cursor(CursorIcon::PointingHand)
            .clicked()
        {
            own_keys = !own_keys;
            ui.data_mut(|data| data.insert_temp(own_keys_id, own_keys));
        }
    }
    let show_fields = !view.one_click || own_keys;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = r.rem(0.5);
        if !show_fields {
            return;
        }
        caps_label(ui, "API KEY", r);
        text_field(
            ui,
            api_key,
            width,
            FieldOptions {
                id: "lastfm_api_key",
                hint: "32-character API key",
                password: false,
                enabled: editable,
            },
            r,
        );
        ui.add_space(r.rem(0.5));
        caps_label(ui, "SHARED SECRET", r);
        text_field(
            ui,
            api_secret,
            width,
            FieldOptions {
                id: "lastfm_api_secret",
                hint: "Shared secret from the same API account",
                password: true,
                enabled: editable,
            },
            r,
        );
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Don't have them yet? Create an API account on Last.fm.")
                    .font(fonts::body(r.rem(0.8125)))
                    .color(theme.muted_text),
            );
            if ui
                .add(
                    egui::Label::new(
                        egui::RichText::new("Open last.fm/api")
                            .font(fonts::body(r.rem(0.8125)))
                            .color(theme.text)
                            .underline(),
                    )
                    .sense(Sense::click()),
                )
                .on_hover_cursor(CursorIcon::PointingHand)
                .clicked()
            {
                actions.push(SettingsAction::OpenUrl(API_ACCOUNT_URL.into()));
            }
        });
        ui.add_space(r.rem(0.5));
        caps_label(ui, "USERNAME", r);
        let mut username = match view.lastfm {
            LastFmState::Connected { username } => username.to_owned(),
            _ => String::new(),
        };
        text_field(
            ui,
            &mut username,
            width,
            FieldOptions {
                id: "lastfm_username",
                hint: "Filled in when you connect",
                password: false,
                enabled: false,
            },
            r,
        );
    });

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.75);
        match view.lastfm {
            // One-click mode has its Connect button above.
            LastFmState::Disconnected if view.one_click && !show_fields => {
                status(ui, theme.muted_text, "Not connected", r);
            }
            LastFmState::Disconnected => {
                let ready = !api_key.trim().is_empty() && !api_secret.trim().is_empty();
                let connect = pill_button(ui, "Connect", PillKind::Primary, PillSize::Medium, r);
                if connect
                    .on_hover_text(if ready {
                        "Opens Last.fm in your browser to approve MusiKPL"
                    } else {
                        "Enter the API key and shared secret first"
                    })
                    .clicked()
                    && ready
                {
                    actions.push(SettingsAction::LastFmConnect);
                }
                status(ui, theme.muted_text, "Not connected", r);
            }
            LastFmState::Starting => {
                if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Medium, r).clicked() {
                    actions.push(SettingsAction::LastFmCancel);
                }
                spinner(ui, r.rem(1.0));
                status(ui, theme.warning, "Contacting Last.fm…", r);
            }
            LastFmState::AwaitingApproval { url } => {
                if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Medium, r).clicked() {
                    actions.push(SettingsAction::LastFmCancel);
                }
                status(ui, theme.warning, "Approve MusiKPL in your browser…", r);
                if pill_button(
                    ui,
                    "Open page again",
                    PillKind::Secondary,
                    PillSize::Small,
                    r,
                )
                .clicked()
                {
                    actions.push(SettingsAction::OpenUrl(url.to_owned()));
                }
            }
            LastFmState::Connected { username } => {
                if pill_button(ui, "Disconnect", PillKind::Secondary, PillSize::Medium, r).clicked()
                {
                    actions.push(SettingsAction::LastFmDisconnect);
                }
                status(ui, theme.success, &format!("Scrobbling as {username}"), r);
            }
        }
    });
    error_line(ui, view.error, r);
}

fn status(ui: &mut Ui, color: Color32, text: &str, r: &ResponsiveUi) {
    let (dot, _) = ui.allocate_exact_size(r.rem2(0.5, 0.5), Sense::hover());
    ui.painter()
        .circle_filled(dot.center(), dot.width() / 2.0, color);
    ui.label(
        egui::RichText::new(text)
            .font(fonts::body(r.rem(0.875)))
            .color(Theme::default().secondary_text),
    );
}
