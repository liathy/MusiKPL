//! The connected user's Last.fm profile: avatar, totals, and their top
//! artists, top songs and recent plays for a chosen period.

use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Ui};

use crate::{
    domain::Cover,
    lastfm::browse::{Period, Profile, ProfileTrack},
    theme::Theme,
    ui::{
        artist::{artist_card, compact, round_photo, section},
        fonts,
        icons::IconTextures,
        responsive::ResponsiveUi,
        widgets::{self, elided, hover_amount, mix, pill_button, PillKind, PillSize},
    },
};

#[derive(Debug, PartialEq)]
pub(crate) enum ProfileAction {
    Period(Period),
    OpenArtist(String),
    Play { artist: String, title: String },
    OpenUrl(String),
    Connect,
    AddFriend,
    RemoveFriend,
    CopyFriendCode,
}

pub(crate) struct ProfileView<'a> {
    pub(crate) username: &'a str,
    pub(crate) profile: Option<&'a Result<Profile, String>>,
    pub(crate) loading: bool,
    pub(crate) period: Period,
    pub(crate) avatar: Option<Cover>,
    /// Photos of the top artists, in order.
    pub(crate) artist_photos: Vec<Option<Cover>>,
    /// Unix seconds now, for "5 min ago".
    pub(crate) now: u64,
    /// The profile is the user's own (else someone else's).
    pub(crate) is_me: bool,
    /// Someone else's profile: whether they are a friend.
    pub(crate) is_friend: bool,
}

pub(crate) fn show(
    ui: &mut Ui,
    area: Rect,
    view: &ProfileView<'_>,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<ProfileAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(area), |ui| {
        ui.set_clip_rect(area);
        egui::ScrollArea::vertical()
            .id_salt("profile_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::none()
                    .inner_margin(egui::Margin::same(r.rem(1.5)))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if view.username.is_empty() {
                            not_connected(ui, &theme, r, &mut actions);
                            return;
                        }
                        let profile = view.profile.and_then(|profile| profile.as_ref().ok());
                        header(ui, view, profile, icons, &theme, r, &mut actions);
                        ui.add_space(r.rem(1.0));
                        period_picker(ui, view.period, &theme, r, &mut actions);
                        match view.profile {
                            Some(Err(error)) => {
                                ui.add_space(r.rem(1.0));
                                label(ui, error, theme.danger, r);
                            }
                            Some(Ok(profile)) => {
                                stats(ui, profile, view, icons, &theme, r, &mut actions)
                            }
                            None => {}
                        }
                    });
            });
    });
    actions
}

fn label(ui: &mut Ui, text: &str, color: Color32, r: &ResponsiveUi) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(fonts::body(r.rem(0.9375)))
                .color(color),
        )
        .wrap(),
    );
}

fn not_connected(ui: &mut Ui, theme: &Theme, r: &ResponsiveUi, actions: &mut Vec<ProfileAction>) {
    ui.add_space(r.rem(3.0));
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = r.rem(0.75);
        ui.label(
            egui::RichText::new("Your Last.fm profile")
                .font(fonts::display(r.rem(1.75)))
                .color(theme.text),
        );
        label(
            ui,
            "Connect Last.fm to see your scrobbles, top artists and songs, and to love songs from MusiKPL.",
            theme.secondary_text,
            r,
        );
        if pill_button(ui, "Connect Last.fm", PillKind::Primary, PillSize::Medium, r).clicked() {
            actions.push(ProfileAction::Connect);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn header(
    ui: &mut Ui,
    view: &ProfileView<'_>,
    profile: Option<&Profile>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ProfileAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(1.5);
        let (photo, _) = ui.allocate_exact_size(r.rem2(7.5, 7.5), Sense::hover());
        round_photo(ui, photo, view.username, view.avatar.as_ref(), icons);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = r.rem(0.375);
            ui.add_space(r.rem(0.5));
            ui.label(
                egui::RichText::new(view.username)
                    .font(fonts::display(r.rem(2.25)))
                    .color(theme.text),
            );
            if let Some(profile) = profile {
                let mut facts = vec![
                    format!("{} scrobbles", compact(profile.scrobbles)),
                    format!("{} artists", compact(profile.artists)),
                    format!("{} loved", compact(profile.loved)),
                ];
                if let Some(year) = year_of(profile.registered) {
                    facts.push(format!("since {year}"));
                }
                if !profile.country.is_empty() {
                    facts.push(profile.country.clone());
                }
                label(ui, &facts.join(" · "), theme.secondary_text, r);
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = r.rem(0.5);
                if view.is_me {
                    if pill_button(ui, "Copy my friend code", PillKind::Primary, PillSize::Small, r)
                        .on_hover_text(
                            "Send it to friends: once they add it (and you add theirs), you see each other online and can invite each other to Jams",
                        )
                        .clicked()
                    {
                        actions.push(ProfileAction::CopyFriendCode);
                    }
                } else if view.is_friend {
                    if pill_button(ui, "✓ Friend", PillKind::Secondary, PillSize::Small, r)
                        .on_hover_text("Click to remove from your friends")
                        .clicked()
                    {
                        actions.push(ProfileAction::RemoveFriend);
                    }
                } else if pill_button(ui, "Add friend", PillKind::Primary, PillSize::Small, r)
                    .on_hover_text("Adds them to Friends in the menu")
                    .clicked()
                {
                    actions.push(ProfileAction::AddFriend);
                }
                if let Some(profile) = profile.filter(|profile| profile.url.starts_with("https://"))
                {
                    if pill_button(
                        ui,
                        "Open on Last.fm",
                        PillKind::Secondary,
                        PillSize::Small,
                        r,
                    )
                    .clicked()
                    {
                        actions.push(ProfileAction::OpenUrl(profile.url.clone()));
                    }
                }
                if view.loading {
                    widgets::spinner(ui, r.rem(1.125));
                }
            });
        });
    });
}

fn period_picker(
    ui: &mut Ui,
    current: Period,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ProfileAction>,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.375);
        for period in Period::ALL {
            let selected = period == current;
            let font = fonts::body(r.rem(0.875));
            let galley =
                ui.painter()
                    .layout_no_wrap(period.label().to_owned(), font.clone(), theme.text);
            let (rect, response) =
                ui.allocate_exact_size(galley.size() + r.rem2(0.875, 0.375) * 2.0, Sense::click());
            let hover = hover_amount(ui, &response);
            let fill = if selected {
                theme.text
            } else {
                mix(theme.surface, theme.hover, hover)
            };
            ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                period.label(),
                font,
                if selected { Color32::BLACK } else { theme.text },
            );
            if response.on_hover_cursor(CursorIcon::PointingHand).clicked() && !selected {
                actions.push(ProfileAction::Period(period));
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn stats(
    ui: &mut Ui,
    profile: &Profile,
    view: &ProfileView<'_>,
    icons: &IconTextures,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ProfileAction>,
) {
    if !profile.top_artists.is_empty() {
        section(ui, "Top artists", theme, r);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = r.rem2(0.5, 0.5);
            for (index, artist) in profile.top_artists.iter().enumerate() {
                let photo = view.artist_photos.get(index).cloned().flatten();
                let subtitle = format!("{} plays", compact(artist.listeners));
                if artist_card(
                    ui,
                    r.rem(14.0),
                    &artist.name,
                    &subtitle,
                    photo.as_ref(),
                    icons,
                    r,
                )
                .clicked()
                {
                    actions.push(ProfileAction::OpenArtist(artist.name.clone()));
                }
            }
        });
    }
    if !profile.top_tracks.is_empty() {
        section(ui, "Top songs", theme, r);
        ui.spacing_mut().item_spacing.y = r.rem(0.125);
        for track in &profile.top_tracks {
            let detail = format!("{} plays", compact(track.plays));
            track_row(ui, track, &detail, theme, r, actions);
        }
    }
    if !profile.recent.is_empty() {
        section(ui, "Recently played", theme, r);
        ui.spacing_mut().item_spacing.y = r.rem(0.125);
        for track in &profile.recent {
            let detail = if track.now_playing {
                "Playing now".to_owned()
            } else {
                ago(view.now, track.when)
            };
            track_row(ui, track, &detail, theme, r, actions);
        }
    }
}

/// A song row: "Title — Artist" and a detail on the right; clicking plays it.
fn track_row(
    ui: &mut Ui,
    track: &ProfileTrack,
    detail: &str,
    theme: &Theme,
    r: &ResponsiveUi,
    actions: &mut Vec<ProfileAction>,
) {
    let (rect, row) =
        ui.allocate_exact_size(vec2(ui.available_width(), r.rem(3.0)), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    fonts::cover(ui.ctx(), &track.title);
    fonts::cover(ui.ctx(), &track.artist);
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
    let detail_width = r.rem(8.0);
    elided(
        ui.painter(),
        Rect::from_min_max(
            inner.min,
            pos2(
                inner.right() - detail_width,
                inner.center().y + r.rem(0.125),
            ),
        ),
        &track.title,
        fonts::display(r.rem(1.0)),
        theme.text,
        Align2::LEFT_BOTTOM,
    );
    elided(
        ui.painter(),
        Rect::from_min_max(
            pos2(inner.left(), inner.center().y + r.rem(0.125)),
            pos2(inner.right() - detail_width, inner.bottom()),
        ),
        &track.artist,
        fonts::body(r.rem(0.8125)),
        theme.secondary_text,
        Align2::LEFT_TOP,
    );
    ui.painter().text(
        pos2(inner.right(), inner.center().y),
        Align2::RIGHT_CENTER,
        detail,
        fonts::body(r.rem(0.8125)),
        if track.now_playing {
            theme.success
        } else {
            theme.secondary_text
        },
    );
    if row
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Play")
        .clicked()
    {
        actions.push(ProfileAction::Play {
            artist: track.artist.clone(),
            title: track.title.clone(),
        });
    }
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
fn ago(now: u64, when: u64) -> String {
    let seconds = now.saturating_sub(when);
    match seconds {
        0..=59 => "just now".into(),
        60..=3_599 => format!("{} min ago", seconds / 60),
        3_600..=86_399 => format!("{} h ago", seconds / 3_600),
        _ => {
            let days = seconds / 86_400;
            format!("{days} day{} ago", if days == 1 { "" } else { "s" })
        }
    }
}

/// The year of a Unix time, for "since 2015".
fn year_of(unix: u64) -> Option<u64> {
    if unix == 0 {
        return None;
    }
    // Days since 1970 to a civil year (proleptic Gregorian).
    let days = unix / 86_400;
    let era_days = days + 719_468;
    let era = era_days / 146_097;
    let day_of_era = era_days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let year = year_of_era + era * 400;
    Some(if month_index >= 10 { year + 1 } else { year })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_naturally() {
        assert_eq!(ago(1_000, 990), "just now");
        assert_eq!(ago(10_000, 10_000 - 300), "5 min ago");
        assert_eq!(ago(100_000, 100_000 - 7_200), "2 h ago");
        assert_eq!(ago(1_000_000, 1_000_000 - 86_400), "1 day ago");
    }

    #[test]
    fn years_are_right() {
        assert_eq!(year_of(1_500_000_000), Some(2017));
        assert_eq!(year_of(1_704_067_200), Some(2024)); // 2024-01-01
        assert_eq!(year_of(1_704_067_199), Some(2023));
        assert_eq!(year_of(0), None);
    }
}
