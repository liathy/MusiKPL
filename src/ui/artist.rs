//! Last.fm artists: the small artist card used across the app, and an
//! artist's page (photo, stats, genres, bio, top songs, similar artists).

use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Ui};

use crate::{
    domain::Cover,
    lastfm::browse::ArtistPage,
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        responsive::ResponsiveUi,
        widgets::{self, elided, hover_amount, item_id, mix, pill_button, PillKind, PillSize},
    },
};

/// "1.2M", "34.5K", "812".
pub(crate) fn compact(number: u64) -> String {
    match number {
        0..=999 => number.to_string(),
        1_000..=999_999 => format!("{:.1}K", number as f64 / 1_000.0).replace(".0K", "K"),
        _ => format!("{:.1}M", number as f64 / 1_000_000.0).replace(".0M", "M"),
    }
}

/// A round photo: the artist's (or user's) picture once it has loaded, and
/// their initial until then (or if it can't be loaded).
pub(crate) fn round_photo(
    ui: &Ui,
    rect: Rect,
    name: &str,
    photo: Option<&Cover>,
    _icons: &IconTextures,
) {
    let theme = Theme::default();
    let radius = rect.width() / 2.0;
    let painted = photo.is_some_and(|photo| {
        ui.painter()
            .circle_filled(rect.center(), radius, theme.surface);
        widgets::cover_image(ui, rect, photo, radius)
    });
    if !painted {
        ui.painter()
            .circle_filled(rect.center(), radius, theme.hover);
        let initial: String = name
            .chars()
            .next()
            .map(char::to_uppercase)
            .into_iter()
            .flatten()
            .collect();
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            initial,
            fonts::display(rect.height() * 0.45),
            theme.text,
        );
    }
}
/// A compact card: round photo, name, and a line under it. Returns the
/// card's response (clicked = open).
pub(crate) fn artist_card(
    ui: &mut Ui,
    width: f32,
    name: &str,
    subtitle: &str,
    photo: Option<&Cover>,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> egui::Response {
    let theme = Theme::default();
    let (rect, response) = ui.allocate_exact_size(vec2(width, r.rem(3.5)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return response;
    }
    fonts::cover(ui.ctx(), name);
    let hover = hover_amount(ui, &response);
    ui.painter()
        .rect_filled(rect, r.rem(0.625), mix(theme.surface, theme.hover, hover));
    let photo_rect = Rect::from_min_size(
        pos2(rect.left() + r.rem(0.375), rect.center().y - r.rem(1.375)),
        r.rem2(2.75, 2.75),
    );
    round_photo(ui, photo_rect, name, photo, icons);
    let text = Rect::from_min_max(
        pos2(photo_rect.right() + r.rem(0.625), rect.top()),
        pos2(rect.right() - r.rem(0.5), rect.bottom()),
    );
    elided(
        ui.painter(),
        Rect::from_min_size(
            pos2(text.left(), rect.center().y - r.rem(1.125)),
            vec2(text.width(), r.rem(1.375)),
        ),
        name,
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
        subtitle,
        fonts::body(r.rem(0.75)),
        theme.secondary_text,
        Align2::LEFT_CENTER,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

#[derive(Debug, PartialEq)]
pub(crate) enum ArtistAction {
    ToggleBookmark,
    Play(String),
    /// A choice from a top song's + menu.
    Add(String, SongPick),
    OpenArtist(String),
    OpenUrl(String),
}

/// What a top song's + menu does with it, once it's found on YouTube.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SongPick {
    Like,
    Mix,
    Playlist(crate::ui::menu::PlaylistTarget),
}

pub(crate) struct ArtistView<'a> {
    pub(crate) name: &'a str,
    pub(crate) page: Option<&'a Result<ArtistPage, String>>,
    pub(crate) photo: Option<Cover>,
    pub(crate) bookmarked: bool,
    /// A song is being found on YouTube.
    pub(crate) finding_song: bool,
    /// Photos of the similar artists, in order.
    pub(crate) similar_photos: Vec<Option<Cover>>,
    pub(crate) guest: bool,
    pub(crate) playlists: &'a crate::domain::Playlists,
}

pub(crate) fn show_artist(
    ui: &mut Ui,
    area: Rect,
    view: &ArtistView<'_>,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<ArtistAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
        ui.set_clip_rect(area);
        egui::ScrollArea::vertical()
            .id_salt(("artist_scroll", view.name))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(r.rem(1.5)))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        header(ui, view, icons, &theme, r, &mut actions);
                        match view.page {
                            None => {
                                ui.add_space(r.rem(1.5));
                                ui.horizontal(|ui| {
                                    widgets::spinner(ui, r.rem(1.25));
                                    note(ui, "Loading from Last.fm…", theme.secondary_text, r);
                                });
                            }
                            Some(Err(error)) => {
                                ui.add_space(r.rem(1.5));
                                note(ui, error, theme.danger, r);
                            }
                            Some(Ok(page)) => body(ui, page, view, icons, &theme, r, &mut actions),
                        }
                    });
            });
    });
    actions
}

fn note(ui: &mut Ui, text: &str, color: Color32, r: &ResponsiveUi) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(fonts::body(r.rem(0.9375)))
                .color(color),
        )
        .wrap(),
    );
}

fn header(
    ui: &mut Ui,
    view: &ArtistView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ArtistAction>,
) {
    let page = view.page.and_then(|page| page.as_ref().ok());
    let name = page.map_or(view.name, |page| page.name.as_str());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(1.5);
        let (photo, _) = ui.allocate_exact_size(r.rem2(9.0, 9.0), Sense::hover());
        round_photo(ui, photo, name, view.photo.as_ref(), icons);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = r.rem(0.5);
            ui.add_space(r.rem(0.75));
            fonts::cover(ui.ctx(), name);
            ui.label(
                egui::RichText::new(name)
                    .font(fonts::display(r.rem(2.25)))
                    .color(theme.text),
            );
            if let Some(page) = page {
                ui.label(
                    egui::RichText::new(format!(
                        "{} listeners · {} plays on Last.fm",
                        compact(page.listeners),
                        compact(page.plays)
                    ))
                    .font(fonts::body(r.rem(0.9375)))
                    .color(theme.secondary_text),
                );
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = r.rem(0.5);
                let (label, kind) = if view.bookmarked {
                    ("✓ Bookmarked", PillKind::Secondary)
                } else {
                    ("Bookmark", PillKind::Primary)
                };
                if pill_button(ui, label, kind, PillSize::Medium, r)
                    .on_hover_text(
                        "Bookmarked artists show in the menu and shape Home and Auto Mix",
                    )
                    .clicked()
                {
                    actions.push(ArtistAction::ToggleBookmark);
                }
                if let Some(page) = page.filter(|page| page.url.starts_with("https://")) {
                    if pill_button(
                        ui,
                        "Open on Last.fm",
                        PillKind::Secondary,
                        PillSize::Medium,
                        r,
                    )
                    .clicked()
                    {
                        actions.push(ArtistAction::OpenUrl(page.url.clone()));
                    }
                }
                if view.finding_song {
                    widgets::spinner(ui, r.rem(1.125));
                }
            });
        });
    });
}

#[allow(clippy::too_many_arguments)]
fn body(
    ui: &mut Ui,
    page: &ArtistPage,
    view: &ArtistView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ArtistAction>,
) {
    if !page.tags.is_empty() {
        ui.add_space(r.rem(1.0));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = r.rem2(0.375, 0.375);
            for tag in &page.tags {
                tag_chip(ui, tag, theme, r);
            }
        });
    }
    if !page.bio.is_empty() {
        ui.add_space(r.rem(0.75));
        note(ui, &page.bio, theme.secondary_text, r);
    }

    if !page.top_tracks.is_empty() {
        section(ui, "Top songs", theme, r);
        ui.spacing_mut().item_spacing.y = r.rem(0.125);
        for (index, track) in page.top_tracks.iter().enumerate() {
            song_row(
                ui,
                index,
                &track.title,
                track.plays,
                view,
                icons,
                theme,
                r,
                actions,
            );
        }
    }

    if !page.similar.is_empty() {
        section(ui, "Similar artists", theme, r);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = r.rem2(0.5, 0.5);
            for (index, name) in page.similar.iter().enumerate() {
                let photo = view.similar_photos.get(index).cloned().flatten();
                if artist_card(ui, r.rem(14.0), name, "Artist", photo.as_ref(), icons, r).clicked()
                {
                    actions.push(ArtistAction::OpenArtist(name.clone()));
                }
            }
        });
    }
}

pub(crate) fn section(ui: &mut Ui, title: &str, theme: &Theme, r: &ResponsiveUi) {
    ui.add_space(r.rem(1.5));
    ui.label(
        egui::RichText::new(title)
            .font(fonts::display(r.rem(1.375)))
            .color(theme.text),
    );
    ui.add_space(r.rem(0.25));
}

pub(crate) fn tag_chip(ui: &mut Ui, tag: &str, theme: &Theme, r: &ResponsiveUi) {
    let font = fonts::body(r.rem(0.8125));
    let galley = ui
        .painter()
        .layout_no_wrap(tag.to_owned(), font.clone(), theme.text);
    let (rect, _) =
        ui.allocate_exact_size(galley.size() + r.rem2(0.75, 0.25) * 2.0, Sense::hover());
    ui.painter()
        .rect_filled(rect, rect.height() / 2.0, theme.surface);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        tag,
        font,
        theme.secondary_text,
    );
}

/// A top song: number, title, plays, and + on hover, with the same menu as
/// song cards (like, add to the mix, add to a playlist). Clicking plays it
/// (it is found on YouTube first).
#[allow(clippy::too_many_arguments)]
fn song_row(
    ui: &mut Ui,
    index: usize,
    title: &str,
    plays: u64,
    view: &ArtistView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ArtistAction>,
) {
    let (rect, row) =
        ui.allocate_exact_size(vec2(ui.available_width(), r.rem(3.0)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let hover = widgets::pointer_amount(ui, &row);
    ui.painter().rect_filled(
        rect,
        r.rem(0.5),
        mix(
            Color32::TRANSPARENT,
            Color32::from_rgba_unmultiplied(34, 34, 34, 153),
            hover,
        ),
    );
    let inner = rect.shrink2(r.rem2(0.75, 0.0));
    ui.painter().text(
        pos2(inner.left() + r.rem(0.75), inner.center().y),
        Align2::CENTER_CENTER,
        (index + 1).to_string(),
        fonts::body(r.rem(0.9375)),
        theme.muted_text,
    );
    let add = Rect::from_center_size(
        pos2(inner.right() - r.rem(1.0), inner.center().y),
        r.rem2(2.0, 2.0),
    );
    elided(
        ui.painter(),
        Rect::from_min_max(
            pos2(inner.left() + r.rem(2.0), inner.top()),
            pos2(add.left() - r.rem(6.0), inner.bottom()),
        ),
        title,
        fonts::display(r.rem(1.0)),
        theme.text,
        Align2::LEFT_CENTER,
    );
    ui.painter().text(
        pos2(add.left() - r.rem(0.75), inner.center().y),
        Align2::RIGHT_CENTER,
        format!("{} plays", compact(plays)),
        fonts::body(r.rem(0.8125)),
        theme.secondary_text,
    );
    let pointer = ui.input(|input| input.pointer.interact_pos());
    let on_add = pointer.is_some_and(|pointer| add.contains(pointer));
    let button = ui.interact(add, item_id("artist_add", title), Sense::click());
    let popup = item_id("artist_add_popup", title);
    let popup_open = ui.memory(|memory| memory.is_popup_open(popup));
    // The + stays while its menu is open, even with the pointer on the menu.
    if row.contains_pointer() || popup_open {
        let colors = widgets::ButtonColors::round(theme);
        widgets::paint_icon_button(
            ui,
            add,
            &button,
            &icons.add,
            r.rem2(0.875, 0.875),
            add.width() / 2.0,
            colors,
        );
    }
    let clicked = row.clicked() || button.clicked();
    let button = button.on_hover_cursor(CursorIcon::PointingHand);
    let button = if popup_open {
        button
    } else {
        button.on_hover_text("Like, add to the mix or a playlist")
    };
    if clicked && on_add {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    let picked = egui::popup::popup_below_widget(
        ui,
        popup,
        &button,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| add_menu(ui, view, r),
    )
    .flatten();
    if let Some(pick) = picked {
        ui.memory_mut(|memory| memory.close_popup());
        actions.push(ArtistAction::Add(title.to_owned(), pick));
        return;
    }
    if on_add {
        return;
    }
    if row
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if view.guest {
            "Add to the Jam queue"
        } else {
            "Play"
        })
        .clicked()
    {
        actions.push(ArtistAction::Play(title.to_owned()));
    }
}

/// A top song's + menu. It isn't on YouTube yet, so whether it's liked or
/// in a playlist isn't known; liking only ever likes.
fn add_menu(ui: &mut Ui, view: &ArtistView<'_>, r: &ResponsiveUi) -> Option<SongPick> {
    ui.set_min_width(r.rem(14.0));
    let item = |ui: &mut Ui, text: &str| {
        ui.add(
            egui::Button::new(egui::RichText::new(text).font(fonts::body(r.rem(0.9375))))
                .frame(false),
        )
        .clicked()
    };
    if item(ui, "♥ Like") {
        return Some(SongPick::Like);
    }
    if item(
        ui,
        if view.guest {
            "Add to the Jam queue"
        } else {
            "Add to mix"
        },
    ) {
        return Some(SongPick::Mix);
    }
    ui.separator();
    crate::ui::menu::add_to_playlist_menu(ui, view.playlists, "", r).map(SongPick::Playlist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_numbers_are_short() {
        assert_eq!(compact(812), "812");
        assert_eq!(compact(34_500), "34.5K");
        assert_eq!(compact(2_000), "2K");
        assert_eq!(compact(1_250_000), "1.2M");
    }
}
