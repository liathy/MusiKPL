#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod infrastructure;
mod media;
mod theme;
mod ui;
mod uninstall;

// The shared core (also used by the Android app), under the names the
// desktop has always used.
use musikpl_core::{domain, friends, jam, lastfm, playback, recommendations};

use infrastructure::{install::UNINSTALL_ARG, updater};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some(UNINSTALL_ARG) => return uninstall::run(),
        // A portable copy that just replaced itself: let the old one close.
        Some(updater::AFTER_UPDATE_ARG) => {
            if let Some(pid) = args.get(1).and_then(|pid| pid.parse().ok()) {
                updater::wait_for_exit(pid);
            }
        }
        _ => {}
    }
    updater::clean_up_previous_version();
    app::run()
}
