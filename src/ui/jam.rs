//! The Jam section of the menu: start a Jam, join a nearby one, and, during
//! a session, its status and song requests.

use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, Rect, Sense, Stroke, Ui};

use crate::{
    jam::{RequestStatus, Role},
    theme::Theme,
    ui::{
        fonts,
        responsive::ResponsiveUi,
        widgets::{
            elided, hover_amount, item_id, mix, pill_button, pointer_amount, spinner, text_field,
            FieldOptions, PillKind, PillSize,
        },
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JamPanelState {
    Inactive,
    Connecting,
    Hosting { listeners: usize },
    Joined,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum JamAction {
    Start,
    JoinNearby(u64),
    End,
    AcceptRequest(u64),
    RejectRequest(u64),
    RemoveRequest(u64),
    /// Add the Windows Firewall rule Jams need on this PC.
    AllowFirewall,
    /// Host: create an invite for people outside this network.
    ShareOverInternet,
    /// Join the Jam in the pasted invite.
    JoinInvite,
}

/// Host: the state of the internet invite.
pub(crate) enum InternetView<'a> {
    Off,
    Preparing,
    Ready(&'a str),
    Failed(&'a str),
}

/// A Jam found on the local network.
pub(crate) struct NearbyView {
    pub(crate) session_id: u64,
    pub(crate) host: String,
    pub(crate) track: Option<String>,
    pub(crate) listeners: u32,
}

pub(crate) enum DiscoveryView {
    Searching,
    Ready,
    Unavailable(String),
}

pub(crate) struct RequestView {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) requester: String,
    pub(crate) status: RequestStatus,
    /// Guest: whether this device made the request.
    pub(crate) mine: bool,
}

/// Everything the Jam section shows, borrowed from the application.
pub(crate) struct JamView<'a> {
    pub(crate) state: JamPanelState,
    /// The host's name while connecting to or listening with it.
    pub(crate) host_name: &'a str,
    /// Connection or synchronization status for the current session.
    pub(crate) status: &'a str,
    /// A recent outcome worth telling the user, such as how a session ended.
    pub(crate) notice: Option<&'a str>,
    /// Host: other devices cannot find this Jam automatically.
    pub(crate) undiscoverable: bool,
    /// Host: a new track is held until listeners have loaded it.
    pub(crate) waiting_for_guests: bool,
    /// The firewall change is waiting for Windows' approval.
    pub(crate) firewall_pending: bool,
    /// Host: sharing over the internet.
    pub(crate) internet: InternetView<'a>,
    pub(crate) discovery: &'a DiscoveryView,
    pub(crate) nearby: &'a [NearbyView],
    pub(crate) requests: &'a [RequestView],
}

/// `invite_input` is the invite being pasted to join over the internet.
pub(crate) fn show(
    ui: &mut Ui,
    view: &JamView<'_>,
    invite_input: &mut String,
    r: &ResponsiveUi,
) -> Vec<JamAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();
    ui.spacing_mut().item_spacing.y = r.rem(0.5);
    if let Some(notice) = view.notice {
        hint(ui, notice, theme.warning, r);
    }
    match view.state {
        JamPanelState::Inactive => {
            show_inactive(ui, view, r, &mut actions);
            join_by_invite(ui, invite_input, r, &mut actions);
        }
        JamPanelState::Connecting => show_connecting(ui, view, r, &mut actions),
        JamPanelState::Hosting { listeners } => show_hosting(ui, view, listeners, r, &mut actions),
        JamPanelState::Joined => show_joined(ui, view, r, &mut actions),
    }
    actions
}

fn heading(ui: &mut Ui, text: &str, r: &ResponsiveUi) {
    ui.label(
        egui::RichText::new(text)
            .font(fonts::display(r.rem(1.5)))
            .color(Theme::default().text),
    );
}

fn hint(ui: &mut Ui, text: &str, color: Color32, r: &ResponsiveUi) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(fonts::body(r.rem(0.8125)))
                .color(color),
        )
        .wrap(),
    );
}

fn section_label(ui: &mut Ui, text: &str, r: &ResponsiveUi) {
    ui.label(
        egui::RichText::new(text)
            .font(fonts::body(r.rem(0.875)))
            .color(Theme::default().secondary_text),
    );
}

fn show_inactive(ui: &mut Ui, view: &JamView<'_>, r: &ResponsiveUi, actions: &mut Vec<JamAction>) {
    let theme = Theme::default();
    ui.horizontal(|ui| {
        heading(ui, "Start a Jam", r);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if pill_button(ui, "Start", PillKind::Primary, PillSize::Small, r)
                .on_hover_text("Let people on your network listen along")
                .clicked()
            {
                actions.push(JamAction::Start);
            }
        });
    });
    section_label(ui, "Nearby Jams", r);
    for jam in view.nearby {
        if jam_item(ui, jam, r) {
            actions.push(JamAction::JoinNearby(jam.session_id));
        }
    }
    match view.discovery {
        DiscoveryView::Searching => {
            ui.horizontal(|ui| {
                spinner(ui, r.rem(1.0));
                hint(ui, "Looking for Jams…", theme.secondary_text, r);
            });
        }
        DiscoveryView::Ready if view.nearby.is_empty() => {
            hint(
                ui,
                "No Jams nearby yet. They appear here when someone on your network starts one.",
                theme.secondary_text,
                r,
            );
            firewall_help(
                ui,
                view,
                "Someone's Jam isn't showing up? Both PCs need to let MusiKPL through Windows Firewall.",
                r,
                actions,
            );
        }
        DiscoveryView::Ready => {}
        DiscoveryView::Unavailable(reason) => {
            hint(ui, reason, theme.warning, r);
            firewall_help(
                ui,
                view,
                "Letting MusiKPL through Windows Firewall usually fixes this.",
                r,
                actions,
            );
        }
    }
}

/// Paste an invite from a friend's MusiKPL to join over the internet.
fn join_by_invite(
    ui: &mut Ui,
    invite_input: &mut String,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    ui.add_space(r.rem(0.25));
    section_label(ui, "Join over the internet", r);
    hint(
        ui,
        "Paste the invite a friend sent you from their MusiKPL.",
        theme.secondary_text,
        r,
    );
    let field = text_field(
        ui,
        invite_input,
        ui.available_width(),
        FieldOptions {
            id: "jam_invite",
            hint: "musikpl:…",
            password: false,
            enabled: true,
        },
        r,
    );
    let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let ready = !invite_input.trim().is_empty();
    let join = pill_button(ui, "Join", PillKind::Primary, PillSize::Small, r);
    if ready && (join.clicked() || entered) {
        actions.push(JamAction::JoinInvite);
    }
}

/// Host: create and copy an invite for people outside this network.
fn internet_invite(
    ui: &mut Ui,
    view: &JamView<'_>,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    ui.add_space(r.rem(0.25));
    section_label(ui, "Invite over the internet", r);
    match view.internet {
        InternetView::Off => {
            hint(
                ui,
                "Let friends outside your network join with an invite.",
                theme.secondary_text,
                r,
            );
            if pill_button(ui, "Create invite", PillKind::Secondary, PillSize::Small, r).clicked() {
                actions.push(JamAction::ShareOverInternet);
            }
        }
        InternetView::Preparing => {
            ui.horizontal(|ui| {
                spinner(ui, r.rem(1.0));
                hint(ui, "Creating invite…", theme.secondary_text, r);
            });
        }
        InternetView::Ready(invite) => {
            hint(
                ui,
                "Send this to friends. It works until you end the Jam; anyone with it can join.",
                theme.secondary_text,
                r,
            );
            let (rect, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), r.rem(2.25)), Sense::hover());
            ui.painter().rect(
                rect,
                r.rem(0.5),
                Color32::BLACK,
                Stroke::new(1.0_f32, theme.border),
            );
            elided(
                ui.painter(),
                rect.shrink2(r.rem2(0.75, 0.0)),
                invite,
                fonts::body(r.rem(0.8125)),
                theme.secondary_text,
                Align2::LEFT_CENTER,
            );
            let copied_id = egui::Id::new("jam_invite_copied");
            let now = ui.input(|input| input.time);
            let copied_at = ui.data(|data| data.get_temp::<f64>(copied_id));
            let recently = copied_at.is_some_and(|at| now - at < 2.0);
            let label = if recently { "Copied!" } else { "Copy invite" };
            if pill_button(ui, label, PillKind::Primary, PillSize::Small, r).clicked() {
                ui.ctx().copy_text(invite.to_owned());
                ui.data_mut(|data| data.insert_temp(copied_id, now));
            }
            if recently {
                crate::ui::widgets::repaint_after(ui.ctx(), std::time::Duration::from_millis(500));
            }
        }
        InternetView::Failed(reason) => {
            hint(ui, reason, theme.warning, r);
            if pill_button(ui, "Try again", PillKind::Secondary, PillSize::Small, r).clicked() {
                actions.push(JamAction::ShareOverInternet);
            }
        }
    }
}

/// An explanation and the button that adds MusiKPL's firewall rule.
fn firewall_help(
    ui: &mut Ui,
    view: &JamView<'_>,
    text: &str,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    hint(ui, text, theme.secondary_text, r);
    if view.firewall_pending {
        ui.horizontal(|ui| {
            spinner(ui, r.rem(1.0));
            hint(
                ui,
                "Waiting for Windows' permission…",
                theme.secondary_text,
                r,
            );
        });
    } else if pill_button(
        ui,
        "Allow through firewall",
        PillKind::Secondary,
        PillSize::Small,
        r,
    )
    .on_hover_text("Adds a Windows Firewall rule for MusiKPL. Windows asks for permission.")
    .clicked()
    {
        actions.push(JamAction::AllowFirewall);
    }
}

/// One nearby Jam: avatar, host name, what's playing, and Join. Returns true
/// when Join was clicked.
fn jam_item(ui: &mut Ui, jam: &NearbyView, r: &ResponsiveUi) -> bool {
    let theme = Theme::default();
    let (rect, row) =
        ui.allocate_exact_size(vec2(ui.available_width(), r.rem(3.25)), Sense::hover());
    let hover = pointer_amount(ui, &row);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        r.rem(0.625),
        mix(
            Color32::from_rgba_unmultiplied(34, 34, 34, 128),
            Color32::from_rgba_unmultiplied(34, 34, 34, 230),
            hover,
        ),
    );
    let inner = rect.shrink(r.rem(0.5));
    let avatar = Rect::from_min_size(
        pos2(inner.left(), inner.center().y - r.rem(1.125)),
        r.rem2(2.25, 2.25),
    );
    painter.circle_filled(avatar.center(), avatar.width() / 2.0, theme.surface);
    let initial = jam
        .host
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "?".into());
    painter.text(
        avatar.center(),
        Align2::CENTER_CENTER,
        initial,
        fonts::display(r.rem(1.0)),
        theme.text,
    );

    // Join, drawn as a small white pill on the right.
    let label_font = fonts::display(r.rem(0.875));
    let label = painter.layout_no_wrap("Join".into(), label_font, Color32::BLACK);
    let join_rect = Rect::from_min_size(
        pos2(
            inner.right() - label.size().x - r.rem(1.75),
            inner.center().y - (label.size().y + r.rem(0.25)) / 2.0,
        ),
        label.size() + r.rem2(1.75, 0.25),
    );
    let join = ui.interact(
        join_rect,
        item_id("join_jam", &jam.session_id.to_string()),
        Sense::click(),
    );
    let join_hover = hover_amount(ui, &join);
    let painter = ui.painter();
    painter.rect_filled(
        join_rect,
        join_rect.height() / 2.0,
        mix(theme.text, theme.primary_hover, join_hover),
    );
    painter.galley(
        join_rect.center() - label.size() / 2.0,
        label,
        Color32::BLACK,
    );

    let text_left = avatar.right() + r.rem(0.625);
    let text_width = (join_rect.left() - r.rem(0.5) - text_left).max(0.0);
    let name_rect = Rect::from_min_size(
        pos2(text_left, inner.center().y - r.rem(1.125)),
        vec2(text_width, r.rem(1.375)),
    );
    elided(
        painter,
        name_rect,
        &jam.host,
        fonts::display(r.rem(1.0)),
        theme.text,
        Align2::LEFT_CENTER,
    );
    let listening = format!("{} listening", jam.listeners);
    let details = match &jam.track {
        Some(track) => format!("{track} · {listening}"),
        None => listening,
    };
    elided(
        painter,
        Rect::from_min_size(
            pos2(text_left, name_rect.bottom()),
            vec2(text_width, r.rem(1.0)),
        ),
        &details,
        fonts::body(r.rem(0.75)),
        theme.secondary_text,
        Align2::LEFT_CENTER,
    );
    join.on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text("Join this Jam")
        .clicked()
}

fn show_connecting(
    ui: &mut Ui,
    view: &JamView<'_>,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    ui.horizontal(|ui| {
        spinner(ui, r.rem(1.0));
        heading(
            ui,
            &if view.host_name.is_empty() {
                "Connecting…".to_owned()
            } else {
                format!("Connecting to {}…", view.host_name)
            },
            r,
        );
    });
    hint(ui, view.status, theme.secondary_text, r);
    if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Small, r).clicked() {
        actions.push(JamAction::End);
    }
}

fn show_hosting(
    ui: &mut Ui,
    view: &JamView<'_>,
    listeners: usize,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    heading(ui, "Your Jam is live", r);
    hint(
        ui,
        &format!(
            "{listeners} listener{} connected",
            if listeners == 1 { "" } else { "s" }
        ),
        theme.secondary_text,
        r,
    );
    if view.waiting_for_guests {
        hint(
            ui,
            "Starting when everyone has loaded the track…",
            theme.warning,
            r,
        );
    }
    if view.undiscoverable {
        hint(
            ui,
            "Other devices can't find this Jam automatically on this network.",
            theme.warning,
            r,
        );
    } else {
        hint(
            ui,
            "People on your network can join from Nearby Jams.",
            theme.secondary_text,
            r,
        );
    }
    if listeners == 0 {
        firewall_help(
            ui,
            view,
            "Not showing up on other PCs? Let MusiKPL through Windows Firewall on this PC.",
            r,
            actions,
        );
    }
    internet_invite(ui, view, r, actions);
    requests(ui, view, true, r, actions);
    if pill_button(ui, "End Jam", PillKind::Secondary, PillSize::Small, r).clicked() {
        actions.push(JamAction::End);
    }
}

fn show_joined(ui: &mut Ui, view: &JamView<'_>, r: &ResponsiveUi, actions: &mut Vec<JamAction>) {
    let theme = Theme::default();
    heading(
        ui,
        &if view.host_name.is_empty() {
            "You joined the Jam".to_owned()
        } else {
            format!("Listening with {}", view.host_name)
        },
        r,
    );
    hint(ui, view.status, theme.secondary_text, r);
    hint(
        ui,
        "The host controls playback. Press + on a song to add it to the queue.",
        theme.secondary_text,
        r,
    );
    requests(ui, view, false, r, actions);
    if pill_button(ui, "Leave Jam", PillKind::Secondary, PillSize::Small, r).clicked() {
        actions.push(JamAction::End);
    }
}

fn requests(
    ui: &mut Ui,
    view: &JamView<'_>,
    host: bool,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    if view.requests.is_empty() {
        return;
    }
    section_label(ui, "Requests", r);
    for request in view.requests {
        show_request(ui, request, host, r, actions);
    }
}

fn show_request(
    ui: &mut Ui,
    request: &RequestView,
    host: bool,
    r: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(34, 34, 34, 128))
        .stroke(Stroke::NONE)
        .rounding(r.rem(0.625))
        .inner_margin(egui::Margin::same(r.rem(0.5)))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&request.title)
                        .font(fonts::display(r.rem(1.0)))
                        .color(theme.text),
                )
                .truncate(),
            );
            let who = if request.mine {
                "You"
            } else {
                request.requester.as_str()
            };
            ui.add(
                egui::Label::new(
                    egui::RichText::new(format!("{who} · {}", status_label(request.status)))
                        .font(fonts::body(r.rem(0.75)))
                        .color(theme.secondary_text),
                )
                .truncate(),
            );
            // Only the host is ever offered request controls.
            if !host {
                return;
            }
            ui.horizontal(|ui| match request.status {
                RequestStatus::Pending => {
                    if pill_button(ui, "Accept", PillKind::Primary, PillSize::Small, r).clicked() {
                        actions.push(JamAction::AcceptRequest(request.id));
                    }
                    if pill_button(ui, "Reject", PillKind::Secondary, PillSize::Small, r).clicked()
                    {
                        actions.push(JamAction::RejectRequest(request.id));
                    }
                }
                RequestStatus::Accepted
                    if pill_button(ui, "Remove", PillKind::Secondary, PillSize::Small, r)
                        .clicked() =>
                {
                    actions.push(JamAction::RemoveRequest(request.id));
                }
                _ => {}
            });
        });
}

pub(crate) fn status_label(status: RequestStatus) -> &'static str {
    match status {
        RequestStatus::Pending => "Waiting for the host",
        RequestStatus::Accepted => "Accepted · in the queue",
        RequestStatus::Played => "Played",
        RequestStatus::Rejected => "Declined",
        RequestStatus::Removed => "Removed by the host",
    }
}

pub(crate) fn state(role: Option<Role>, connecting: bool, listeners: usize) -> JamPanelState {
    match role {
        Some(Role::Host) => JamPanelState::Hosting { listeners },
        Some(Role::Guest) if connecting => JamPanelState::Connecting,
        Some(Role::Guest) => JamPanelState::Joined,
        None => JamPanelState::Inactive,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::responsive::DESIGN_SIZE;

    #[test]
    fn maps_roles_to_panel_states() {
        assert_eq!(state(None, false, 0), JamPanelState::Inactive);
        assert_eq!(
            state(Some(Role::Host), false, 2),
            JamPanelState::Hosting { listeners: 2 }
        );
        assert_eq!(state(Some(Role::Guest), true, 0), JamPanelState::Connecting);
        assert_eq!(state(Some(Role::Guest), false, 0), JamPanelState::Joined);
    }

    fn run_panel(view: &JamView<'_>) -> Vec<String> {
        let ctx = egui::Context::default();
        // The design's fonts take effect from the next frame.
        crate::ui::fonts::install(&ctx);
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        let r = ResponsiveUi::from_size(DESIGN_SIZE);
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_width(r.rem(16.25));
                show(ui, view, &mut String::new(), &r);
            });
        });
        let mut texts = Vec::new();
        collect_text(
            &egui::Shape::Vec(output.shapes.into_iter().map(|c| c.shape).collect()),
            &mut texts,
        );
        texts
    }

    fn collect_text(shape: &egui::Shape, texts: &mut Vec<String>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| collect_text(shape, texts)),
            egui::Shape::Text(text) => texts.push(text.galley.text().to_owned()),
            _ => {}
        }
    }

    fn view<'a>(
        state: JamPanelState,
        discovery: &'a DiscoveryView,
        nearby: &'a [NearbyView],
        requests: &'a [RequestView],
    ) -> JamView<'a> {
        JamView {
            state,
            host_name: "Desk",
            status: "In sync",
            notice: None,
            undiscoverable: false,
            waiting_for_guests: false,
            firewall_pending: false,
            internet: InternetView::Off,
            discovery,
            nearby,
            requests,
        }
    }

    fn request(status: RequestStatus) -> RequestView {
        RequestView {
            id: 1,
            title: "Wanted song".into(),
            requester: "Ana".into(),
            status,
            mine: false,
        }
    }

    #[test]
    fn nearby_jams_and_discovery_states_are_shown() {
        let nearby = [NearbyView {
            session_id: 7,
            host: "Living room".into(),
            track: Some("Song".into()),
            listeners: 2,
        }];
        let texts = run_panel(&view(
            JamPanelState::Inactive,
            &DiscoveryView::Ready,
            &nearby,
            &[],
        ));
        assert!(texts.iter().any(|text| text == "Living room"));
        assert!(texts.iter().any(|text| text == "Song · 2 listening"));
        assert!(texts.iter().any(|text| text == "Join"));
        assert!(!texts.iter().any(|text| text.contains("code")));

        let texts = run_panel(&view(
            JamPanelState::Inactive,
            &DiscoveryView::Searching,
            &[],
            &[],
        ));
        assert!(texts.iter().any(|text| text.contains("Looking for Jams")));

        // With nothing found, the firewall fix is offered; with Jams listed,
        // it isn't in the way.
        let texts = run_panel(&view(
            JamPanelState::Inactive,
            &DiscoveryView::Ready,
            &[],
            &[],
        ));
        assert!(texts.iter().any(|text| text == "Allow through firewall"));
        let texts = run_panel(&view(
            JamPanelState::Inactive,
            &DiscoveryView::Ready,
            &nearby,
            &[],
        ));
        assert!(!texts.iter().any(|text| text == "Allow through firewall"));
        let texts = run_panel(&view(
            JamPanelState::Hosting { listeners: 0 },
            &DiscoveryView::Ready,
            &[],
            &[],
        ));
        assert!(texts.iter().any(|text| text == "Allow through firewall"));
        // Hosts can invite people outside the network...
        assert!(texts.iter().any(|text| text == "Create invite"));
        let mut ready = view(
            JamPanelState::Hosting { listeners: 1 },
            &DiscoveryView::Ready,
            &[],
            &[],
        );
        ready.internet = InternetView::Ready("musikpl:abc");
        let texts = run_panel(&ready);
        assert!(texts.iter().any(|text| text == "Copy invite"));
        // ...and anyone not in a Jam can join with one.
        let texts = run_panel(&view(
            JamPanelState::Inactive,
            &DiscoveryView::Ready,
            &[],
            &[],
        ));
        assert!(texts.iter().any(|text| text == "Join over the internet"));

        let blocked = DiscoveryView::Unavailable("Discovery is blocked.".into());
        let texts = run_panel(&view(JamPanelState::Inactive, &blocked, &[], &[]));
        assert!(texts.iter().any(|text| text == "Discovery is blocked."));
    }

    #[test]
    fn only_the_host_panel_offers_request_controls() {
        let requests = [request(RequestStatus::Pending)];
        let host = run_panel(&view(
            JamPanelState::Hosting { listeners: 1 },
            &DiscoveryView::Ready,
            &[],
            &requests,
        ));
        assert!(host.iter().any(|text| text == "Accept"));
        assert!(host.iter().any(|text| text == "Reject"));

        let guest = run_panel(&view(
            JamPanelState::Joined,
            &DiscoveryView::Ready,
            &[],
            &requests,
        ));
        assert!(guest.iter().any(|text| text == "Wanted song"));
        assert!(guest
            .iter()
            .any(|text| text.contains("Waiting for the host")));
        assert!(!guest
            .iter()
            .any(|text| text == "Accept" || text == "Reject"));
        assert!(guest.iter().any(|text| text == "Leave Jam"));
    }
}
