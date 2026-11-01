//! Crossfade controls: how long and which curve (Settings, and a
//! playlist's Transitions menu).

use eframe::egui::{self, Ui};
use musikpl_core::{audio::FadeCurve, domain::Transition, settings::MAX_CROSSFADE};

use crate::{
    theme::Theme,
    ui::{
        fonts,
        responsive::ResponsiveUi,
        widgets::{pill_button, slider, PillKind, PillSize},
    },
};

fn hint(curve: FadeCurve) -> &'static str {
    match curve {
        FadeCurve::Smooth => "Gentle at both ends, like Spotify.",
        FadeCurve::EqualPower => "Loudness stays even through the mix, DJ style.",
        FadeCurve::Linear => "Evenly from one song to the other.",
        FadeCurve::Exponential => "The next song comes in late and strong.",
    }
}

/// The length slider and the curve choices. Returns the new pair when
/// either changed.
pub(crate) fn fade_controls(
    ui: &mut Ui,
    seconds: u8,
    curve: FadeCurve,
    r: &ResponsiveUi,
) -> Option<(u8, FadeCurve)> {
    let theme = Theme::default();
    let mut changed = None;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = r.rem(0.5);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = r.rem(0.75);
            let mut value = f32::from(seconds);
            let width = r.rem(14.0).min(ui.available_width() - r.rem(5.0));
            if slider(
                ui,
                width,
                &mut value,
                1.0..=f32::from(MAX_CROSSFADE),
                true,
                true,
                r,
            )
            .changed()
            {
                let picked = value.round().clamp(1.0, f32::from(MAX_CROSSFADE)) as u8;
                if picked != seconds {
                    changed = Some((picked, curve));
                }
            }
            ui.label(
                egui::RichText::new(format!("{seconds} s"))
                    .font(fonts::display(r.rem(1.0)))
                    .color(theme.text),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = r.rem2(0.5, 0.5);
            for option in FadeCurve::ALL {
                let kind = if option == curve {
                    PillKind::Primary
                } else {
                    PillKind::Secondary
                };
                if pill_button(ui, option.label(), kind, PillSize::Small, r)
                    .on_hover_text(hint(option))
                    .clicked()
                {
                    changed = Some((seconds, option));
                }
            }
        });
        ui.label(
            egui::RichText::new(hint(curve))
                .font(fonts::body(r.rem(0.8125)))
                .color(theme.secondary_text),
        );
    });
    changed
}

/// A playlist's Transitions menu. Returns the new choice.
pub(crate) fn transition_menu(
    ui: &mut Ui,
    transition: Transition,
    r: &ResponsiveUi,
) -> Option<Transition> {
    let theme = Theme::default();
    let mut picked = None;
    ui.set_width(r.rem(22.0));
    ui.spacing_mut().item_spacing.y = r.rem(0.5);
    ui.label(
        egui::RichText::new("Transitions")
            .font(fonts::display(r.rem(1.125)))
            .color(theme.text),
    );
    ui.label(
        egui::RichText::new("How this playlist's songs fade into each other.")
            .font(fonts::body(r.rem(0.8125)))
            .color(theme.secondary_text),
    );
    let (seconds, curve) = match transition {
        Transition::Fade { seconds, curve } => (seconds, curve),
        _ => (6, FadeCurve::Smooth),
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = r.rem(0.5);
        let choices = [
            ("As in Settings", Transition::Default),
            ("No fade", Transition::Off),
            ("Its own", Transition::Fade { seconds, curve }),
        ];
        for (label, choice) in choices {
            let on = std::mem::discriminant(&choice) == std::mem::discriminant(&transition);
            let kind = if on {
                PillKind::Primary
            } else {
                PillKind::Secondary
            };
            if pill_button(ui, label, kind, PillSize::Small, r).clicked() && !on {
                picked = Some(choice);
            }
        }
    });
    if matches!(transition, Transition::Fade { .. }) {
        if let Some((seconds, curve)) = fade_controls(ui, seconds, curve, r) {
            picked = Some(Transition::Fade { seconds, curve });
        }
    }
    picked
}
