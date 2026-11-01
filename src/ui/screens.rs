//! The main area: search results as a grid of cover cards, plus the error
//! toast that floats above it.

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, Id, Order, Rect, Sense, Stroke, Ui,
};

use crate::{
    domain::{Cover, CoverArt, Playlists, Track},
    lastfm::browse::{ArtistHit, TrackHit},
    theme::Theme,
    ui::{
        artist::{artist_card, compact},
        fonts,
        icons::{paint_icon, IconTextures},
        layout::grid_columns,
        menu::{add_to_playlist_menu, PlaylistTarget},
        responsive::ResponsiveUi,
        widgets::{self, elided, hover_amount, item_id, mix, pointer_amount},
    },
};

#[derive(Debug)]
pub(crate) enum ResultAction {
    Play(Track),
    AddToMix(Track),
    ToggleLike(Track),
    AddToPlaylist(Track, PlaylistTarget),
    /// An artist card on the Search screen: open their page.
    OpenArtist(String),
    /// A person found by the search: open their Last.fm profile.
    OpenUser(String),
    /// A song Last.fm found: find it on YouTube, then play it or add it.
    PlayLastFm {
        artist: String,
        title: String,
    },
    AddLastFm {
        artist: String,
        title: String,
    },
}

/// A Last.fm user found by a search, ready to show.
pub(crate) struct PersonCard<'a> {
    pub(crate) name: &'a str,
    pub(crate) scrobbles: u64,
    pub(crate) avatar: Option<Cover>,
}

pub(crate) struct ResultsView<'a> {
    pub(crate) results: &'a [Track],
    /// The query the results are for.
    pub(crate) query: &'a str,
    pub(crate) searching: bool,
    /// Jam guests request songs from the host instead of playing them.
    pub(crate) guest: bool,
    pub(crate) covers: &'a CoverArt,
    pub(crate) in_mix: &'a dyn Fn(&str) -> bool,
    pub(crate) liked: &'a dyn Fn(&str) -> bool,
    pub(crate) playlists: &'a Playlists,
    /// Artists Last.fm found for the search, with their photos.
    pub(crate) artists: &'a [ArtistHit],
    pub(crate) artist_photos: &'a [Option<Cover>],
    /// People (Last.fm users) whose name is the search.
    pub(crate) people: &'a [PersonCard<'a>],
    /// Songs Last.fm found, shown when YouTube's search finds none.
    pub(crate) lastfm_songs: &'a [TrackHit],
    /// A Last.fm song is being looked up on YouTube.
    pub(crate) finding_song: bool,
}

pub(crate) fn show_results(
    ui: &mut Ui,
    area: Rect,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<ResultAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
        let fallback = view.results.is_empty() && !view.searching && !view.lastfm_songs.is_empty();
        if view.results.is_empty() && !fallback {
            empty_state(ui, area, view, &theme, r);
            return;
        }
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(r.rem(1.5)))
                    .show(ui, |ui| {
                        header(ui, view, &theme, r);
                        ui.add_space(r.rem(1.25));
                        artist_strip(ui, view, icons, &theme, r, &mut actions);
                        if fallback {
                            lastfm_songs(ui, view, &theme, r, &mut actions);
                        } else {
                            grid(ui, view, icons, &theme, r, &mut actions);
                        }
                    });
            });
    });
    actions
}

fn header(ui: &mut Ui, view: &ResultsView<'_>, theme: &Theme, r: &ResponsiveUi) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.75);
        ui.label(
            egui::RichText::new(format!("Results for “{}”", view.query))
                .font(fonts::display(r.rem(1.75)))
                .color(theme.text),
        );
        let count = view.results.len();
        ui.label(
            egui::RichText::new(format!("{count} song{}", if count == 1 { "" } else { "s" }))
                .font(fonts::body(r.rem(0.9375)))
                .color(theme.secondary_text),
        );
        if view.searching {
            widgets::spinner(ui, r.rem(1.125));
        }
    });
}

/// Small cards for artists matching the search; each opens their page.
fn artist_strip(
    ui: &mut Ui,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ResultAction>,
) {
    if view.artists.is_empty() && view.people.is_empty() {
        return;
    }
    if !view.people.is_empty() {
        ui.label(
            egui::RichText::new("People")
                .font(fonts::display(r.rem(1.25)))
                .color(theme.text),
        );
        ui.add_space(r.rem(0.375));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = r.rem2(0.5, 0.5);
            for person in view.people {
                let subtitle = format!("{} scrobbles · Last.fm", compact(person.scrobbles));
                if artist_card(
                    ui,
                    r.rem(14.0),
                    person.name,
                    &subtitle,
                    person.avatar.as_ref(),
                    icons,
                    r,
                )
                .on_hover_text("Open profile")
                .clicked()
                {
                    actions.push(ResultAction::OpenUser(person.name.to_owned()));
                }
            }
        });
        ui.add_space(r.rem(1.0));
    }
    if view.artists.is_empty() {
        ui.label(
            egui::RichText::new("Songs")
                .font(fonts::display(r.rem(1.25)))
                .color(theme.text),
        );
        ui.add_space(r.rem(0.375));
        return;
    }
    ui.label(
        egui::RichText::new("Artists")
            .font(fonts::display(r.rem(1.25)))
            .color(theme.text),
    );
    ui.add_space(r.rem(0.375));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = r.rem2(0.5, 0.5);
        for (index, artist) in view.artists.iter().take(6).enumerate() {
            let photo = view.artist_photos.get(index).cloned().flatten();
            let subtitle = format!("{} listeners", compact(artist.listeners));
            if artist_card(
                ui,
                r.rem(14.0),
                &artist.name,
                &subtitle,
                photo.as_ref(),
                icons,
                r,
            )
            .on_hover_text("Open artist")
            .clicked()
            {
                actions.push(ResultAction::OpenArtist(artist.name.clone()));
            }
        }
    });
    ui.add_space(r.rem(1.0));
    ui.label(
        egui::RichText::new("Songs")
            .font(fonts::display(r.rem(1.25)))
            .color(theme.text),
    );
    ui.add_space(r.rem(0.375));
}

/// When YouTube's search finds nothing: the songs Last.fm knows, as rows.
/// Each is looked up on YouTube when played.
fn lastfm_songs(
    ui: &mut Ui,
    view: &ResultsView<'_>,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ResultAction>,
) {
    use crate::ui::widgets::{pill_button, PillKind, PillSize};
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(
                "YouTube's search found nothing, so these come from Last.fm. Picking one finds it on YouTube.",
            )
            .font(fonts::body(r.rem(0.875)))
            .color(theme.secondary_text),
        );
        if view.finding_song {
            widgets::spinner(ui, r.rem(1.0));
        }
    });
    ui.add_space(r.rem(0.5));
    let (add, play) = if view.guest {
        ("Request", "Request")
    } else {
        ("Add to mix", "Play")
    };
    for (index, song) in view.lastfm_songs.iter().enumerate() {
        let width = ui.available_width();
        let (rect, row) = ui.allocate_exact_size(vec2(width, r.rem(3.25)), Sense::hover());
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
        let buttons = Rect::from_min_max(
            pos2(rect.right() - r.rem(13.0), rect.top()),
            pos2(rect.right() - r.rem(0.75), rect.bottom()),
        );
        let text_right = buttons.left() - r.rem(0.75);
        let left = rect.left() + r.rem(0.75);
        let painter = ui.painter();
        elided(
            painter,
            Rect::from_min_max(pos2(left, rect.top()), pos2(text_right, rect.center().y)),
            &song.title,
            fonts::body(r.rem(1.0)),
            theme.text,
            Align2::LEFT_BOTTOM,
        );
        let detail = if song.listeners > 0 {
            format!("{} · {} listeners", song.artist, compact(song.listeners))
        } else {
            song.artist.clone()
        };
        elided(
            painter,
            Rect::from_min_max(
                pos2(left, rect.center().y + r.rem(0.125)),
                pos2(text_right, rect.bottom()),
            ),
            &detail,
            fonts::body(r.rem(0.8125)),
            theme.secondary_text,
            Align2::LEFT_TOP,
        );
        ui.allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(buttons)
                .layout(egui::Layout::right_to_left(egui::Align::Center))
                .id_salt(("lastfm_song", index)),
            |ui| {
                ui.spacing_mut().item_spacing.x = r.rem(0.5);
                let song_of = || (song.artist.clone(), song.title.clone());
                if pill_button(ui, play, PillKind::Primary, PillSize::Small, r).clicked() {
                    let (artist, title) = song_of();
                    actions.push(ResultAction::PlayLastFm { artist, title });
                }
                if !view.guest
                    && pill_button(ui, add, PillKind::Secondary, PillSize::Small, r).clicked()
                {
                    let (artist, title) = song_of();
                    actions.push(ResultAction::AddLastFm { artist, title });
                }
            },
        );
    }
}

fn grid(
    ui: &mut Ui,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ResultAction>,
) {
    let card = card_size(r);
    let gap = r.rem(1.0);
    let columns = grid_columns(ui.available_width(), card.x, gap);
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    for row in view.results.chunks(columns) {
        ui.horizontal(|ui| {
            for track in row {
                result_card(ui, card, track, view, icons, theme, r, actions);
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn result_card(
    ui: &mut Ui,
    size: egui::Vec2,
    track: &Track,
    view: &ResultsView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ResultAction>,
) {
    let cards = CardContext {
        guest: view.guest,
        covers: view.covers,
        in_mix: view.in_mix,
        liked: view.liked,
        playlists: view.playlists,
    };
    if let Some(action) = song_card(ui, size, track, &cards, icons, theme, r) {
        actions.push(action);
    }
}

/// What song cards need to know besides the song.
pub(crate) struct CardContext<'a> {
    pub(crate) guest: bool,
    pub(crate) covers: &'a CoverArt,
    pub(crate) in_mix: &'a dyn Fn(&str) -> bool,
    pub(crate) liked: &'a dyn Fn(&str) -> bool,
    pub(crate) playlists: &'a Playlists,
}

/// The card size used in grids and Home rows.
pub(crate) fn card_size(r: &ResponsiveUi) -> egui::Vec2 {
    vec2(r.rem(11.4375), r.rem(15.3125))
}

/// A song card: cover, title, artist, with Like on the cover's top-right
/// and Add to mix on its bottom-right. Clicking the card plays the song.
/// Cards scrolled out of view only take up space, so their covers aren't
/// downloaded or decoded until they are seen.
pub(crate) fn song_card(
    ui: &mut Ui,
    size: egui::Vec2,
    track: &Track,
    view: &CardContext<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
) -> Option<ResultAction> {
    let (rect, card) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let hover = pointer_amount(ui, &card);
    let rounding = r.rem(0.625);
    ui.painter().rect(
        rect,
        rounding,
        mix(theme.panel, Color32::from_rgb(24, 24, 24), hover),
        Stroke::new(1.0_f32, mix(theme.border, theme.hover, hover)),
    );

    let inner = rect.shrink(r.rem(0.75));
    let cover_rect = Rect::from_min_size(inner.min, egui::Vec2::splat(inner.width()));
    widgets::cover(
        ui,
        cover_rect,
        view.covers.cover_for(track).as_ref(),
        icons,
        r.rem(0.5),
    );

    let title_rect = Rect::from_min_size(
        pos2(inner.left(), cover_rect.bottom() + r.rem(0.625)),
        vec2(inner.width(), r.rem(1.75)),
    );
    elided(
        ui.painter(),
        title_rect,
        &track.title,
        fonts::display(r.rem(1.125)),
        theme.text,
        Align2::LEFT_CENTER,
    );
    let author_rect = title_rect.translate(vec2(0.0, title_rect.height()));
    let author_rect = Rect::from_min_size(author_rect.min, vec2(inner.width(), r.rem(1.375)));
    elided(
        ui.painter(),
        author_rect,
        &track.channel,
        fonts::body(r.rem(0.875)),
        theme.secondary_text,
        Align2::LEFT_CENTER,
    );

    // + on the cover's bottom-right corner: asks whether to add the song to
    // the mix or to a playlist.
    let button_size = r.rem(2.0);
    let button_rect = Rect::from_min_size(
        cover_rect.max - egui::Vec2::splat(button_size + r.rem(0.5)),
        egui::Vec2::splat(button_size),
    );
    let added = !view.guest && (view.in_mix)(&track.id);
    let add = ui.interact(
        button_rect,
        item_id("add_to_mix", &track.id),
        Sense::click(),
    );
    let add_hover = hover_amount(ui, &add);
    let light = if added { 1.0 } else { add_hover };
    let painter = ui.painter();
    painter.circle(
        button_rect.center(),
        button_size / 2.0,
        mix(Color32::from_black_alpha(178), theme.text, light),
        Stroke::new(1.0_f32, Color32::from_white_alpha(38)),
    );
    paint_icon(
        painter,
        if added { &icons.added } else { &icons.add },
        Rect::from_center_size(button_rect.center(), r.rem2(1.0, 1.0)),
        mix(theme.text, Color32::BLACK, light),
    );

    // A small ♥ on the cover's top-right corner marks liked songs; liking
    // happens from the + menu.
    let liked = (view.liked)(&track.id);
    if liked {
        let badge = Rect::from_min_size(
            pos2(
                cover_rect.right() - r.rem(1.75) - r.rem(0.5),
                cover_rect.top() + r.rem(0.5),
            ),
            r.rem2(1.75, 1.75),
        );
        ui.painter().circle_filled(
            badge.center(),
            badge.width() / 2.0,
            Color32::from_black_alpha(178),
        );
        paint_icon(
            ui.painter(),
            &icons.heart_filled,
            Rect::from_center_size(badge.center(), r.rem2(0.875, 0.875)),
            theme.like,
        );
    }

    // Right-click: add to a playlist.
    let mut added_to = None;
    card.context_menu(|ui| {
        added_to = add_to_playlist_menu(ui, view.playlists, &track.id, r);
    });
    if let Some(target) = added_to {
        return Some(ResultAction::AddToPlaylist(track.clone(), target));
    }

    // A click is routed by where the pointer is, not by which widget egui
    // picked: inside a scrolling row it sometimes handed a click on + to the
    // card underneath, which then played the song.
    let pointer = ui.input(|input| input.pointer.interact_pos());
    let on_button = pointer.is_some_and(|pointer| button_rect.contains(pointer));
    let clicked = card.clicked() || add.clicked();

    let popup = item_id("add_popup", &track.id);
    let add = add.on_hover_cursor(CursorIcon::PointingHand);
    let add = if ui.memory(|memory| memory.is_popup_open(popup)) {
        add
    } else {
        add.on_hover_text("Like, add to the mix or a playlist")
    };
    if clicked && on_button {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    let picked = egui::popup::popup_below_widget(
        ui,
        popup,
        &add,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| add_menu(ui, track, view, added, liked, r),
    )
    .flatten();
    if let Some(action) = picked {
        ui.memory_mut(|memory| memory.close_popup());
        return Some(action);
    }
    if on_button {
        return None;
    }
    let play = card
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(if view.guest {
            "Add to the Jam queue"
        } else {
            "Play · right-click to add to a playlist"
        })
        .clicked();
    play.then(|| ResultAction::Play(track.clone()))
}
/// The + menu: like the song, add it to the mix (or to the Jam's queue
/// host), or add it to a playlist.
fn add_menu(
    ui: &mut Ui,
    track: &Track,
    view: &CardContext<'_>,
    in_mix: bool,
    liked: bool,
    r: &ResponsiveUi,
) -> Option<ResultAction> {
    ui.set_min_width(r.rem(14.0));
    let like = ui.add(
        egui::Button::new(
            egui::RichText::new(if liked { "♥ Unlike" } else { "♥ Like" })
                .font(fonts::body(r.rem(0.9375))),
        )
        .frame(false),
    );
    if like.clicked() {
        return Some(ResultAction::ToggleLike(track.clone()));
    }
    let label = match (view.guest, in_mix) {
        (true, _) => "Add to the Jam queue",
        (false, true) => "✓ In your mix",
        (false, false) => "Add to mix",
    };
    let mix = ui.add_enabled(
        !in_mix,
        egui::Button::new(egui::RichText::new(label).font(fonts::body(r.rem(0.9375)))).frame(false),
    );
    if mix.clicked() {
        return Some(ResultAction::AddToMix(track.clone()));
    }
    ui.separator();
    add_to_playlist_menu(ui, view.playlists, &track.id, r)
        .map(|target| ResultAction::AddToPlaylist(track.clone(), target))
}

fn empty_state(ui: &mut Ui, area: Rect, view: &ResultsView<'_>, theme: &Theme, r: &ResponsiveUi) {
    let (title, detail) = if view.searching {
        (
            format!("Searching for “{}”…", view.query),
            "Looking on YouTube".to_owned(),
        )
    } else if view.query.is_empty() {
        (
            "Search for music".to_owned(),
            "Type a song, artist, album, or YouTube link above and press Enter.".to_owned(),
        )
    } else {
        (
            format!("No results for “{}”", view.query),
            "Try another search.".to_owned(),
        )
    };
    let center = area.center();
    let painter = ui.painter();
    painter.text(
        center - vec2(0.0, r.rem(1.0)),
        Align2::CENTER_CENTER,
        title,
        fonts::display(r.rem(2.0)),
        theme.text,
    );
    painter.text(
        center + vec2(0.0, r.rem(1.25)),
        Align2::CENTER_CENTER,
        detail,
        fonts::body(r.rem(1.0)),
        theme.secondary_text,
    );
    if view.searching {
        let spinner = Rect::from_center_size(center + vec2(0.0, r.rem(3.5)), r.rem2(1.5, 1.5));
        ui.put(spinner, |ui: &mut Ui| widgets::spinner(ui, r.rem(1.5)));
    }
}

/// A dismissable error card at the bottom-left of the main area. Returns
/// true when the user closed it.
pub(crate) fn error_toast(
    ctx: &egui::Context,
    main: Rect,
    message: &str,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> bool {
    let theme = Theme::default();
    let width = r.rem(26.0).min(main.width() - r.rem(3.0));
    let font = fonts::body(r.rem(0.9375));
    egui::Area::new(Id::new("error_toast"))
        .order(Order::Foreground)
        .fixed_pos(pos2(main.left() + r.rem(1.5), main.bottom() - r.rem(1.5)))
        .pivot(Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            let frame = egui::Frame::none()
                .fill(theme.panel)
                .stroke(Stroke::new(1.0_f32, theme.danger))
                .rounding(r.rem(0.75))
                .inner_margin(egui::Margin::symmetric(r.rem(1.0), r.rem(0.75)));
            let mut closed = false;
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal_top(|ui| {
                    let button = r.rem(1.75);
                    let gap = r.rem(0.75);
                    ui.spacing_mut().item_spacing.x = gap;
                    ui.allocate_ui(vec2(width - button - gap, 0.0), |ui| {
                        ui.set_width(width - button - gap);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(message)
                                    .font(font.clone())
                                    .color(theme.text),
                            )
                            .wrap(),
                        );
                    });
                    let close = widgets::icon_button(
                        ui,
                        &icons.close,
                        vec2(button, button),
                        r.rem2(0.875, 0.875),
                        button / 2.0,
                        widgets::ButtonColors::round(&theme),
                    );
                    closed = close.on_hover_text("Dismiss").clicked();
                });
            });
            closed
        })
        .inner
}
