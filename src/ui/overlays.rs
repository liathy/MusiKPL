//! Floating panels: the menu and queue drawers, and the settings modal.
//! Each slides or fades in over a backdrop that closes it when clicked.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, Id, Order, Rect, Sense, Shadow, Stroke, Ui,
};

use crate::{
    domain::{CoverArt, Track},
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        jam::{self, JamAction, JamView},
        layout::{drawer_rect, modal_rect, Side, WindowLayout},
        responsive::ResponsiveUi,
        widgets::{
            self, caps_label, checkbox_row, elided, hover_amount, icon_button, item_id, mix,
            pill_button, pointer_amount, spinner, text_field, toggle, ButtonColors, FieldOptions,
            PillKind, PillSize,
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
    ToggleAutoMix,
    Jam(JamAction),
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn show_menu(
    ctx: &egui::Context,
    window: Rect,
    layout: &WindowLayout,
    open: f32,
    icons: &IconTextures,
    auto_mix: bool,
    jam_view: &JamView<'_>,
    jam_invite: &mut String,
    r: &ResponsiveUi,
) -> Vec<MenuAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    if backdrop(ctx, "menu", window, 0.0) {
        actions.push(MenuAction::Close);
    }
    let rect = drawer_rect(layout, Side::Left, r.rem(18.75), r);
    let slide = (rect.right() - window.left()) * (1.0 - open);
    panel(
        ctx,
        "menu",
        rect.translate(vec2(-slide, 0.0)),
        open,
        r.rem(1.25),
        r,
        |ui| {
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
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = r.rem(1.0);
                    if checkbox_row(ui, icons, auto_mix, "Auto Mix", r)
                        .on_hover_text("Keep the queue filled with songs like the one playing")
                        .clicked()
                    {
                        actions.push(MenuAction::ToggleAutoMix);
                    }
                    let (line, _) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().rect_filled(line, 0.0, theme.border);
                    actions.extend(
                        jam::show(ui, jam_view, jam_invite, r)
                            .into_iter()
                            .map(MenuAction::Jam),
                    );
                });
        },
    );
    actions
}

// ── Queue ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) enum QueueAction {
    Close,
    Play(Track),
    Remove(usize),
}

pub(crate) struct QueueView<'a> {
    pub(crate) now_playing: Option<&'a Track>,
    pub(crate) queue: &'a [Track],
    pub(crate) mixing: bool,
    pub(crate) guest: bool,
    pub(crate) covers: &'a CoverArt,
}

pub(crate) fn show_queue(
    ctx: &egui::Context,
    window: Rect,
    layout: &WindowLayout,
    open: f32,
    icons: &IconTextures,
    view: &QueueView<'_>,
    r: &ResponsiveUi,
) -> Vec<QueueAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    if backdrop(ctx, "queue", window, 0.0) {
        actions.push(QueueAction::Close);
    }
    let rect = drawer_rect(layout, Side::Right, r.rem(25.0), r);
    let slide = (window.right() - rect.left()) * (1.0 - open);
    panel(
        ctx,
        "queue",
        rect.translate(vec2(slide, 0.0)),
        open,
        r.rem(1.25),
        r,
        |ui| {
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
                            "The host controls the queue. Press + on a song to request it."
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
        },
    );
    actions
}

/// One queue row. Clicking plays it; hovering shows a remove button in
/// place of the duration.
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
    let clickable = index.is_some() && !view.guest;
    let (rect, row) = ui.allocate_exact_size(
        vec2(ui.available_width(), r.rem(4.0)),
        if clickable {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let hover = pointer_amount(ui, &row);
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

    let trailing = r.rem(2.75);
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

    let trailing_rect = Rect::from_min_max(pos2(inner.right() - trailing, inner.top()), inner.max);
    let removable = clickable && row.contains_pointer();
    if let (true, Some(index)) = (removable, index) {
        let button = Rect::from_center_size(trailing_rect.center(), r.rem2(1.75, 1.75));
        let remove = ui.interact(
            button,
            item_id("queue_remove", &format!("{index}:{}", track.id)),
            Sense::click(),
        );
        widgets::paint_icon_button(
            ui,
            button,
            &remove,
            &icons.close,
            r.rem2(0.875, 0.875),
            r.rem(0.875),
            ButtonColors::round(&theme),
        );
        if remove
            .on_hover_cursor(CursorIcon::PointingHand)
            .on_hover_text("Remove from queue")
            .clicked()
        {
            actions.push(QueueAction::Remove(index));
            return;
        }
    } else {
        ui.painter().text(
            pos2(trailing_rect.right(), trailing_rect.center().y),
            Align2::RIGHT_CENTER,
            track.duration_text(),
            fonts::body(r.rem(0.8125)),
            theme.secondary_text,
        );
    }
    if clickable && row.on_hover_cursor(CursorIcon::PointingHand).clicked() {
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
    LastFmConnect,
    LastFmCancel,
    LastFmDisconnect,
    OpenUrl(String),
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
    pub(crate) lastfm: LastFmState<'a>,
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
    let theme = Theme::default();
    page_heading(ui, "General", None, r);
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
                                egui::RichText::new("Start with Windows")
                                    .font(fonts::display(r.rem(1.125)))
                                    .color(theme.text),
                            );
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(
                                        "Open MusiKPL automatically when you sign in to Windows.",
                                    )
                                    .font(fonts::body(r.rem(0.875)))
                                    .color(theme.secondary_text),
                                )
                                .wrap(),
                            );
                        });
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if toggle(ui, view.start_with_windows, r).clicked() {
                        actions.push(SettingsAction::SetStartWithWindows(
                            !view.start_with_windows,
                        ));
                    }
                });
            });
        });
    error_line(ui, view.error, r);
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
        Some("Connect your Last.fm account to scrobble the songs you listen to. Album covers come from Last.fm too when it knows the song."),
        r,
    );
    let editable = matches!(view.lastfm, LastFmState::Disconnected);
    let width = r.rem(30.0).min(ui.available_width());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = r.rem(0.5);
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
