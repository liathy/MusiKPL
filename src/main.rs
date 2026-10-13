#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod domain;
mod infrastructure;
mod jam;
mod media;
mod playback;
mod recommendations;
mod theme;
mod ui;

fn main() -> eframe::Result<()> {
    app::run()
}
