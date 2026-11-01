//! Last.fm: connecting an account, scrobbling, and genre lookups.
//!
//! All network calls run on background threads; the app submits jobs and
//! polls for events once per frame, like the yt-dlp workers.

mod api;
pub mod browse;
mod scrobble;

pub use api::Credentials;
pub use scrobble::{ScrobbleStep, ScrobbleTracker};

/// MusiKPL's own read-only Last.fm API key, used for genres when
/// the user hasn't entered one. It comes from `lastfm-api-key.txt` (ignored
/// by git) when building; an API key alone can't scrobble or reach
/// accounts.
const BUILT_IN_API_KEY: &str = env!("MUSIKPL_LASTFM_KEY");

/// MusiKPL's shared secret, from lastfm-api-secret.txt (ignored by git).
/// With it, users connect their account in one click, without keys.
const BUILT_IN_API_SECRET: &str = env!("MUSIKPL_LASTFM_SECRET");

/// The API key for read-only lookups: the user's own, else MusiKPL's.
pub fn lookup_key(user_key: &str) -> Option<String> {
    [user_key.trim(), BUILT_IN_API_KEY]
        .into_iter()
        .find(|key| !key.is_empty())
        .map(str::to_owned)
}

/// The key and secret to connect and scrobble with: the user's own pair if
/// they entered one, else MusiKPL's built-in pair.
pub fn app_keys(user_key: &str, user_secret: &str) -> Option<(String, String)> {
    let (key, secret) = (user_key.trim(), user_secret.trim());
    if !key.is_empty() && !secret.is_empty() {
        return Some((key.to_owned(), secret.to_owned()));
    }
    built_in_keys()
}

/// MusiKPL's own key pair, when this build carries both.
pub fn built_in_keys() -> Option<(String, String)> {
    (!BUILT_IN_API_KEY.is_empty() && !BUILT_IN_API_SECRET.is_empty())
        .then(|| (BUILT_IN_API_KEY.to_owned(), BUILT_IN_API_SECRET.to_owned()))
}

/// Where new users sign up.
pub const JOIN_URL: &str = "https://www.last.fm/join";

use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{domain::SongInfo, platform};

/// How long to wait for the user to approve MusiKPL in the browser.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(300);
const APPROVAL_POLL: Duration = Duration::from_secs(3);
/// Spacing between genre lookups, well inside Last.fm's rate limit.
const LOOKUP_SPACING: Duration = Duration::from_millis(250);

pub enum LastFmJob {
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
    /// Love or unlove a song on the user's profile.
    Love {
        credentials: Credentials,
        song: SongInfo,
        love: bool,
    },
    /// The song's genres, for the taste profile.
    Tags {
        api_key: String,
        video_id: String,
        song: SongInfo,
    },
}

#[derive(Debug)]
pub enum LastFmEvent {
    /// Raw Last.fm tags (possibly none); `None` when the lookup failed.
    Tags {
        video_id: String,
        tags: Option<Vec<String>>,
    },
    /// A scrobble or now-playing update failed.
    Failed(String),
    /// Artists found for a search.
    Artists {
        query: String,
        result: Result<Vec<browse::ArtistHit>, String>,
    },
    /// An artist's page.
    Artist {
        name: String,
        result: Result<browse::ArtistPage, String>,
    },
    /// Who the user follows on Last.fm.
    Following {
        user: String,
        result: Result<Vec<browse::Followed>, String>,
    },
    /// The user's loved songs.
    Loved {
        user: String,
        result: Result<Vec<browse::Loved>, String>,
    },
    /// The user's most-scrobbled artists.
    TopArtists {
        user: String,
        result: Result<Vec<String>, String>,
    },
    /// Songs Last.fm found for a search.
    Tracks {
        query: String,
        result: Result<Vec<browse::TrackHit>, String>,
    },
    /// The Last.fm user whose name is the search, if any.
    People {
        query: String,
        result: Result<Option<browse::Profile>, String>,
    },
    /// A user's profile and stats.
    Profile {
        user: String,
        period: browse::Period,
        result: Result<browse::Profile, String>,
    },
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

/// Something to look up for browsing.
pub enum BrowseRequest {
    Artists {
        api_key: String,
        query: String,
    },
    Artist {
        api_key: String,
        name: String,
    },
    People {
        api_key: String,
        query: String,
    },
    Tracks {
        api_key: String,
        query: String,
    },
    /// The best Last.fm artist for a YouTube artist name; answered as
    /// [`LastFmEvent::Artist`] under `names[0]`.
    BestArtist {
        api_key: String,
        names: Vec<String>,
    },
    Following {
        api_key: String,
        user: String,
    },
    Loved {
        api_key: String,
        user: String,
    },
    TopArtists {
        api_key: String,
        user: String,
    },
    Profile {
        api_key: String,
        user: String,
        period: browse::Period,
    },
}

pub struct LastFm {
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
    pub fn submit(&mut self, job: LastFmJob) {
        let sender = self.jobs.get_or_insert_with(|| {
            let (sender, jobs) = mpsc::channel();
            let events = self.events_tx.clone();
            thread::spawn(move || run_jobs(&jobs, &events));
            sender
        });
        let _ = sender.send(job);
    }

    /// Look something up for browsing on its own thread, so it never waits
    /// behind scrobbles or genre lookups.
    pub fn browse(&self, request: BrowseRequest) {
        let events = self.events_tx.clone();
        thread::spawn(move || {
            let event = match request {
                BrowseRequest::Artists { api_key, query } => LastFmEvent::Artists {
                    result: browse::search_artists(&api_key, &query),
                    query,
                },
                BrowseRequest::BestArtist { api_key, names } => LastFmEvent::Artist {
                    result: browse::best_artist_page(&api_key, &names),
                    name: names.first().cloned().unwrap_or_default(),
                },
                BrowseRequest::Following { api_key, user } => LastFmEvent::Following {
                    result: browse::following(&api_key, &user),
                    user,
                },
                BrowseRequest::Loved { api_key, user } => LastFmEvent::Loved {
                    result: browse::loved(&api_key, &user),
                    user,
                },
                BrowseRequest::TopArtists { api_key, user } => LastFmEvent::TopArtists {
                    result: browse::top_artists(&api_key, &user),
                    user,
                },
                BrowseRequest::Tracks { api_key, query } => LastFmEvent::Tracks {
                    result: browse::search_tracks(&api_key, &query),
                    query,
                },
                BrowseRequest::People { api_key, query } => LastFmEvent::People {
                    result: browse::find_user(&api_key, &query),
                    query,
                },
                BrowseRequest::Artist { api_key, name } => LastFmEvent::Artist {
                    result: browse::artist_page(&api_key, &name),
                    name,
                },
                BrowseRequest::Profile {
                    api_key,
                    user,
                    period,
                } => LastFmEvent::Profile {
                    result: browse::profile(&api_key, &user, period),
                    user,
                    period,
                },
            };
            let _ = events.send(event);
        });
    }

    pub fn poll(&self) -> Vec<LastFmEvent> {
        self.events.try_iter().collect()
    }

    /// Start desktop authentication: get a token, open the approval page,
    /// and wait in the background until the user approves it.
    pub fn connect(&mut self, api_key: String, api_secret: String) {
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

    pub fn cancel_connect(&mut self) {
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
    let _ = platform::open_url(&url);
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
            LastFmJob::Love {
                credentials,
                song,
                love,
            } => api::love(&credentials, &song, love)
                .err()
                .map(LastFmEvent::Failed),
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
