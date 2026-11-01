//! Downloads in the app: songs and whole playlists saved to play offline,
//! one download at a time, and knowing when the internet is gone (then
//! Home shows the downloads and Auto Mix shuffles them).

use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{
    domain::Track,
    downloads::{self, DownloadIndex, Downloaded, INDEX_FILE},
    storage,
};

use super::{unix_now, Engine};

/// How often the internet is checked.
const ONLINE_CHECK: Duration = Duration::from_secs(20);
/// How often downloaded playlists are checked for new songs.
const PLAYLIST_CHECK: Duration = Duration::from_secs(5);
/// A failed download waits this long before the next try.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// Where a song stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DownloadMark {
    None,
    Queued,
    /// Per mille done.
    Downloading(u32),
    Done,
}

struct Active {
    track: Track,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<Result<u64, String>>,
}

pub struct DownloadsState {
    pub index: DownloadIndex,
    /// The downloaded songs, newest first (for Home).
    pub songs: Vec<Track>,
    queue: VecDeque<Track>,
    active: Option<Active>,
    online: Arc<AtomicBool>,
    checker: bool,
    playlists_checked: Option<Instant>,
    retry_at: Option<Instant>,
    /// Songs added by playlists, not by the user.
    from_playlist: std::collections::HashSet<String>,
    pub notice: Option<(String, Instant)>,
}

impl Default for DownloadsState {
    fn default() -> Self {
        Self {
            index: DownloadIndex::default(),
            songs: Vec::new(),
            queue: VecDeque::new(),
            active: None,
            // Online until a check says otherwise.
            online: Arc::new(AtomicBool::new(true)),
            checker: false,
            playlists_checked: None,
            retry_at: None,
            from_playlist: std::collections::HashSet::new(),
            notice: None,
        }
    }
}

impl DownloadsState {
    pub fn is_downloaded(&self, id: &str) -> bool {
        self.index.get(id).is_some()
    }

    pub fn mark(&self, id: &str) -> DownloadMark {
        if self.is_downloaded(id) {
            return DownloadMark::Done;
        }
        if let Some(active) = self.active.as_ref().filter(|a| a.track.id == id) {
            return DownloadMark::Downloading(active.progress.load(Ordering::Relaxed));
        }
        if self.queue.iter().any(|track| track.id == id) {
            return DownloadMark::Queued;
        }
        DownloadMark::None
    }

    /// Every song downloaded, downloading or waiting, by id.
    pub fn marks(&self) -> std::collections::HashMap<String, DownloadMark> {
        let mut marks: std::collections::HashMap<String, DownloadMark> = self
            .index
            .songs
            .iter()
            .map(|song| (song.track.id.clone(), DownloadMark::Done))
            .collect();
        for track in &self.queue {
            marks.insert(track.id.clone(), DownloadMark::Queued);
        }
        if let Some((track, done)) = self.current() {
            marks.insert(track.id.clone(), DownloadMark::Downloading(done));
        }
        marks
    }

    /// Songs waiting or downloading.
    pub fn pending(&self) -> usize {
        self.queue.len() + usize::from(self.active.is_some())
    }

    /// The song downloading and how far (per mille).
    pub fn current(&self) -> Option<(&Track, u32)> {
        self.active
            .as_ref()
            .map(|active| (&active.track, active.progress.load(Ordering::Relaxed)))
    }

    pub fn tracks(&self) -> Vec<Track> {
        let mut songs: Vec<&Downloaded> = self.index.songs.iter().collect();
        songs.sort_by_key(|song| std::cmp::Reverse(song.at));
        songs.into_iter().map(|song| song.track.clone()).collect()
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|(_, at)| at.elapsed() < Duration::from_secs(6))
            .map(|(text, _)| text.as_str())
    }
}

impl Engine {
    pub fn load_downloads(&mut self) {
        let mut index: DownloadIndex = storage::read(INDEX_FILE).unwrap_or_default();
        // Files deleted by hand are forgotten.
        index
            .songs
            .retain(|song| downloads::path_of(song).is_some());
        self.downloads.index = index;
        self.downloads.songs = self.downloads.tracks();
    }

    fn save_downloads(&mut self) {
        storage::write_in_background(INDEX_FILE, &self.downloads.index);
        self.downloads.songs = self.downloads.tracks();
    }

    /// No internet right now.
    pub fn offline(&self) -> bool {
        !self.downloads.online.load(Ordering::Relaxed)
    }

    /// A downloaded song's file, to play instead of streaming.
    pub fn local_file(&self, id: &str) -> Option<String> {
        let song = self.downloads.index.get(id)?;
        downloads::path_of(song).map(|path| path.to_string_lossy().into_owned())
    }

    fn set_download_notice(&mut self, text: impl Into<String>) {
        self.downloads.notice = Some((text.into(), Instant::now()));
    }

    /// Download a song (the user asked).
    pub fn download(&mut self, track: &Track) {
        if let Some(song) = self
            .downloads
            .index
            .songs
            .iter_mut()
            .find(|song| song.track.id == track.id)
        {
            song.pinned = true;
            self.save_downloads();
            return;
        }
        self.downloads.from_playlist.remove(&track.id);
        self.queue_download(track.clone());
    }

    fn queue_download(&mut self, track: Track) {
        if self.downloads.mark(&track.id) == DownloadMark::None {
            self.downloads.queue.push_back(track);
        }
    }

    /// Remove a downloaded song (or stop its download).
    pub fn remove_download(&mut self, id: &str) {
        self.downloads.queue.retain(|track| track.id != id);
        if let Some(active) = self.downloads.active.as_ref().filter(|a| a.track.id == id) {
            active.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(index) = self
            .downloads
            .index
            .songs
            .iter()
            .position(|s| s.track.id == id)
        {
            let song = self.downloads.index.songs.remove(index);
            downloads::delete(&song.file);
            self.save_downloads();
        }
    }

    pub fn toggle_download(&mut self, track: &Track) {
        if self.downloads.mark(&track.id) == DownloadMark::None {
            self.download(track);
        } else {
            self.remove_download(&track.id);
        }
    }

    pub fn playlist_downloaded(&self, id: u64) -> bool {
        self.library
            .playlists
            .get(id)
            .is_some_and(|playlist| self.downloads.index.playlists.contains(&playlist.uid))
    }

    /// Keep a playlist downloaded (its new songs too), or stop: its songs
    /// go, unless downloaded by themselves or kept by another playlist.
    pub fn toggle_playlist_download(&mut self, id: u64) {
        let Some(playlist) = self.library.playlists.get(id).cloned() else {
            return;
        };
        if let Some(index) = self
            .downloads
            .index
            .playlists
            .iter()
            .position(|uid| *uid == playlist.uid)
        {
            self.downloads.index.playlists.remove(index);
            let kept: std::collections::HashSet<String> = self
                .library
                .playlists
                .iter()
                .filter(|other| self.downloads.index.playlists.contains(&other.uid))
                .flat_map(|other| other.tracks.iter().map(|t| t.id.clone()))
                .collect();
            for track in &playlist.tracks {
                let pinned = self
                    .downloads
                    .index
                    .get(&track.id)
                    .is_some_and(|song| song.pinned);
                if !pinned && !kept.contains(&track.id) {
                    self.remove_download(&track.id);
                }
            }
            self.save_downloads();
        } else {
            self.downloads.index.playlists.push(playlist.uid.clone());
            self.save_downloads();
            for track in playlist.tracks {
                self.downloads.from_playlist.insert(track.id.clone());
                self.queue_download(track);
            }
            self.set_download_notice(format!("Downloading {}…", playlist.name));
        }
    }

    pub fn tick_downloads(&mut self) {
        self.watch_internet();
        self.poll_download();
        // Downloaded playlists: new songs are downloaded too.
        if self
            .downloads
            .playlists_checked
            .is_none_or(|at| at.elapsed() >= PLAYLIST_CHECK)
        {
            self.downloads.playlists_checked = Some(Instant::now());
            let wanted: Vec<Track> = self
                .library
                .playlists
                .iter()
                .filter(|playlist| self.downloads.index.playlists.contains(&playlist.uid))
                .flat_map(|playlist| playlist.tracks.iter().cloned())
                .collect();
            for track in wanted {
                if self.downloads.mark(&track.id) == DownloadMark::None {
                    self.downloads.from_playlist.insert(track.id.clone());
                    self.queue_download(track);
                }
            }
        }
        self.start_next_download();
    }

    /// Check the internet now and then, on a thread.
    fn watch_internet(&mut self) {
        if self.downloads.checker || cfg!(test) {
            return;
        }
        self.downloads.checker = true;
        let online = self.downloads.online.clone();
        let waker = self.waker.clone();
        thread::Builder::new()
            .name("musikpl-online".into())
            .spawn(move || loop {
                let now = downloads::internet_reachable();
                if online.swap(now, Ordering::Relaxed) != now {
                    waker();
                }
                thread::sleep(ONLINE_CHECK);
            })
            .ok();
    }

    fn start_next_download(&mut self) {
        if self.downloads.active.is_some()
            || self.offline()
            || self
                .downloads
                .retry_at
                .is_some_and(|at| Instant::now() < at)
        {
            return;
        }
        let Some(track) = self.downloads.queue.pop_front() else {
            return;
        };
        let (Some(dir), Some(file)) = (downloads::dir(), downloads::file_name(&track.id)) else {
            return;
        };
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        let extractor = self.extractor.clone();
        let waker = self.waker.clone();
        let (thread_track, thread_progress, thread_cancel) =
            (track.clone(), progress.clone(), cancel.clone());
        thread::spawn(move || {
            let result = extractor.stream_url(&thread_track).and_then(|url| {
                downloads::fetch(&url, &dir.join(file), &thread_progress, &thread_cancel)
            });
            let _ = sender.send(result);
            waker();
        });
        self.downloads.active = Some(Active {
            track,
            progress,
            cancel,
            rx: receiver,
        });
    }

    fn poll_download(&mut self) {
        let Some(active) = &self.downloads.active else {
            return;
        };
        let result = match active.rx.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("The download stopped".into()),
        };
        let Some(active) = self.downloads.active.take() else {
            return;
        };
        match result {
            Ok(bytes) => {
                let Some(file) = downloads::file_name(&active.track.id) else {
                    return;
                };
                let pinned = !self.downloads.from_playlist.remove(&active.track.id);
                self.downloads
                    .index
                    .songs
                    .retain(|s| s.track.id != active.track.id);
                self.downloads.index.songs.push(Downloaded {
                    track: active.track,
                    file,
                    bytes,
                    at: unix_now(),
                    pinned,
                });
                self.save_downloads();
                self.downloads.retry_at = None;
            }
            Err(_) if active.cancel.load(Ordering::Relaxed) => {}
            Err(error) => {
                // Tried again later (a playlist's songs come back on their
                // own; a single song goes to the end of the line).
                self.set_download_notice(format!(
                    "Couldn't download {}: {error}",
                    active.track.title
                ));
                if !self.downloads.from_playlist.contains(&active.track.id) {
                    self.downloads.queue.push_back(active.track);
                }
                self.downloads.retry_at = Some(Instant::now() + RETRY_AFTER);
            }
        }
    }

    /// Offline with nothing queued: shuffle the downloads (Auto Mix can't
    /// reach YouTube).
    pub(super) fn fill_queue_offline(&mut self) -> bool {
        if !self.offline() || !self.auto_mix {
            return false;
        }
        let current = self.current.as_ref().map(|track| track.id.clone());
        let mut songs: Vec<Track> = self
            .downloads
            .tracks()
            .into_iter()
            .filter(|track| Some(&track.id) != current.as_ref())
            .collect();
        if songs.is_empty() {
            return false;
        }
        crate::recommendations::rng::Rng::from_time().shuffle(&mut songs);
        for track in songs {
            self.queue.enqueue_unique(track);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(100.0),
            channel: "Band".into(),
        }
    }

    #[test]
    fn songs_and_playlists_queue_for_download_and_can_be_undone() {
        let mut app = Engine::default();
        app.download(&track("a"));
        assert_eq!(app.downloads.mark("a"), DownloadMark::Queued);
        app.remove_download("a");
        assert_eq!(app.downloads.mark("a"), DownloadMark::None);

        let id = app.library.playlists.create();
        app.library.playlists.add(id, track("b"));
        app.library.playlists.add(id, track("c"));
        app.toggle_playlist_download(id);
        assert!(app.playlist_downloaded(id));
        assert_eq!(app.downloads.pending(), 2);
        // Songs added later follow.
        app.library.playlists.add(id, track("d"));
        app.downloads.playlists_checked = None;
        app.downloads.online.store(false, Ordering::Relaxed);
        app.tick_downloads();
        assert_eq!(app.downloads.mark("d"), DownloadMark::Queued);
        app.toggle_playlist_download(id);
        assert!(!app.playlist_downloaded(id));
        assert_eq!(app.downloads.pending(), 0);
    }

    #[test]
    fn offline_auto_mix_shuffles_the_downloads() {
        let mut app = Engine::default();
        app.downloads.online.store(false, Ordering::Relaxed);
        for id in ["x", "y"] {
            app.downloads.index.songs.push(Downloaded {
                track: track(id),
                file: format!("{id}.audio"),
                bytes: 1,
                at: 0,
                pinned: true,
            });
        }
        app.current = Some(track("x"));
        assert!(app.fill_queue_offline());
        let queued: Vec<_> = app.queue.iter().map(|t| t.id.clone()).collect();
        assert_eq!(queued, ["y"]);
    }
}
