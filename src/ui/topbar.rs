//! The top bar: menu button, search field, logo, and the app's own window
//! controls. The bar itself drags the window, since it has no OS title bar.

use eframe::egui::{self, pos2, vec2, Align2, Color32, CursorIcon, Id, Rect, Sense, TextEdit, Ui};

use crate::{
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        responsive::ResponsiveUi,
        widgets::{marquee, mix, paint_icon_button, ButtonColors, HOVER_TIME},
    },
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TopBarAction {
    ToggleMenu,
    Search,
    StartDrag,
    Minimize,
    Close,
}

pub(crate) fn show(
    ui: &mut Ui,
    bar: Rect,
    search: &mut String,
    menu_open: bool,
    icons: &IconTextures,
    r: &ResponsiveUi,
) -> Vec<TopBarAction> {
    let theme = Theme::default();
    let mut actions = Vec::new();

    // Empty parts of the bar move the window. Controls added below sit on
    // top of this and take their own clicks.
    let drag = ui.interact(bar, Id::new("top_bar_drag"), Sense::click_and_drag());
    if drag.drag_started() {
        actions.push(TopBarAction::StartDrag);
    }

    let center_y = bar.center().y;
    let button = |ui: &mut Ui, id: &str, rect: Rect| ui.interact(rect, Id::new(id), Sense::click());

    // Menu (40 × 40 inside a 60 × 60 container at x = 15).
    let menu_rect = Rect::from_center_size(
        pos2(bar.left() + r.rem(0.9375 + 1.875), center_y),
        r.rem2(2.5, 2.5),
    );
    let menu = button(ui, "top_bar_menu", menu_rect);
    let mut menu_colors = ButtonColors::ghost(&theme);
    if menu_open {
        menu_colors.fill = theme.surface;
    }
    paint_icon_button(
        ui,
        menu_rect,
        &menu,
        &icons.menu,
        r.rem2(2.0, 1.5),
        r.rem(0.625),
        menu_colors,
    );
    if menu.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        actions.push(TopBarAction::ToggleMenu);
    }

    // Window controls, right to left.
    let control = r.rem2(2.25, 2.25);
    let close_rect =
        Rect::from_center_size(pos2(bar.right() - r.rem(0.9375 + 1.125), center_y), control);
    let minimize_rect = close_rect.translate(vec2(-(control.x + r.rem(0.5)), 0.0));
    let close = button(ui, "window_close", close_rect);
    paint_icon_button(
        ui,
        close_rect,
        &close,
        &icons.close,
        r.rem2(1.125, 1.125),
        control.x / 2.0,
        ButtonColors::danger(&theme),
    );
    if close.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        actions.push(TopBarAction::Close);
    }
    let minimize = button(ui, "window_minimize", minimize_rect);
    paint_icon_button(
        ui,
        minimize_rect,
        &minimize,
        &icons.minimize,
        r.rem2(1.125, 1.125),
        control.x / 2.0,
        ButtonColors::round(&theme),
    );
    if minimize.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        actions.push(TopBarAction::Minimize);
    }

    // Logo.
    let logo_font = fonts::display(r.rem(1.5));
    let logo = ui
        .painter()
        .layout_no_wrap("MusiKPL".into(), logo_font, theme.text);
    let logo_right = minimize_rect.left() - r.rem(1.25);
    let logo_pos = pos2(logo_right - logo.size().x, center_y - logo.size().y / 2.0);
    let logo_left = logo_pos.x;
    ui.painter().galley(logo_pos, logo, theme.text);

    // Search fills the space between the menu and the logo, up to 1000 px.
    let space = Rect::from_min_max(
        pos2(menu_rect.right() + r.rem(1.625), bar.top()),
        pos2(logo_left - r.rem(1.0), bar.bottom()),
    );
    let pill = Rect::from_center_size(
        pos2(space.center().x, center_y),
        vec2(space.width().min(r.rem(62.5)).max(0.0), r.rem(2.3125)),
    );
    if search_field(ui, pill, search, &theme, r) {
        actions.push(TopBarAction::Search);
    }

    actions
}

/// The search pill. Returns true when the user pressed Enter.
fn search_field(
    ui: &mut Ui,
    pill: Rect,
    search: &mut String,
    theme: &Theme,
    r: &ResponsiveUi,
) -> bool {
    let id = Id::new("search_field");
    let focused = ui.memory(|memory| memory.has_focus(id));
    let hovered = ui.rect_contains_pointer(pill);
    let t = ui
        .ctx()
        .animate_bool_with_time(id.with("hover"), hovered || focused, HOVER_TIME);
    ui.painter().rect_filled(
        pill,
        pill.height() / 2.0,
        mix(theme.surface, Color32::from_rgb(42, 42, 42), t),
    );

    let font = fonts::display(r.rem(1.5));
    let text_rect = pill.shrink2(vec2(r.rem(1.25), 0.0));
    let text_width = ui
        .painter()
        .layout_no_wrap(search.clone(), font.clone(), theme.text)
        .size()
        .x;
    // Like the design: text sits in the middle of the pill while it fits.
    // Longer text starts at the left edge and runs off to the right; when the
    // field isn't being typed in, it scrolls back and forth like the song
    // title. While typing, the field follows the cursor as usual.
    let fits = text_width <= text_rect.width();
    let scrolling = !focused && !fits;
    let response = ui.put(
        text_rect,
        TextEdit::singleline(search)
            .id(id)
            .frame(false)
            .font(font.clone())
            .text_color(if scrolling {
                Color32::TRANSPARENT
            } else {
                theme.text
            })
            .margin(egui::Margin::ZERO)
            .horizontal_align(if fits {
                egui::Align::Center
            } else {
                egui::Align::Min
            })
            .vertical_align(egui::Align::Center)
            .desired_width(text_rect.width()),
    );
    if scrolling {
        marquee(ui, text_rect, search, font.clone(), theme.text, r);
    }
    if search.is_empty() && !focused {
        ui.painter().text(
            pill.center(),
            Align2::CENTER_CENTER,
            "Search",
            font,
            theme.muted_text,
        );
    }
    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))
}
