//! Songs saved for offline listening, in the data folder's `downloads`
//! folder, with an index (downloads.json) of what each file is.
//!
//! Streams are fetched in pieces (like yt-dlp does), which YouTube serves at
//! full speed, into a `.part` file renamed when complete.

use std::{
    fs::File,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{domain::Track, storage};

pub const INDEX_FILE: &str = "downloads.json";
/// Bytes asked for per request.
const PIECE: u64 = 10 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Downloaded {
    pub track: Track,
    /// The file's name in the downloads folder.
    pub file: String,
    pub bytes: u64,
    /// Unix seconds.
    pub at: u64,
    /// Downloaded by itself (not only as part of a playlist).
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DownloadIndex {
    pub songs: Vec<Downloaded>,
    /// Playlists (by their permanent id) kept downloaded: songs added to
    /// them are downloaded too.
    pub playlists: Vec<String>,
}

impl DownloadIndex {
    pub fn get(&self, id: &str) -> Option<&Downloaded> {
        self.songs.iter().find(|song| song.track.id == id)
    }

    pub fn total_bytes(&self) -> u64 {
        self.songs.iter().map(|song| song.bytes).sum()
    }
}

pub fn dir() -> Option<PathBuf> {
    storage::data_dir().map(|dir| dir.join("downloads"))
}

/// A song's file name: its id (YouTube ids are file-safe), checked.
pub fn file_name(id: &str) -> Option<String> {
    let safe = !id.is_empty()
        && id.len() < 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    safe.then(|| format!("{id}.audio"))
}

/// Where a downloaded song's file is, if it's there.
pub fn path_of(song: &Downloaded) -> Option<PathBuf> {
    if file_name(&song.track.id).as_deref() != Some(song.file.as_str()) {
        return None;
    }
    let path = dir()?.join(&song.file);
    path.is_file().then_some(path)
}

pub fn delete(file: &str) {
    if file.contains(['/', '\\']) || file.contains("..") {
        return;
    }
    if let Some(dir) = dir() {
        let _ = std::fs::remove_file(dir.join(file));
    }
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(30))
        .build()
}

/// "bytes 0-99/1234" → 1234.
fn total_from(content_range: &str) -> Option<u64> {
    content_range.rsplit('/').next()?.trim().parse().ok()
}

/// Save `url` to `dest`. `progress` goes from 0 to 1000; `cancel` stops it.
/// Returns the size.
pub fn fetch(
    url: &str,
    dest: &Path,
    progress: &AtomicU32,
    cancel: &AtomicBool,
) -> Result<u64, String> {
    let part = dest.with_extension("part");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Couldn't save the song ({e})"))?;
    }
    let mut file = File::create(&part).map_err(|e| format!("Couldn't save the song ({e})"))?;
    let agent = agent();
    let mut done: u64 = 0;
    let mut total: Option<u64> = None;
    let mut buffer = vec![0u8; 64 * 1024];
    let result = (|| {
        loop {
            let end = done + PIECE - 1;
            let response = agent
                .get(url)
                .set("Range", &format!("bytes={done}-{end}"))
                .call()
                .map_err(|e| format!("The download stopped ({e})"))?;
            let ranged = response.status() == 206;
            if ranged {
                total = total.or_else(|| response.header("Content-Range").and_then(total_from));
            } else {
                // The whole file at once.
                total = response
                    .header("Content-Length")
                    .and_then(|n| n.parse().ok());
                if done > 0 {
                    return Err("The download couldn't continue".to_owned());
                }
            }
            let mut reader = response.into_reader();
            let mut got = 0u64;
            loop {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Cancelled".to_owned());
                }
                let read = reader
                    .read(&mut buffer)
                    .map_err(|e| format!("The download stopped ({e})"))?;
                if read == 0 {
                    break;
                }
                file.write_all(&buffer[..read])
                    .map_err(|e| format!("Couldn't save the song ({e})"))?;
                got += read as u64;
                done += read as u64;
                if let Some(total) = total.filter(|total| *total > 0) {
                    progress.store((done * 1000 / total).min(1000) as u32, Ordering::Relaxed);
                }
            }
            let finished = !ranged || got == 0 || total.is_some_and(|total| done >= total);
            if finished {
                return Ok(done);
            }
        }
    })();
    drop(file);
    match result {
        Ok(size) if size > 0 => {
            std::fs::rename(&part, dest).map_err(|e| format!("Couldn't save the song ({e})"))?;
            Ok(size)
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&part);
            Err("The song came back empty".into())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&part);
            Err(error)
        }
    }
}

/// Whether the internet answers (a quick connection to Cloudflare or
/// Google's public servers).
pub fn internet_reachable() -> bool {
    let servers: [SocketAddr; 2] = [([1, 1, 1, 1], 443).into(), ([8, 8, 8, 8], 443).into()];
    servers
        .iter()
        .any(|server| TcpStream::connect_timeout(server, Duration::from_secs(3)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_ids_become_files() {
        assert_eq!(
            file_name("dQw4w9WgXcQ").as_deref(),
            Some("dQw4w9WgXcQ.audio")
        );
        assert_eq!(file_name("../x"), None);
        assert_eq!(file_name(""), None);
        assert_eq!(total_from("bytes 0-99/1234"), Some(1234));
    }
}
