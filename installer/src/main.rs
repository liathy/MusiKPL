//! MusiKPL-Setup: installs MusiKPL for the current user (no administrator
//! rights) into `%LOCALAPPDATA%\Programs\MusiKPL`, with optional Start menu
//! and Desktop shortcuts and an Installed apps entry.
//!
//! `--update --wait-pid <pid>` is how MusiKPL updates itself: it waits for
//! MusiKPL to close, replaces it, and opens it again, without a window.
//! The user's data lives elsewhere (`%APPDATA%\MusiKPL`) and is never touched.

#![windows_subsystem = "windows"]

#[path = "../../src/infrastructure/install.rs"]
mod install;

use std::{
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CursorIcon, FontData, FontDefinitions, FontFamily, FontId,
    Rect, Sense, Stroke, Ui,
};

/// The MusiKPL.exe this setup installs.
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/MusiKPL.exe"));
const VERSION: &str = env!("MUSIKPL_VERSION");

const BACKGROUND: Color32 = Color32::from_rgb(16, 16, 16);
const SURFACE: Color32 = Color32::from_rgb(34, 34, 34);
const HOVER: Color32 = Color32::from_rgb(51, 51, 51);
const SECONDARY: Color32 = Color32::from_rgba_premultiplied(153, 153, 153, 153);
const MUTED: Color32 = Color32::from_rgba_premultiplied(102, 102, 102, 102);
const DANGER: Color32 = Color32::from_rgb(229, 72, 77);

#[derive(Clone, Copy)]
struct Choices {
    start_menu: bool,
    desktop: bool,
    open_after: bool,
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut error = None;
    if args.first().map(String::as_str) == Some("--update") {
        let pid = args
            .iter()
            .position(|arg| arg == "--wait-pid")
            .and_then(|index| args.get(index + 1))
            .and_then(|pid| pid.parse().ok());
        if let Some(pid) = pid {
            wait_for_exit(pid);
        }
        // Shortcuts already point at the installed exe; only replace it.
        match install_files().and_then(|exe| launch(&exe)) {
            Ok(()) => return Ok(()),
            Err(problem) => error = Some(problem),
        }
    }
    window(error)
}

/// Copy MusiKPL.exe into place and register it. Returns the installed exe.
fn install_files() -> Result<PathBuf, String> {
    if PAYLOAD.is_empty() {
        return Err("This setup was built without MusiKPL inside it.".into());
    }
    let dir = install::install_dir().ok_or("Couldn't find your AppData folder")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Couldn't create {} ({e})", dir.display()))?;
    let exe = dir.join(install::EXE_NAME);
    let new = dir.join("MusiKPL.exe.new");
    std::fs::write(&new, PAYLOAD).map_err(|e| format!("Couldn't write MusiKPL.exe ({e})"))?;
    // An open MusiKPL locks its exe: close it (it saves everything first).
    install::close_running(&exe);
    // MusiKPL may take a moment to let go of its exe after closing.
    let mut attempt = 0;
    loop {
        match std::fs::rename(&new, &exe) {
            Ok(()) => break,
            Err(_) if attempt < 40 => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(250));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&new);
                return Err(format!(
                    "Couldn't replace MusiKPL.exe. Close MusiKPL and try again ({e})"
                ));
            }
        }
    }
    let size_kb = (PAYLOAD.len() / 1024) as u32;
    install::register(&dir, VERSION, size_kb)?;
    install::retarget_start_with_windows(&exe);
    Ok(exe)
}

fn install(choices: Choices) -> Result<PathBuf, String> {
    let exe = install_files()?;
    if choices.start_menu {
        install::create_shortcut(install::Shortcut::StartMenu, &exe)?;
    }
    if choices.desktop {
        install::create_shortcut(install::Shortcut::Desktop, &exe)?;
    }
    Ok(exe)
}

fn launch(exe: &std::path::Path) -> Result<(), String> {
    Command::new(exe)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("MusiKPL was installed but couldn't be opened ({e})"))
}

/// Wait (at most 20 s) for process `pid` to exit.
fn wait_for_exit(pid: u32) {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    // SAFETY: plain handle use; a missing process yields a null handle.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle != 0 {
            WaitForSingleObject(handle, 20_000);
            CloseHandle(handle);
        }
    }
}

// ── Window ──────────────────────────────────────────────────────────────

enum Step {
    Ready,
    Installing(Receiver<Result<PathBuf, String>>),
    Done(PathBuf),
    Failed(String),
}

struct Setup {
    choices: Choices,
    step: Step,
    updating: bool,
}

fn window(error: Option<String>) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MusiKPL Setup")
            .with_inner_size(vec2(560.0, 400.0))
            .with_resizable(false)
            .with_maximize_button(false),
        ..Default::default()
    };
    eframe::run_native(
        "MusiKPL Setup",
        options,
        Box::new(move |cc| {
            install_fonts(&cc.egui_ctx);
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BACKGROUND;
            cc.egui_ctx.set_visuals(visuals);
            let updating = install::installed_exe().is_some_and(|exe| exe.exists());
            Ok(Box::new(Setup {
                choices: Choices {
                    start_menu: true,
                    desktop: !updating,
                    open_after: true,
                },
                step: match error {
                    Some(error) => Step::Failed(error),
                    None => Step::Ready,
                },
                updating,
            }))
        }),
    )
}

impl eframe::App for Setup {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Step::Installing(receiver) = &self.step {
            match receiver.try_recv() {
                Ok(Ok(exe)) => {
                    if self.choices.open_after && launch(&exe).is_ok() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    self.step = Step::Done(exe);
                }
                Ok(Err(error)) => self.step = Step::Failed(error),
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100))
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.step = Step::Failed("Setup stopped unexpectedly".into())
                }
            }
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::same(28.0)),
            )
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                ui.label(
                    egui::RichText::new("MusiKPL")
                        .font(display(30.0))
                        .color(Color32::WHITE),
                );
                match &self.step {
                    Step::Ready => self.ready(ui),
                    Step::Installing(_) => {
                        heading(ui, "Installing…");
                        note(ui, "Copying MusiKPL and creating shortcuts.", SECONDARY);
                        ui.add(egui::Spinner::new().size(22.0).color(Color32::WHITE));
                    }
                    Step::Done(exe) => {
                        let exe = exe.clone();
                        heading(ui, "MusiKPL is installed");
                        note(
                            ui,
                            "Find it in the Start menu. To remove it, use Settings › Apps › Installed apps.",
                            SECONDARY,
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if pill(ui, "Open MusiKPL", true).clicked() && launch(&exe).is_ok() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                            if pill(ui, "Close", false).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                    Step::Failed(error) => {
                        heading(ui, "Setup couldn't finish");
                        note(ui, error, DANGER);
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if pill(ui, "Try again", true).clicked() {
                                self.step = Step::Ready;
                            }
                            if pill(ui, "Close", false).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        });
                    }
                }
            });
    }
}

impl Setup {
    fn ready(&mut self, ui: &mut Ui) {
        if self.updating {
            heading(ui, &format!("Update MusiKPL to {VERSION}"));
            note(
                ui,
                "MusiKPL is already installed. If it's open, it closes to update. Your music history, likes, playlists and settings are kept.",
                SECONDARY,
            );
        } else {
            heading(ui, &format!("Install MusiKPL {VERSION}"));
            note(
                ui,
                "Installs for you only, no administrator rights needed. Your data stays on this PC.",
                SECONDARY,
            );
        }
        if let Some(dir) = install::install_dir() {
            note(ui, &dir.display().to_string(), MUTED);
        }
        ui.add_space(6.0);
        checkbox(ui, &mut self.choices.start_menu, "Start menu shortcut");
        checkbox(ui, &mut self.choices.desktop, "Desktop shortcut");
        checkbox(ui, &mut self.choices.open_after, "Open MusiKPL when done");
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let label = if self.updating { "Update" } else { "Install" };
            if pill(ui, label, true).clicked() {
                let choices = self.choices;
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || {
                    let _ = sender.send(install(choices));
                });
                self.step = Step::Installing(receiver);
            }
            if pill(ui, "Cancel", false).clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }
}

// ── Look ────────────────────────────────────────────────────────────────

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "display".into(),
        FontData::from_static(include_bytes!(
            "../../src/assets/fonts/BalooThambi-Regular.ttf"
        )),
    );
    fonts.font_data.insert(
        "body".into(),
        FontData::from_static(include_bytes!(
            "../../src/assets/fonts/BalooThambi2-Regular.ttf"
        )),
    );
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut body = vec!["body".to_owned()];
    body.extend(fallbacks.iter().cloned());
    let mut display = vec!["display".to_owned()];
    display.extend(fallbacks);
    fonts.families.insert(FontFamily::Proportional, body);
    fonts
        .families
        .insert(FontFamily::Name("display".into()), display);
    ctx.set_fonts(fonts);
}

fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(display(22.0))
            .color(Color32::WHITE),
    );
}

fn note(ui: &mut Ui, text: &str, color: Color32) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(FontId::proportional(15.0))
                .color(color),
        )
        .wrap(),
    );
}

/// The app's pill buttons: white for the main action, grey otherwise.
fn pill(ui: &mut Ui, label: &str, primary: bool) -> egui::Response {
    let font = display(16.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE);
    let size = galley.size() + vec2(48.0, 16.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let (fill, hover_fill, text) = if primary {
        (
            Color32::WHITE,
            Color32::from_rgb(214, 214, 214),
            Color32::BLACK,
        )
    } else {
        (SURFACE, HOVER, Color32::WHITE)
    };
    let fill = if response.hovered() { hover_fill } else { fill };
    ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, label, font, text);
    response.on_hover_cursor(CursorIcon::PointingHand)
}

fn checkbox(ui: &mut Ui, value: &mut bool, label: &str) {
    let height = 30.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let box_rect = Rect::from_min_size(pos2(rect.left(), rect.center().y - 10.0), vec2(20.0, 20.0));
    let painter = ui.painter();
    if *value {
        painter.rect_filled(box_rect, 5.0, Color32::WHITE);
        let check = [
            pos2(box_rect.left() + 5.0, box_rect.center().y),
            pos2(box_rect.left() + 8.5, box_rect.bottom() - 5.5),
            pos2(box_rect.right() - 4.5, box_rect.top() + 5.5),
        ];
        painter.line_segment([check[0], check[1]], Stroke::new(2.2_f32, Color32::BLACK));
        painter.line_segment([check[1], check[2]], Stroke::new(2.2_f32, Color32::BLACK));
    } else {
        let stroke = if response.hovered() {
            Color32::WHITE
        } else {
            SECONDARY
        };
        painter.rect_stroke(box_rect, 5.0, Stroke::new(1.5_f32, stroke));
    }
    painter.text(
        pos2(box_rect.right() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(16.0),
        Color32::WHITE,
    );
    if response.on_hover_cursor(CursorIcon::PointingHand).clicked() {
        *value = !*value;
    }
}
