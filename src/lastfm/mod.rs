//! Last.fm: connecting an account, scrobbling, and album-art lookups.
//!
//! All network calls run on background threads; the app submits jobs and
//! polls for events once per frame, like the yt-dlp workers.

mod api;
mod scrobble;

pub(crate) use api::Credentials;
pub(crate) use scrobble::{ScrobbleStep, ScrobbleTracker};

/// MusiKPL's own read-only Last.fm API key, used for genres and covers when
/// the user hasn't entered one. It comes from `lastfm-api-key.txt` (ignored
/// by git) when building; an API key alone can't scrobble or reach
/// accounts.
const BUILT_IN_API_KEY: &str = env!("MUSIKPL_LASTFM_KEY");

/// The API key for read-only lookups: the user's own, else MusiKPL's.
pub(crate) fn lookup_key(user_key: &str) -> Option<String> {
    [user_key.trim(), BUILT_IN_API_KEY]
        .into_iter()
        .find(|key| !key.is_empty())
        .map(str::to_owned)
}

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{domain::SongInfo, infrastructure::system};

/// How long to wait for the user to approve MusiKPL in the browser.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);
const APPROVAL_POLL: Duration = Duration::from_secs(3);
/// Spacing between album-art lookups, well inside Last.fm's rate limit.
const LOOKUP_SPACING: Duration = Duration::from_millis(250);

pub(crate) enum LastFmJob {
    NowPlaying {
        credentials: Credentials,
        song: SongInfo,
        duration: Option<f64>,
    },
    Scrobble {
        credentials: Credentials,
        song: SongInfo,
        started_at: u64,
        duration: Option<f64>,
    },
    AlbumArt {
        api_key: String,
        video_id: String,
        song: SongInfo,
    },
    /// The song's genres, for the taste profile.
    Tags {
        api_key: String,
        video_id: String,
        song: SongInfo,
    },
}

#[derive(Debug)]
pub(crate) enum LastFmEvent {
    AlbumArt {
        video_id: String,
        url: Option<String>,
    },
    /// Raw Last.fm tags (possibly none); `None` when the lookup failed.
    Tags {
        video_id: String,
        tags: Option<Vec<String>>,
    },
    /// A scrobble or now-playing update failed.
    Failed(String),
    /// The approval page is open in the browser.
    AwaitingApproval {
        url: String,
    },
    Connected {
        username: String,
        session_key: String,
    },
    ConnectFailed(String),
}

pub(crate) struct LastFm {
    jobs: Option<Sender<LastFmJob>>,
    events_tx: Sender<LastFmEvent>,
    events: Receiver<LastFmEvent>,
    connecting: Option<Arc<AtomicBool>>,
}

impl Default for LastFm {
    fn default() -> Self {
        let (events_tx, events) = mpsc::channel();
        Self {
            jobs: None,
            events_tx,
            events,
            connecting: None,
        }
    }
}

impl Drop for LastFm {
    fn drop(&mut self) {
        self.cancel_connect();
    }
}

impl LastFm {
    pub(crate) fn submit(&mut self, job: LastFmJob) {
        let sender = self.jobs.get_or_insert_with(|| {
            let (sender, jobs) = mpsc::channel();
            let events = self.events_tx.clone();
            thread::spawn(move || run_jobs(&jobs, &events));
            sender
        });
        let _ = sender.send(job);
    }

    pub(crate) fn poll(&self) -> Vec<LastFmEvent> {
        self.events.try_iter().collect()
    }

    /// Start desktop authentication: get a token, open the approval page,
    /// and wait in the background until the user approves it.
    pub(crate) fn connect(&mut self, api_key: String, api_secret: String) {
        self.cancel_connect();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.connecting = Some(cancelled.clone());
        let events = self.events_tx.clone();
        thread::spawn(move || {
            let result = authorize(&api_key, &api_secret, &cancelled, &events);
            if !cancelled.load(Ordering::SeqCst) {
                let _ = events.send(match result {
                    Ok((username, session_key)) => LastFmEvent::Connected {
                        username,
                        session_key,
                    },
                    Err(message) => LastFmEvent::ConnectFailed(message),
                });
            }
        });
    }

    pub(crate) fn cancel_connect(&mut self) {
        if let Some(cancelled) = self.connecting.take() {
            cancelled.store(true, Ordering::SeqCst);
        }
    }
}

fn authorize(
    api_key: &str,
    api_secret: &str,
    cancelled: &AtomicBool,
    events: &Sender<LastFmEvent>,
) -> Result<(String, String), String> {
    let token = api::get_token(api_key, api_secret)?;
    let url = api::auth_url(api_key, &token);
    // The page link is shown in Settings too, in case the browser didn't open.
    let _ = system::open_url(&url);
    let _ = events.send(LastFmEvent::AwaitingApproval { url });

    let started = Instant::now();
    while started.elapsed() < APPROVAL_TIMEOUT {
        thread::sleep(APPROVAL_POLL);
        if cancelled.load(Ordering::SeqCst) {
            return Err("Cancelled".into());
        }
        if let Some(session) = api::get_session(api_key, api_secret, &token)? {
            return Ok(session);
        }
    }
    Err("Last.fm approval timed out. Press Connect to try again".into())
}

fn run_jobs(jobs: &Receiver<LastFmJob>, events: &Sender<LastFmEvent>) {
    for job in jobs {
        let event = match job {
            LastFmJob::NowPlaying {
                credentials,
                song,
                duration,
            } => api::update_now_playing(&credentials, &song, duration)
                .err()
                .map(LastFmEvent::Failed),
            LastFmJob::Scrobble {
                credentials,
                song,
                started_at,
                duration,
            } => api::scrobble(&credentials, &song, started_at, duration)
                .err()
                .map(LastFmEvent::Failed),
            LastFmJob::AlbumArt {
                api_key,
                video_id,
                song,
            } => {
                // A failed lookup simply keeps the YouTube thumbnail.
                let url = api::album_art(&api_key, &song).ok().flatten();
                thread::sleep(LOOKUP_SPACING);
                Some(LastFmEvent::AlbumArt { video_id, url })
            }
            LastFmJob::Tags {
                api_key,
                video_id,
                song,
            } => {
                let tags = api::tags(&api_key, &song).ok();
                thread::sleep(LOOKUP_SPACING);
                Some(LastFmEvent::Tags { video_id, tags })
            }
        };
        if let Some(event) = event {
            if events.send(event).is_err() {
                return;
            }
        }
    }
}
