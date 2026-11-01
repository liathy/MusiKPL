pub(crate) mod bundled;
pub(crate) mod images;
pub(crate) mod install;
pub(crate) mod mpv;
pub(crate) mod process;
pub(crate) mod system;
pub(crate) mod tray;
pub(crate) mod updater;

// Shared with Android: moved to the core.
pub(crate) use musikpl_core::{cloudflare_dns, settings, storage};
