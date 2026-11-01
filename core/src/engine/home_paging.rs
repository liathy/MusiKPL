//! Home loads more as the user scrolls: each row shows its songs a page at
//! a time and searches for more when it runs out, and reaching the bottom
//! adds more rows.

use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver, TryRecvError},
};

use crate::{
    domain::Track,
    recommendations::{
        home::{finish_row, plan_more_rows, usable_songs, ROW_LEN},
        rng::Rng,
    },
};

use super::{
    library::{unix_now, HomeBuild},
    Engine,
};

#[derive(Default)]
pub struct HomePaging {
    /// Songs shown per row, by row title (one page unless asked for more).
    shown: HashMap<String, usize>,
    /// The row searching for more songs, and its results.
    row_fetch: Option<(String, Receiver<Vec<Track>>)>,
    /// More rows being built.
    more_rows: Option<Receiver<HomeBuild>>,
    /// No favorites left for more rows.
    pub no_more_rows: bool,
}

impl HomePaging {
    /// How many of a row's songs to show.
    pub fn shown(&self, title: &str, available: usize) -> usize {
        self.shown
            .get(title)
            .copied()
            .unwrap_or(ROW_LEN)
            .min(available)
    }

    pub fn loading_row(&self, title: &str) -> bool {
        self.row_fetch
            .as_ref()
            .is_some_and(|(loading, _)| loading == title)
    }

    pub fn loading_rows(&self) -> bool {
        self.more_rows.is_some()
    }

    pub fn busy(&self) -> bool {
        self.row_fetch.is_some() || self.more_rows.is_some()
    }
}

impl Engine {
    /// The user is nearing the end of a row: show its next page, and when
    /// less than a page is left in reserve, search for more in the
    /// background so the following page is ready too (infinite scrolling).
    pub fn more_in_row(&mut self, title: &str, available: usize) {
        let shown = self.library.paging.shown(title, available);
        let shown = if shown < available {
            let next = (shown + ROW_LEN).min(available);
            self.library.paging.shown.insert(title.to_owned(), next);
            next
        } else {
            shown
        };
        if available - shown >= ROW_LEN || self.library.paging.row_fetch.is_some() {
            return;
        }
        // Only rows built from searches can search again.
        let Some(row) = self
            .library
            .home
            .rows
            .iter_mut()
            .find(|row| row.title == title)
        else {
            return;
        };
        if row.next.is_empty() {
            return;
        }
        let source = row.next.remove(0);
        let extractor = self.extractor.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(extractor.list(&source, Some(30)).unwrap_or_default());
        });
        self.library.paging.row_fetch = Some((title.to_owned(), receiver));
    }

    /// The user reached the bottom of Home: add rows for favorites that
    /// don't have one yet.
    pub fn more_rows(&mut self) {
        let paging = &self.library.paging;
        if paging.more_rows.is_some() || paging.no_more_rows || self.library.building_home() {
            return;
        }
        let shown: HashSet<String> = self
            .library
            .home
            .rows
            .iter()
            .map(|row| row.title.clone())
            .collect();
        let plans = plan_more_rows(
            &self.library.taste,
            &self.settings.favorite_genres,
            &self.followed_artists(),
            unix_now(),
            &shown,
            &mut Rng::from_time(),
        );
        if plans.is_empty() {
            self.library.paging.no_more_rows = true;
            return;
        }
        self.library.paging.more_rows = Some(self.spawn_home_build(plans));
    }

    /// Collect songs and rows that finished loading.
    pub fn poll_home_paging(&mut self) {
        self.poll_row_fetch();
        self.poll_more_rows();
    }

    fn poll_row_fetch(&mut self) {
        let Some((title, receiver)) = &self.library.paging.row_fetch else {
            return;
        };
        let found = match receiver.try_recv() {
            Ok(found) => found,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        let title = title.clone();
        self.library.paging.row_fetch = None;
        let now = unix_now();
        let Some(row) = self
            .library
            .home
            .rows
            .iter_mut()
            .find(|row| row.title == title)
        else {
            return;
        };
        let known: HashSet<String> = row.tracks.iter().map(|track| track.id.clone()).collect();
        let before = row.tracks.len();
        let mut new = usable_songs(found, row.discover, &self.library.taste, now, &known);
        Rng::from_time().shuffle(&mut new);
        row.tracks.extend(new);
        if row.tracks.len() > before {
            // If the user already reached the end, show the new page now.
            if self.library.paging.shown(&title, before) >= before {
                self.library.paging.shown.insert(title, before + ROW_LEN);
            }
            self.save_home();
        }
    }

    fn poll_more_rows(&mut self) {
        let Some(receiver) = &self.library.paging.more_rows else {
            return;
        };
        let build = match receiver.try_recv() {
            Ok(build) => build,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Vec::new(),
        };
        self.library.paging.more_rows = None;
        let now = unix_now();
        let mut rng = Rng::from_time();
        let shown: HashSet<String> = self
            .library
            .home
            .rows
            .iter()
            .flat_map(|row| row.tracks.iter().map(|track| track.id.clone()))
            .collect();
        let rows: Vec<_> = build
            .into_iter()
            .filter_map(|(plan, candidates)| {
                finish_row(
                    &plan,
                    candidates,
                    &self.library.taste,
                    now,
                    &shown,
                    &mut rng,
                )
            })
            .collect();
        if rows.is_empty() {
            // Nothing usable came back; don't keep trying at the bottom.
            self.library.paging.no_more_rows = true;
            return;
        }
        self.library.home.rows.extend(rows);
        self.save_home();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recommendations::home::HomeRow;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(200.0),
            channel: format!("Artist {id}"),
        }
    }

    #[test]
    fn rows_page_through_their_songs_before_searching() {
        let mut app = Engine::default();
        let tracks: Vec<Track> = (0..30).map(|i| track(&format!("s{i}"))).collect();
        app.library.home.rows.push(HomeRow {
            title: "Row".into(),
            tracks,
            discover: false,
            next: vec!["ytsearch30:more".into()],
        });
        assert_eq!(app.library.paging.shown("Row", 30), ROW_LEN);
        app.more_in_row("Row", 30);
        assert_eq!(app.library.paging.shown("Row", 30), ROW_LEN * 2);
        assert!(
            app.library.paging.loading_row("Row"),
            "less than a page left: the next batch is fetched ahead"
        );
        assert!(app.library.home.rows[0].next.is_empty());
        app.more_in_row("Row", 30);
        assert_eq!(app.library.paging.shown("Row", 30), 30);
    }
}
