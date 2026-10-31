//! `MusiKPL.exe --uninstall`, run from Windows' Installed apps: a small
//! window that removes MusiKPL and, only if asked, the user's data.

use eframe::egui::{self, vec2, Color32};

use crate::{
    infrastructure::install,
    theme::Theme,
    ui::{
        fonts,
        icons::IconTextures,
        responsive::{ResponsiveUi, DESIGN_SIZE},
        widgets::{checkbox_row, pill_button, PillKind, PillSize},
    },
};

#[derive(PartialEq)]
enum Step {
    Ask,
    Done,
    Failed(String),
}

struct Uninstaller {
    icons: Option<IconTextures>,
    delete_data: bool,
    step: Step,
}

pub(crate) fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Uninstall MusiKPL")
            .with_inner_size(vec2(520.0, 330.0))
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    eframe::run_native(
        "Uninstall MusiKPL",
        options,
        Box::new(|cc| {
            fonts::install(&cc.egui_ctx);
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(Uninstaller {
                icons: IconTextures::load(&cc.egui_ctx).ok(),
                delete_data: false,
                step: Step::Ask,
            }))
        }),
    )
}

/// Remove shortcuts, the Installed apps entry and start-up entry, the
/// program folder (once this process exits) and, if chosen, the data.
fn uninstall(delete_data: bool) -> Result<(), String> {
    install::remove_shortcuts();
    install::unregister();
    if delete_data {
        for dir in install::data_dirs() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    let dir = install::install_dir().ok_or("Couldn't find where MusiKPL is installed")?;
    if dir.exists() {
        install::remove_dir_after_exit(&dir)?;
    }
    Ok(())
}

impl eframe::App for Uninstaller {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let theme = Theme::default();
        let r = ResponsiveUi::from_size(DESIGN_SIZE);
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme.panel)
                    .inner_margin(egui::Margin::same(r.rem(1.75))),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = r.rem(0.75);
                let text = |ui: &mut egui::Ui, text: &str, size: f32, color: Color32| {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .font(fonts::body(r.rem(size)))
                                .color(color),
                        )
                        .wrap(),
                    );
                };
                match &self.step {
                    Step::Ask => {
                        ui.label(
                            egui::RichText::new("Uninstall MusiKPL?")
                                .font(fonts::display(r.rem(1.75)))
                                .color(theme.text),
                        );
                        text(
                            ui,
                            "This removes MusiKPL, its shortcuts, and its entry in Installed apps. Close MusiKPL first if it is open.",
                            0.9375,
                            theme.secondary_text,
                        );
                        if let Some(icons) = &self.icons {
                            if checkbox_row(ui, icons, self.delete_data, "Also delete my data", &r)
                                .clicked()
                            {
                                self.delete_data = !self.delete_data;
                            }
                        }
                        text(
                            ui,
                            "Your data is your listening history, likes, Home, settings and Last.fm connection. Keep it if you plan to install MusiKPL again.",
                            0.8125,
                            theme.muted_text,
                        );
                        ui.add_space(r.rem(0.5));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = r.rem(0.75);
                            if pill_button(ui, "Uninstall", PillKind::Primary, PillSize::Medium, &r)
                                .clicked()
                            {
                                self.step = match uninstall(self.delete_data) {
                                    Ok(()) => Step::Done,
                                    Err(error) => Step::Failed(error),
                                };
                            }
                            if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Medium, &r)
                                .clicked()
                            {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                    Step::Done => {
                        ui.label(
                            egui::RichText::new("MusiKPL was uninstalled")
                                .font(fonts::display(r.rem(1.75)))
                                .color(theme.text),
                        );
                        let detail = if self.delete_data {
                            "Your data was deleted too."
                        } else {
                            "Your data was kept, so installing MusiKPL again picks up where you left off."
                        };
                        text(ui, detail, 0.9375, theme.secondary_text);
                        if pill_button(ui, "Close", PillKind::Primary, PillSize::Medium, &r)
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    Step::Failed(error) => {
                        ui.label(
                            egui::RichText::new("Couldn't uninstall MusiKPL")
                                .font(fonts::display(r.rem(1.75)))
                                .color(theme.text),
                        );
                        text(ui, error, 0.9375, theme.danger);
                        if pill_button(ui, "Close", PillKind::Secondary, PillSize::Medium, &r)
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            });
    }
}
