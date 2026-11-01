//! MusiKPL's local data: small JSON files in `%APPDATA%\MusiKPL` on Windows,
//! `~/.local/share/MusiKPL` on Linux, and the app's own folder on Android
//! (passed in with [`set_data_dir`]).
//!
//! Files are written beside their target and swapped in, so a crash never
//! leaves a half-written file. Tests use a private temporary folder, so they
//! never touch the user's real data.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        LazyLock, Mutex,
    },
};

use serde::{de::DeserializeOwned, Serialize};

/// A data folder the platform chose (Android's app storage).
static GIVEN_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Keep data in `folder` (Android). Only the first call counts.
pub fn set_data_dir(folder: PathBuf) {
    let _ = GIVEN_DIR.set(folder);
}

/// The folder holding settings, taste, the session, and the Home cache.
pub fn data_dir() -> Option<PathBuf> {
    // Tests (the core's own, and the apps' through the `testing`
    // feature) never touch the user's real data.
    #[cfg(any(test, feature = "testing"))]
    {
        Some(std::env::temp_dir().join(format!("musikpl-test-data-{}", std::process::id())))
    }
    #[cfg(not(any(test, feature = "testing")))]
    if let Some(folder) = GIVEN_DIR.get() {
        return Some(folder.clone());
    }
    #[cfg(all(not(any(test, feature = "testing")), windows))]
    {
        let base = std::env::var_os("APPDATA")?;
        Some(PathBuf::from(base).join("MusiKPL"))
    }
    // Linux and others: the XDG data folder, normally ~/.local/share.
    #[cfg(all(not(any(test, feature = "testing")), not(windows)))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".local").join("share"))
            })?;
        Some(base.join("MusiKPL"))
    }
}

/// Read `name` from the data folder; `None` when missing or unreadable.
pub fn read<T: DeserializeOwned>(name: &str) -> Option<T> {
    let text = std::fs::read_to_string(data_dir()?.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write `value` as `name` in the data folder, replacing it atomically.
pub fn write<T: Serialize>(name: &str, value: &T) -> Result<(), String> {
    let text = serde_json::to_string(value).map_err(|e| format!("Could not encode {name}: {e}"))?;
    let _writing = lock(&WRITING);
    // Background saves of this file queued earlier are now out of date.
    lock(&LATEST).insert(name.to_owned(), next_write());
    write_text(name, &text)
}

/// Like [`write`], but the file is written on a background thread so the
/// interface never waits on the disk. Encoding happens here, so the value
/// written is the one passed in; if a newer save of the same file comes
/// along before this one runs, this one is dropped.
pub fn write_in_background<T: Serialize>(name: &'static str, value: &T) {
    let Ok(text) = serde_json::to_string(value) else {
        return;
    };
    let write = next_write();
    lock(&LATEST).insert(name.to_owned(), write);
    std::thread::spawn(move || {
        let _writing = lock(&WRITING);
        if lock(&LATEST).get(name) == Some(&write) {
            let _ = write_text(name, &text);
        }
    });
}

/// Held while a file is written, so saves happen one at a time.
static WRITING: Mutex<()> = Mutex::new(());
/// The newest save asked for, per file.
static LATEST: LazyLock<Mutex<HashMap<String, u32>>> = LazyLock::new(Default::default);

fn next_write() -> u32 {
    static WRITES: AtomicU32 = AtomicU32::new(0);
    WRITES.fetch_add(1, Ordering::Relaxed)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_text(name: &str, text: &str) -> Result<(), String> {
    let folder = data_dir().ok_or("Could not find the AppData folder")?;
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("Could not create the MusiKPL data folder: {e}"))?;
    let path = folder.join(name);
    let temporary = folder.join(format!("{name}.tmp"));
    std::fs::write(&temporary, text).map_err(|e| format!("Could not save {name}: {e}"))?;
    std::fs::rename(&temporary, &path).map_err(|e| format!("Could not save {name}: {e}"))
}

/// Remove everything MusiKPL stored about the user's listening.
pub fn delete(name: &str) {
    if let Some(folder) = data_dir() {
        let _ = std::fs::remove_file(folder.join(name));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_a_private_test_folder() {
        let folder = data_dir().unwrap();
        assert!(folder.starts_with(std::env::temp_dir()));
        write("storage-test.json", &vec![1, 2, 3]).unwrap();
        assert_eq!(read::<Vec<i32>>("storage-test.json"), Some(vec![1, 2, 3]));
        delete("storage-test.json");
        assert_eq!(read::<Vec<i32>>("storage-test.json"), None);
    }
}
