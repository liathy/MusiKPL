//! Playlist screen buttons, and choosing a cover photo: the file dialog,
//! then a crop step where the user moves and zooms the photo inside the
//! square it will fill.

use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, TryRecvError},
};

use eframe::egui::{self, pos2, vec2, Color32, Rect, Sense, Stroke};
use musikpl_core::{
    covers::{self, Crop},
    engine::AppError,
};

use crate::{
    infrastructure::images,
    theme::Theme,
    ui::{
        fonts,
        playlist::PlaylistAction,
        responsive::ResponsiveUi,
        widgets::{pill_button, PillKind, PillSize},
    },
};

use super::YtMusicApp;

/// A photo picked for a playlist, read and ready to crop: the playlist,
/// the file, and a preview no larger than 1024 pixels.
type Picked = (u64, PathBuf, egui::ColorImage, [u32; 2]);

/// A cover photo on its way: chosen in the dialog, or being saved.
pub(super) enum CoverPick {
    Choosing(Receiver<Option<Result<Picked, String>>>),
    Saving(u64, Receiver<Result<String, String>>),
}

/// The crop step: the photo, the square's center (in photo pixels), and
/// the zoom (1: the square spans the photo's shorter side).
pub(super) struct CoverCrop {
    id: u64,
    path: PathBuf,
    texture: egui::TextureHandle,
    /// The full photo's size in pixels.
    size: [u32; 2],
    center: egui::Vec2,
    zoom: f32,
}

impl CoverCrop {
    fn shorter(&self) -> f32 {
        self.size[0].min(self.size[1]) as f32
    }

    /// The square's side, in photo pixels.
    fn side(&self) -> f32 {
        self.shorter() / self.zoom
    }

    /// Keep the square inside the photo.
    fn clamp(&mut self) {
        self.zoom = self.zoom.clamp(1.0, 8.0);
        let half = self.side() / 2.0;
        let (w, h) = (self.size[0] as f32, self.size[1] as f32);
        self.center.x = self.center.x.clamp(half, w - half);
        self.center.y = self.center.y.clamp(half, h - half);
    }

    fn crop(&self) -> Crop {
        let side = self.side();
        Crop {
            x: (self.center.x - side / 2.0) / self.size[0] as f32,
            y: (self.center.y - side / 2.0) / self.size[1] as f32,
            side: 1.0 / self.zoom,
        }
    }
}

/// Read a photo for the crop step (on a background thread).
fn read_photo(id: u64, path: PathBuf) -> Result<Picked, String> {
    let image = image::ImageReader::open(&path)
        .and_then(|reader| reader.with_guessed_format())
        .map_err(|e| format!("Couldn't open that image ({e})"))?
        .decode()
        .map_err(|e| format!("Couldn't read that image ({e})"))?;
    let size = [image.width(), image.height()];
    if size[0] == 0 || size[1] == 0 {
        return Err("That image is empty".into());
    }
    let preview = image.thumbnail(1024, 1024).to_rgba8();
    let color = egui::ColorImage::from_rgba_unmultiplied(
        [preview.width() as usize, preview.height() as usize],
        preview.as_raw(),
    );
    Ok((id, path, color, size))
}

impl YtMusicApp {
    /// Open the file dialog (on its own thread, since it waits for the user);
    /// the chosen photo then goes to the crop step.
    fn pick_playlist_cover(&mut self, id: u64) {
        if self.cover_pick.is_some() || self.cover_crop.is_some() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let picked = images::pick_image().map(|path| read_photo(id, path));
            let _ = sender.send(picked);
        });
        self.cover_pick = Some(CoverPick::Choosing(receiver));
    }

    pub fn poll_cover_pick(&mut self, ctx: &egui::Context) {
        match &self.cover_pick {
            Some(CoverPick::Choosing(receiver)) => {
                let picked = match receiver.try_recv() {
                    Ok(picked) => picked,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => None,
                };
                self.cover_pick = None;
                match picked {
                    None => {}
                    Some(Ok((id, path, preview, size))) => {
                        let texture =
                            ctx.load_texture("cover_crop", preview, egui::TextureOptions::LINEAR);
                        let mut crop = CoverCrop {
                            id,
                            path,
                            texture,
                            size,
                            center: vec2(size[0] as f32 / 2.0, size[1] as f32 / 2.0),
                            zoom: 1.0,
                        };
                        crop.clamp();
                        self.cover_crop = Some(crop);
                    }
                    Some(Err(error)) => self.engine.error = Some(AppError::Ui(error)),
                }
            }
            Some(CoverPick::Saving(id, receiver)) => {
                let id = *id;
                let saved = match receiver.try_recv() {
                    Ok(saved) => saved,
                    Err(TryRecvError::Empty) => return,
                    Err(TryRecvError::Disconnected) => Err("The photo wasn't saved".into()),
                };
                self.cover_pick = None;
                match saved {
                    Ok(name) => self.engine.set_playlist_cover(id, name),
                    Err(error) => self.engine.error = Some(AppError::Ui(error)),
                }
            }
            None => {}
        }
    }

    /// The crop window, over everything: drag to move the photo, scroll to
    /// zoom, then Save or Cancel.
    pub(super) fn render_cover_crop(
        &mut self,
        ctx: &egui::Context,
        window: Rect,
        r: &ResponsiveUi,
    ) {
        let Some(crop) = &mut self.cover_crop else {
            return;
        };
        let theme = Theme::default();
        let mut done = None;
        egui::Area::new(egui::Id::new("cover_crop"))
            .order(egui::Order::Foreground)
            .fixed_pos(window.min)
            .show(ctx, |ui| {
                // Dim and block what's behind.
                let (_, _) = ui.allocate_exact_size(window.size(), Sense::click_and_drag());
                ui.painter()
                    .rect_filled(window, 0.0, Color32::from_black_alpha(200));
                let side = r
                    .rem(26.0)
                    .min(window.height() - r.rem(12.0))
                    .max(r.rem(10.0));
                let frame = Rect::from_center_size(
                    window.center() - vec2(0.0, r.rem(2.0)),
                    vec2(side, side),
                );
                ui.painter().text(
                    pos2(frame.center().x, frame.top() - r.rem(1.5)),
                    egui::Align2::CENTER_BOTTOM,
                    "Move and zoom the photo to fit the square",
                    fonts::display(r.rem(1.25)),
                    theme.text,
                );
                // The photo, scaled so the crop square fills the frame.
                let scale = side / crop.side();
                let preview = crop.texture.size_vec2();
                let full = vec2(crop.size[0] as f32, crop.size[1] as f32);
                let photo = Rect::from_min_size(frame.center() - crop.center * scale, full * scale);
                let input = ui.interact(frame, egui::Id::new("cover_crop_drag"), Sense::drag());
                let _ = preview;
                ui.painter().with_clip_rect(frame).image(
                    crop.texture.id(),
                    photo,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                ui.painter()
                    .rect_stroke(frame, 0.0, Stroke::new(2.0_f32, Color32::WHITE));
                if input.dragged() {
                    crop.center -= input.drag_delta() / scale;
                }
                if input.hovered() {
                    let scroll = ui.input(|input| input.smooth_scroll_delta.y);
                    if scroll != 0.0 {
                        crop.zoom *= (scroll / 200.0).exp();
                    }
                }
                crop.clamp();
                let buttons = Rect::from_center_size(
                    pos2(frame.center().x, frame.bottom() + r.rem(2.5)),
                    vec2(r.rem(16.0), r.rem(2.5)),
                );
                ui.allocate_new_ui(
                    egui::UiBuilder::new()
                        .max_rect(buttons)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = r.rem(0.75);
                        if pill_button(ui, "Save", PillKind::Primary, PillSize::Medium, r).clicked()
                        {
                            done = Some(true);
                        }
                        if pill_button(ui, "Cancel", PillKind::Secondary, PillSize::Medium, r)
                            .clicked()
                        {
                            done = Some(false);
                        }
                    },
                );
                ui.painter().text(
                    pos2(frame.center().x, buttons.bottom() + r.rem(1.0)),
                    egui::Align2::CENTER_TOP,
                    "Scroll to zoom",
                    fonts::body(r.rem(0.875)),
                    theme.secondary_text,
                );
            });
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            done = Some(false);
        }
        match done {
            Some(true) => {
                let crop = self.cover_crop.take().expect("the crop step is open");
                let (sender, receiver) = mpsc::channel();
                let (id, path, cut) = (crop.id, crop.path.clone(), crop.crop());
                std::thread::spawn(move || {
                    let _ = sender.send(covers::save_cover_cropped(
                        &path,
                        &format!("playlist-{id}"),
                        Some(cut),
                    ));
                });
                self.cover_pick = Some(CoverPick::Saving(id, receiver));
            }
            Some(false) => self.cover_crop = None,
            None => {}
        }
    }

    pub fn apply_playlist_action(&mut self, action: PlaylistAction) {
        let Some(id) = self.open_playlist_id() else {
            return;
        };
        let changed = match action {
            PlaylistAction::PlayAll => {
                self.engine.play_playlist(id);
                false
            }
            PlaylistAction::PlayFrom(index) => {
                self.play_playlist_from(id, index);
                false
            }
            PlaylistAction::ToggleFavorite => self.library.playlists.toggle_favorite(id),
            PlaylistAction::ToggleDownload => {
                self.engine.toggle_playlist_download(id);
                false
            }
            PlaylistAction::SetTransition(transition) => {
                self.engine.set_playlist_transition(id, transition);
                false
            }
            PlaylistAction::Edit => {
                self.start_editing_playlist();
                false
            }
            PlaylistAction::Done => {
                self.engine.finish_editing_playlist();
                false
            }
            PlaylistAction::Remove(index) => self.library.playlists.remove(id, index),
            PlaylistAction::Move { from, to } => self.library.playlists.reorder(id, from, to),
            PlaylistAction::ToggleLike(track) => {
                self.toggle_like(&track);
                false
            }
            PlaylistAction::AddSuggestion(track) => self.library.playlists.add(id, track),
            PlaylistAction::PlaySuggestion(track) => {
                if self.is_guest() {
                    self.request_from_host(&track);
                } else {
                    self.play_search_track(track);
                }
                false
            }
            PlaylistAction::MoreSuggestions => {
                self.more_playlist_recs();
                false
            }
            PlaylistAction::ChangeCover => {
                self.pick_playlist_cover(id);
                false
            }
            PlaylistAction::RemoveCover => {
                self.engine.remove_playlist_cover(id);
                false
            }
        };
        if changed {
            self.library.playlists_changed = true;
        }
    }
}
