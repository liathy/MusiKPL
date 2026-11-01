//! Reusable controls in the MusiKPL design. Hover and state changes animate
//! over a short time, like the Figma prototype.

use std::{ops::RangeInclusive, time::Duration};

use eframe::egui::{
    self, pos2, text::LayoutJob, vec2, Align2, Color32, CursorIcon, FontId, Id, Rect, Response,
    Sense, Stroke, TextFormat, TextureHandle, Ui, Vec2,
};

use crate::{
    domain::Cover,
    theme::Theme,
    ui::{
        fonts,
        icons::{paint_icon, IconTextures},
        responsive::ResponsiveUi,
    },
};

/// Duration of hover and state transitions, in seconds.
pub(crate) const HOVER_TIME: f32 = 0.15;

/// How far a hover/press transition has progressed for this response.
pub(crate) fn hover_amount(ui: &Ui, response: &Response) -> f32 {
    ui.ctx()
        .animate_bool_with_time(response.id.with("hover"), response.hovered(), HOVER_TIME)
}

/// Like [`hover_amount`], but stays on while the pointer is over a control
/// drawn inside this one (a row's own buttons).
pub(crate) fn pointer_amount(ui: &Ui, response: &Response) -> f32 {
    ui.ctx().animate_bool_with_time(
        response.id.with("pointer"),
        response.contains_pointer(),
        HOVER_TIME,
    )
}

/// Blend two colors; `t` = 0 gives `from`, 1 gives `to`.
pub(crate) fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(from.r(), to.r()),
        channel(from.g(), to.g()),
        channel(from.b(), to.b()),
        channel(from.a(), to.a()),
    )
}

/// Colors of an icon button at rest and when hovered.
#[derive(Clone, Copy)]
pub(crate) struct ButtonColors {
    pub(crate) fill: Color32,
    pub(crate) hover_fill: Color32,
    pub(crate) icon: Color32,
    pub(crate) hover_icon: Color32,
}

impl ButtonColors {
    /// Round grey button that lightens on hover (close, settings, …).
    pub(crate) fn round(theme: &Theme) -> Self {
        Self {
            fill: theme.surface,
            hover_fill: theme.hover,
            icon: theme.icon,
            hover_icon: theme.text,
        }
    }

    /// Bare icon that gains a grey background on hover (menu, skip, queue).
    pub(crate) fn ghost(theme: &Theme) -> Self {
        Self {
            fill: Color32::TRANSPARENT,
            hover_fill: theme.surface,
            icon: theme.icon,
            hover_icon: theme.text,
        }
    }

    /// Grey button that turns white with a black icon (play/pause).
    pub(crate) fn primary(theme: &Theme) -> Self {
        Self {
            fill: theme.surface,
            hover_fill: theme.text,
            icon: theme.icon,
            hover_icon: Color32::BLACK,
        }
    }

    /// Window close: red on hover.
    pub(crate) fn danger(theme: &Theme) -> Self {
        Self {
            fill: theme.surface,
            hover_fill: theme.danger,
            icon: theme.icon,
            hover_icon: theme.text,
        }
    }
}

/// An icon button of exactly `size`, with `rounding` corners.
pub(crate) fn icon_button(
    ui: &mut Ui,
    icon: &TextureHandle,
    size: Vec2,
    icon_size: Vec2,
    rounding: f32,
    colors: ButtonColors,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    paint_icon_button(ui, rect, &response, icon, icon_size, rounding, colors);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

pub(crate) fn paint_icon_button(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    icon: &TextureHandle,
    icon_size: Vec2,
    rounding: f32,
    colors: ButtonColors,
) {
    let t = hover_amount(ui, response);
    let painter = ui.painter();
    painter.rect_filled(rect, rounding, mix(colors.fill, colors.hover_fill, t));
    paint_icon(
        painter,
        icon,
        Rect::from_center_size(rect.center(), icon_size),
        mix(colors.icon, colors.hover_icon, t),
    );
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillKind {
    /// White with black text (Join, Save, Connect).
    Primary,
    /// Grey with white text (Cancel, End Jam, Disconnect).
    Secondary,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PillSize {
    Small,
    Medium,
}

/// A rounded text button.
pub(crate) fn pill_button(
    ui: &mut Ui,
    label: &str,
    kind: PillKind,
    size: PillSize,
    r: &ResponsiveUi,
) -> Response {
    let theme = Theme::default();
    let (font, padding) = match size {
        PillSize::Small => (fonts::display(r.rem(0.875)), r.rem2(0.875, 0.25)),
        PillSize::Medium => (fonts::display(r.rem(1.0)), r.rem2(1.5, 0.5)),
    };
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let (rect, response) = ui.allocate_exact_size(galley.size() + padding * 2.0, Sense::click());
    let t = hover_amount(ui, &response);
    let (fill, text) = match kind {
        PillKind::Primary => (mix(theme.text, theme.primary_hover, t), Color32::BLACK),
        PillKind::Secondary => (mix(theme.surface, theme.hover, t), theme.text),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, rect.height() / 2.0, fill);
    painter.galley(rect.center() - galley.size() / 2.0, galley, text);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// The on/off switch from the settings screen.
pub(crate) fn toggle(ui: &mut Ui, on: bool, r: &ResponsiveUi) -> Response {
    let theme = Theme::default();
    let (rect, response) = ui.allocate_exact_size(r.rem2(2.75, 1.5), Sense::click());
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("on"), on, HOVER_TIME);
    let hover = hover_amount(ui, &response);
    let painter = ui.painter();
    let track = mix(mix(theme.surface, theme.hover, hover), theme.text, t);
    painter.rect_filled(rect, rect.height() / 2.0, track);
    let knob = r.rem(0.5625);
    let inset = r.rem(0.1875) + knob;
    let x = egui::lerp((rect.left() + inset)..=(rect.right() - inset), t);
    painter.circle_filled(
        pos2(x, rect.center().y),
        knob,
        mix(theme.secondary_text, theme.panel, t),
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A slider with the design's rail, white progress, and ringed thumb.
///
/// While dragging, the shown value follows the pointer; `changed()` is
/// reported when the value should be applied: on every move when `live`,
/// otherwise once the drag ends or on a click.
pub(crate) fn slider(
    ui: &mut Ui,
    width: f32,
    value: &mut f32,
    range: RangeInclusive<f32>,
    enabled: bool,
    live: bool,
    r: &ResponsiveUi,
) -> Response {
    let theme = Theme::default();
    let thumb = r.rem(0.4375);
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, r.rem(1.0)), sense);
    let rail = rect.shrink2(vec2(thumb, 0.0));
    let span = (range.end() - range.start()).max(f32::EPSILON);
    let drag_id = response.id.with("drag");

    let pointer_value = response.interact_pointer_pos().map(|pointer| {
        let t = ((pointer.x - rail.left()) / rail.width().max(1.0)).clamp(0.0, 1.0);
        range.start() + t * span
    });
    if enabled && response.dragged() {
        if let Some(target) = pointer_value {
            ui.data_mut(|data| data.insert_temp(drag_id, target));
            if live && (target - *value).abs() > f32::EPSILON {
                *value = target;
                response.mark_changed();
            }
        }
    }
    let dragging = ui.data(|data| data.get_temp::<f32>(drag_id));
    if enabled && (response.drag_stopped() || response.clicked()) {
        if let Some(target) = pointer_value.or(dragging) {
            *value = target;
            response.mark_changed();
        }
        ui.data_mut(|data| data.remove::<f32>(drag_id));
    }
    let shown = if response.dragged() {
        dragging.unwrap_or(*value)
    } else {
        *value
    };

    let t = ((shown - range.start()) / span).clamp(0.0, 1.0);
    let hover = hover_amount(ui, &response);
    let painter = ui.painter();
    let rail_height = r.rem(0.375);
    let rail_rect = Rect::from_center_size(rail.center(), vec2(rect.width(), rail_height));
    painter.rect_filled(rail_rect, rail_height / 2.0, theme.surface);
    let x = egui::lerp(rail.left()..=rail.right(), t);
    let filled = Rect::from_min_max(
        rail_rect.min,
        pos2(x.max(rail_rect.left() + rail_height), rail_rect.bottom()),
    );
    painter.rect_filled(filled, rail_height / 2.0, theme.text);
    if enabled {
        let radius = thumb * (1.0 + 0.15 * hover);
        painter.circle(
            pos2(x, rect.center().y),
            radius,
            theme.text,
            Stroke::new(r.rem(0.125), theme.ring),
        );
    }
    if enabled {
        response = response.on_hover_cursor(CursorIcon::PointingHand);
    }
    response
}

/// The Auto Mix style checkbox row: icon and a large label, all clickable.
pub(crate) fn checkbox_row(
    ui: &mut Ui,
    icons: &IconTextures,
    checked: bool,
    label: &str,
    r: &ResponsiveUi,
) -> Response {
    let theme = Theme::default();
    let font = fonts::display(r.rem(1.5));
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, theme.text);
    let box_size = r.rem(1.25);
    let gap = r.rem(0.5);
    let size = vec2(
        box_size + gap + galley.size().x,
        galley.size().y.max(box_size),
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hover = hover_amount(ui, &response);
    let icon_rect = Rect::from_min_size(
        pos2(rect.left(), rect.center().y - box_size / 2.0),
        Vec2::splat(box_size),
    );
    let icon = if checked {
        &icons.check_box
    } else {
        &icons.check_box_off
    };
    let painter = ui.painter();
    paint_icon(
        painter,
        icon,
        icon_rect.shrink(r.rem(0.0625)),
        mix(theme.icon, theme.text, hover),
    );
    painter.galley(
        pos2(
            icon_rect.right() + gap,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        theme.text,
    );
    response.on_hover_cursor(CursorIcon::PointingHand)
}

/// A labeled single-line text box in the design's input style.
pub(crate) struct FieldOptions<'a> {
    /// Stable name for the field, so it keeps focus between frames.
    pub(crate) id: &'a str,
    pub(crate) hint: &'a str,
    pub(crate) password: bool,
    pub(crate) enabled: bool,
}

pub(crate) fn text_field(
    ui: &mut Ui,
    text: &mut String,
    width: f32,
    options: FieldOptions<'_>,
    r: &ResponsiveUi,
) -> Response {
    let theme = Theme::default();
    let (rect, _) = ui.allocate_exact_size(vec2(width, r.rem(2.75)), Sense::hover());
    let id = Id::new(("text_field", options.id));
    let focused = ui.memory(|memory| memory.has_focus(id));
    let t = ui
        .ctx()
        .animate_bool_with_time(id.with("focus"), focused, HOVER_TIME);
    ui.painter().rect(
        rect,
        r.rem(0.625),
        Color32::BLACK,
        Stroke::new(1.0_f32, mix(theme.border, theme.ring, t)),
    );
    let edit = egui::TextEdit::singleline(text)
        .id(id)
        .frame(false)
        .password(options.password)
        .interactive(options.enabled)
        .font(fonts::body(r.rem(0.9375)))
        .text_color(if options.enabled {
            theme.text
        } else {
            theme.secondary_text
        })
        .hint_text(
            egui::RichText::new(options.hint)
                .font(fonts::body(r.rem(0.9375)))
                .color(theme.muted_text),
        )
        .margin(egui::Margin::symmetric(r.rem(0.875), 0.0))
        .vertical_align(egui::Align::Center);
    ui.put(rect, edit)
}

/// Text that may not fit: painted on one line with an ellipsis.
pub(crate) fn elided(
    painter: &egui::Painter,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align: Align2,
) {
    crate::ui::fonts::cover(painter.ctx(), text);
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: font,
            color,
            ..TextFormat::default()
        },
    );
    job.wrap.max_width = rect.width().max(0.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    let pos = align.align_size_within_rect(galley.size(), rect).min;
    painter
        .with_clip_rect(rect.expand(1.0))
        .galley(pos, galley, color);
}

/// Small spaced capitals ("APP SETTINGS").
pub(crate) fn caps_label(ui: &mut Ui, text: &str, r: &ResponsiveUi) {
    let theme = Theme::default();
    let job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat {
            font_id: fonts::body(r.rem(0.75)),
            color: theme.secondary_text,
            extra_letter_spacing: r.rem(0.06),
            ..TextFormat::default()
        },
    );
    ui.label(job);
}

/// Frame interval for scrolling text: 30 fps is smooth at its slow speed.
const MARQUEE_FRAME: Duration = Duration::from_millis(33);
/// Frame interval for spinners: 20 fps.
const SPINNER_FRAME: Duration = Duration::from_millis(50);

/// Ask for the next frame in `delay`.
///
/// egui shortens every requested delay by its predicted frame time (1/60 s)
/// so it never draws late; for 30 fps that would mean 60 fps. Adding it back
/// keeps low frame rates low.
pub(crate) fn repaint_after(ctx: &egui::Context, delay: Duration) {
    let predicted = ctx.input(|input| input.predicted_dt);
    let predicted = Duration::try_from_secs_f32(predicted).unwrap_or_default();
    ctx.request_repaint_after(delay + predicted);
}

/// Where a scrolling line of text is, `time` seconds into its cycle.
///
/// The text waits at the start, slides left until its end is visible,
/// waits, and slides back, like the title in the Figma prototype.
/// Returns the offset and how long it stays there: zero while moving, the
/// rest of the pause while waiting, so no frames are drawn during pauses.
pub(crate) fn marquee_offset(time: f64, distance: f32, speed: f32) -> (f32, f64) {
    const PAUSE: f64 = 1.5;
    if distance <= 0.0 || speed <= 0.0 {
        return (0.0, f64::INFINITY);
    }
    let travel = f64::from(distance / speed);
    let cycle = 2.0 * (PAUSE + travel);
    let phase = time.rem_euclid(cycle);
    let eased = |t: f64| {
        let t = t.clamp(0.0, 1.0);
        (t * t * (3.0 - 2.0 * t)) as f32
    };
    if phase < PAUSE {
        (0.0, PAUSE - phase)
    } else if phase < PAUSE + travel {
        (distance * eased((phase - PAUSE) / travel), 0.0)
    } else if phase < 2.0 * PAUSE + travel {
        (distance, 2.0 * PAUSE + travel - phase)
    } else {
        (
            distance * (1.0 - eased((phase - 2.0 * PAUSE - travel) / travel)),
            0.0,
        )
    }
}

/// A small spinning arc. Unlike egui's spinner, which redraws on every
/// frame, it asks for a few frames per second only.
pub(crate) fn spinner(ui: &mut Ui, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    if ui.is_rect_visible(rect) {
        repaint_after(ui.ctx(), SPINNER_FRAME);
        let time = ui.input(|input| input.time);
        let start = (time * std::f64::consts::TAU * 0.75) as f32;
        let radius = size * 0.4;
        let points: Vec<_> = (0..=16)
            .map(|step| {
                let angle = start + step as f32 / 16.0 * std::f32::consts::PI * 1.5;
                rect.center() + radius * Vec2::angled(angle)
            })
            .collect();
        ui.painter().add(egui::Shape::line(
            points,
            Stroke::new(size / 8.0, Theme::default().secondary_text),
        ));
    }
    response
}

/// One line of text in `rect` that scrolls back and forth when it is too
/// long to fit.
pub(crate) fn marquee(
    ui: &Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    r: &ResponsiveUi,
) {
    crate::ui::fonts::cover(ui.ctx(), text);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let overflow = galley.size().x - rect.width();
    let distance = if overflow > 0.5 {
        overflow + r.rem(0.75)
    } else {
        0.0
    };
    let time = ui.input(|input| input.time);
    let (offset, still_for) = marquee_offset(time, distance, r.rem(2.5));
    if distance > 0.0 {
        let next = if still_for > 0.0 {
            Duration::from_secs_f64(still_for)
        } else {
            MARQUEE_FRAME
        };
        repaint_after(ui.ctx(), next);
    }
    let pos = pos2(
        rect.left() - offset,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter()
        .with_clip_rect(rect.intersect(ui.clip_rect()))
        .galley(pos, galley, color);
}

/// A rounded square cover: grey with a music note until the image loads.
pub(crate) fn cover(
    ui: &Ui,
    rect: Rect,
    cover: Option<&Cover>,
    icons: &IconTextures,
    rounding: f32,
) {
    let theme = Theme::default();
    let painter = ui.painter();
    painter.rect_filled(rect, rounding, theme.surface);
    paint_icon(
        painter,
        &icons.music,
        Rect::from_center_size(rect.center(), rect.size() * 0.3),
        Color32::from_white_alpha(80),
    );
    if let Some(cover) = cover {
        cover_image(ui, rect, cover, rounding);
    }
}

/// How long a picture that failed to load waits before it is tried again.
const IMAGE_RETRY_SECS: f64 = 20.0;

/// Paint a cover image once it has loaded, over whatever placeholder is
/// already drawn. A picture that fails to load (a network hiccup, say)
/// leaves the placeholder instead of an error sign, and is tried again a
/// little later. Returns whether the image was painted.
pub(crate) fn cover_image(ui: &Ui, rect: Rect, cover: &Cover, rounding: f32) -> bool {
    use egui::load::{SizeHint, TexturePoll};
    let ctx = ui.ctx();
    match ctx.try_load_texture(&cover.url, crate::ui::icons::SMOOTH, SizeHint::default()) {
        Ok(TexturePoll::Ready { texture }) => {
            let [left, top, right, bottom] = cover.uv;
            egui::Image::from_texture(texture)
                .uv(Rect::from_min_max(pos2(left, top), pos2(right, bottom)))
                .rounding(rounding)
                .paint_at(ui, rect);
            true
        }
        Ok(TexturePoll::Pending { .. }) => false,
        Err(_) => {
            let failures = Id::new("image_failures");
            let now = ctx.input(|input| input.time);
            let failed_at = ctx.data_mut(|data| {
                *data
                    .get_temp_mut_or_default::<std::collections::HashMap<String, f64>>(failures)
                    .entry(cover.url.clone())
                    .or_insert(now)
            });
            if now - failed_at > IMAGE_RETRY_SECS {
                ctx.forget_image(&cover.url);
                ctx.data_mut(|data| {
                    data.get_temp_mut_or_default::<std::collections::HashMap<String, f64>>(
                        failures,
                    )
                    .remove(&cover.url);
                });
            }
            false
        }
    }
}

/// An `Id` that is stable for one item in a list.
pub(crate) fn item_id(salt: &str, key: &str) -> Id {
    Id::new((salt, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marquee_waits_slides_waits_and_returns() {
        // 80 px at 40 px/s: 1.5 s wait, 2 s slide, 1.5 s wait, 2 s back.
        // While waiting, the remaining pause says when to draw next.
        assert_eq!(marquee_offset(0.5, 80.0, 40.0), (0.0, 1.0));
        let (middle, still) = marquee_offset(2.5, 80.0, 40.0);
        assert!(still == 0.0 && (middle - 40.0).abs() < 0.01);
        assert_eq!(marquee_offset(4.0, 80.0, 40.0), (80.0, 1.0));
        let (back, still) = marquee_offset(6.0, 80.0, 40.0);
        assert!(still == 0.0 && (back - 40.0).abs() < 0.01);
        // The cycle repeats.
        assert_eq!(marquee_offset(7.0 + 0.5, 80.0, 40.0), (0.0, 1.0));
    }

    #[test]
    fn text_that_fits_never_moves_or_redraws() {
        assert_eq!(marquee_offset(3.0, 0.0, 40.0), (0.0, f64::INFINITY));
        assert_eq!(marquee_offset(3.0, -5.0, 40.0), (0.0, f64::INFINITY));
    }

    #[test]
    fn colors_blend_between_states() {
        let from = Color32::from_rgb(34, 34, 34);
        assert_eq!(mix(from, Color32::WHITE, 0.0), from);
        assert_eq!(mix(from, Color32::WHITE, 1.0), Color32::WHITE);
        assert_eq!(
            mix(from, Color32::WHITE, 0.5),
            Color32::from_rgb(145, 145, 145)
        );
    }
}
