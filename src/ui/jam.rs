use crate::{
    jam::{RequestStatus, Role},
    theme::Theme,
    ui::responsive::ResponsiveUi,
};
use eframe::egui;

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
    ToggleCodeJoin,
    Join,
    End,
    AcceptRequest(u64),
    RejectRequest(u64),
    RemoveRequest(u64),
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

/// Everything the Jam panel shows, borrowed from the application.
pub(crate) struct JamView<'a> {
    pub(crate) state: JamPanelState,
    pub(crate) host_code: &'a str,
    /// The host's name while connecting to or listening with it.
    pub(crate) host_name: &'a str,
    /// Connection or synchronization status for the current session.
    pub(crate) status: &'a str,
    /// A recent outcome worth telling the user, such as how a session ended.
    pub(crate) notice: Option<&'a str>,
    /// Host: `ip:port` to share when nearby discovery is unavailable.
    pub(crate) manual_address: Option<&'a str>,
    /// Host: a new track is held until listeners have loaded it.
    pub(crate) waiting_for_guests: bool,
    pub(crate) discovery: &'a DiscoveryView,
    pub(crate) nearby: &'a [NearbyView],
    pub(crate) requests: &'a [RequestView],
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    view: &JamView<'_>,
    address: &mut String,
    code: &mut String,
    show_code_join: bool,
    responsive: &ResponsiveUi,
) -> Vec<JamAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    ui.label(
        egui::RichText::new("LAN JAM")
            .size(responsive.font_caption)
            .color(theme.secondary_text),
    );
    if let Some(notice) = view.notice {
        ui.small(egui::RichText::new(notice).color(theme.accent));
    }
    match view.state {
        JamPanelState::Inactive => show_inactive(
            ui,
            view,
            address,
            code,
            show_code_join,
            responsive,
            &mut actions,
        ),
        JamPanelState::Connecting => show_connecting(ui, view, responsive, &mut actions),
        JamPanelState::Hosting { listeners } => {
            show_hosting(ui, view, listeners, responsive, &mut actions)
        }
        JamPanelState::Joined => show_joined(ui, view, responsive, &mut actions),
    }
    actions
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.small(egui::RichText::new(text).color(Theme::default().secondary_text));
}

fn show_inactive(
    ui: &mut egui::Ui,
    view: &JamView<'_>,
    address: &mut String,
    code: &mut String,
    show_code_join: bool,
    responsive: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    if ui.button("Start a Jam").clicked() {
        actions.push(JamAction::Start);
    }

    ui.add_space(responsive.space_xs);
    ui.label(egui::RichText::new("Nearby Jams").size(responsive.font_body));
    for jam in view.nearby {
        // One click joins; the address and code come from discovery.
        if ui
            .add(egui::Button::new(&jam.host).truncate())
            .on_hover_text("Join this Jam")
            .clicked()
        {
            actions.push(JamAction::JoinNearby(jam.session_id));
        }
        let listeners = match jam.listeners {
            0 => "No listeners yet".to_owned(),
            1 => "1 listener".to_owned(),
            count => format!("{count} listeners"),
        };
        let details = match &jam.track {
            Some(track) => format!("{track} · {listeners}"),
            None => listeners,
        };
        ui.add(
            egui::Label::new(
                egui::RichText::new(details)
                    .small()
                    .color(theme.secondary_text),
            )
            .truncate(),
        );
    }
    match view.discovery {
        DiscoveryView::Searching => {
            ui.horizontal(|ui| {
                ui.spinner();
                hint(ui, "Looking for Jams…");
            });
        }
        DiscoveryView::Ready if view.nearby.is_empty() => {
            hint(
                ui,
                "No Jams nearby yet. They appear here automatically when someone on your network starts one.",
            );
        }
        DiscoveryView::Ready => {}
        DiscoveryView::Unavailable(reason) => {
            ui.small(egui::RichText::new(reason).color(theme.accent));
            hint(ui, "You can still join with the host's code below.");
        }
    }

    if ui.button("Join with code").clicked() {
        actions.push(JamAction::ToggleCodeJoin);
    }
    if show_code_join {
        ui.add(
            egui::TextEdit::singleline(address)
                .id_salt("jam_join_address")
                .hint_text("Host address"),
        );
        ui.add(
            egui::TextEdit::singleline(code)
                .id_salt("jam_join_code")
                .hint_text("Six-digit code"),
        );
        if ui.button("Join").clicked() {
            actions.push(JamAction::Join);
        }
    }
}

fn show_connecting(
    ui: &mut egui::Ui,
    view: &JamView<'_>,
    responsive: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    ui.add_space(responsive.space_xs);
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(if view.host_name.is_empty() {
            "Connecting…".to_owned()
        } else {
            format!("Connecting to {}…", view.host_name)
        });
    });
    hint(ui, view.status);
    if ui.button("Cancel").clicked() {
        actions.push(JamAction::End);
    }
}

fn show_hosting(
    ui: &mut egui::Ui,
    view: &JamView<'_>,
    listeners: usize,
    responsive: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    ui.add_space(responsive.space_xs);
    ui.label(egui::RichText::new("Your Jam is live").size(responsive.font_body));
    ui.small(format!(
        "{listeners} listener{} connected",
        if listeners == 1 { "" } else { "s" }
    ));
    if view.waiting_for_guests {
        ui.small(
            egui::RichText::new("Starting when everyone has loaded the track…").color(theme.accent),
        );
    }
    match view.manual_address {
        None => hint(ui, "People on your network can join from Nearby Jams."),
        Some(address) => {
            ui.small(
                egui::RichText::new("This Jam can't be found automatically on this network.")
                    .color(theme.accent),
            );
            hint(ui, &format!("Guests can join with code at {address}"));
        }
    }
    ui.small(format!("Code: {}", display_code(view.host_code)));

    if !view.requests.is_empty() {
        ui.add_space(responsive.space_xs);
        ui.label(egui::RichText::new("Requests").size(responsive.font_body));
        for request in view.requests {
            show_request(ui, request, true, actions);
        }
    }
    ui.add_space(responsive.space_xs);
    if ui.button("End Jam").clicked() {
        actions.push(JamAction::End);
    }
}

fn show_joined(
    ui: &mut egui::Ui,
    view: &JamView<'_>,
    responsive: &ResponsiveUi,
    actions: &mut Vec<JamAction>,
) {
    ui.add_space(responsive.space_xs);
    ui.label(
        egui::RichText::new(if view.host_name.is_empty() {
            "You joined the Jam".to_owned()
        } else {
            format!("Listening with {}", view.host_name)
        })
        .size(responsive.font_body),
    );
    hint(ui, view.status);
    hint(
        ui,
        "The host controls playback. Search and press Request to suggest a song.",
    );

    if !view.requests.is_empty() {
        ui.add_space(responsive.space_xs);
        ui.label(egui::RichText::new("Requests").size(responsive.font_body));
        for request in view.requests {
            show_request(ui, request, false, actions);
        }
    }
    ui.add_space(responsive.space_xs);
    if ui.button("Leave Jam").clicked() {
        actions.push(JamAction::End);
    }
}

fn show_request(
    ui: &mut egui::Ui,
    request: &RequestView,
    host: bool,
    actions: &mut Vec<JamAction>,
) {
    let theme = Theme::default();
    ui.add(egui::Label::new(&request.title).truncate());
    let who = if request.mine {
        "You"
    } else {
        request.requester.as_str()
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(format!("{who} · {}", status_label(request.status)))
                .small()
                .color(theme.secondary_text),
        )
        .truncate(),
    );
    // Only the host is ever offered request controls.
    if !host {
        return;
    }
    ui.horizontal_wrapped(|ui| match request.status {
        RequestStatus::Pending => {
            if ui.small_button("Accept").clicked() {
                actions.push(JamAction::AcceptRequest(request.id));
            }
            if ui.small_button("Reject").clicked() {
                actions.push(JamAction::RejectRequest(request.id));
            }
        }
        RequestStatus::Accepted if ui.small_button("Remove").clicked() => {
            actions.push(JamAction::RemoveRequest(request.id));
        }
        _ => {}
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

fn display_code(code: &str) -> String {
    if code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("{} {}", &code[..3], &code[3..])
    } else {
        code.into()
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

    #[test]
    fn groups_six_digit_codes_for_sharing() {
        assert_eq!(display_code("483921"), "483 921");
        assert_eq!(display_code("not-a-code"), "not-a-code");
    }

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
        let responsive = ResponsiveUi::from_size(egui::vec2(1_000.0, 700.0));
        let (mut address, mut code) = (String::new(), String::new());
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show(ui, view, &mut address, &mut code, false, &responsive);
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
            host_code: "483921",
            host_name: "Desk",
            status: "In sync",
            notice: None,
            manual_address: None,
            waiting_for_guests: false,
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
        assert!(texts.iter().any(|text| text == "Song · 2 listeners"));

        let texts = run_panel(&view(
            JamPanelState::Inactive,
            &DiscoveryView::Searching,
            &[],
            &[],
        ));
        assert!(texts.iter().any(|text| text.contains("Looking for Jams")));

        let blocked = DiscoveryView::Unavailable("Discovery is blocked.".into());
        let texts = run_panel(&view(JamPanelState::Inactive, &blocked, &[], &[]));
        assert!(texts.iter().any(|text| text == "Discovery is blocked."));
        assert!(texts.iter().any(|text| text.contains("host's code")));
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
