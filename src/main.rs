#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod jam;
mod models;
mod player;
mod process;
mod theme;
mod youtube;

fn main() -> eframe::Result<()> {
    app::run()
}
