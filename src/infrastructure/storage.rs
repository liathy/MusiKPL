//! MusiKPL's local data: small JSON files in `%APPDATA%\MusiKPL`.
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

/// The folder holding settings, taste, the session, and the Home cache.
pub(crate) fn data_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        Some(std::env::temp_dir().join(format!("musikpl-test-data-{}", std::process::id())))
    }
    #[cfg(not(test))]
    {
        let base = std::env::var_os("APPDATA").or_else(|| std::env::var_os("HOME"))?;
        Some(PathBuf::from(base).join("MusiKPL"))
    }
}

/// Read `name` from the data folder; `None` when missing or unreadable.
pub(crate) fn read<T: DeserializeOwned>(name: &str) -> Option<T> {
    let text = std::fs::read_to_string(data_dir()?.join(name)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write `value` as `name` in the data folder, replacing it atomically.
pub(crate) fn write<T: Serialize>(name: &str, value: &T) -> Result<(), String> {
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
pub(crate) fn write_in_background<T: Serialize>(name: &'static str, value: &T) {
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
pub(crate) fn delete(name: &str) {
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
