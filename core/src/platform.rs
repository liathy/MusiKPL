//! What the core asks of the platform it runs on, registered once at start.
//!
//! Each app (desktop, Android) installs its [`Platform`] with [`install`]
//! before using the core. Until then, and in tests, a quiet default is used.

use std::sync::{Arc, OnceLock};

/// Asks the interface to draw again (egui's repaint, Android's state update).
/// Background work calls it when something it shows has changed.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// A waker that does nothing, for tests and work nothing watches.
pub fn quiet_waker() -> Waker {
    Arc::new(|| {})
}

pub trait Platform: Send + Sync {
    /// Open a web page in the browser.
    fn open_url(&self, url: &str) -> Result<(), String>;
}

struct NoPlatform;

impl Platform for NoPlatform {
    fn open_url(&self, _url: &str) -> Result<(), String> {
        Err("Opening links isn't available here".into())
    }
}

static PLATFORM: OnceLock<Box<dyn Platform>> = OnceLock::new();

/// Register the platform. Only the first call counts.
pub fn install(platform: Box<dyn Platform>) {
    let _ = PLATFORM.set(platform);
}

fn current() -> &'static dyn Platform {
    PLATFORM
        .get()
        .map_or(&NoPlatform, |platform| platform.as_ref())
}

/// Open a web page in the browser.
pub fn open_url(url: &str) -> Result<(), String> {
    current().open_url(url)
}

static DEVICE_NAME: OnceLock<String> = OnceLock::new();

/// The device's name as its owner knows it ("Galaxy A56"), for platforms
/// without a computer name. Only the first call counts.
pub fn set_device_name(name: &str) {
    let name = name.trim();
    if !name.is_empty() {
        let _ = DEVICE_NAME.set(name.to_owned());
    }
}

/// This device's name: the one the platform gave, else the computer's.
pub fn device_name() -> Option<String> {
    DEVICE_NAME.get().cloned().or_else(|| {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .ok()
    })
}
