//! Back and forward between pages, like a browser (the shell maps its own
//! buttons and keys onto `go_back` and `go_forward`).
//!
//! Pages are recorded by watching which one is showing, so every way of
//! opening one (menu, cards, search, links) is covered without each having
//! to remember it.

use super::{Engine, View};

/// Pages kept in each direction.
const LIMIT: usize = 50;

/// One page and what it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Search(String),
    Playlist(u64),
    Artist(String),
    Profile(String),
}

#[derive(Default)]
pub struct Navigation {
    back: Vec<Page>,
    forward: Vec<Page>,
    /// The page showing when last checked.
    current: Option<Page>,
}

impl Navigation {
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Note the page now showing: a new one puts the last on the back list
    /// and clears forward, as opening a link does in a browser.
    fn visit(&mut self, page: Page) {
        if self.current.as_ref() == Some(&page) {
            return;
        }
        if let Some(previous) = self.current.replace(page) {
            push(&mut self.back, previous);
            self.forward.clear();
        }
    }
}

fn push(list: &mut Vec<Page>, page: Page) {
    list.push(page);
    if list.len() > LIMIT {
        list.remove(0);
    }
}

impl Engine {
    fn current_page(&self) -> Page {
        match self.view {
            View::Home => Page::Home,
            View::Search => Page::Search(self.results_query.clone()),
            View::Playlist => self.open_playlist.map_or(Page::Home, Page::Playlist),
            View::Artist => Page::Artist(self.browse.artist_name.clone()),
            View::Profile => Page::Profile(self.browse.profile_user.clone()),
        }
    }

    /// Every tick: a page change puts the last page on the back list.
    pub fn track_page(&mut self) {
        let page = self.current_page();
        self.nav.visit(page);
    }

    /// The open artist page took Last.fm's name for the artist: it's the
    /// same page, not a new one.
    pub fn artist_renamed(&mut self) {
        if self.view == View::Artist && matches!(self.nav.current, Some(Page::Artist(_))) {
            self.nav.current = Some(self.current_page());
        }
    }

    pub fn go_back(&mut self) {
        while let Some(page) = self.nav.back.pop() {
            let from = self.nav.current.take();
            if self.show_page(&page) {
                if let Some(from) = from {
                    push(&mut self.nav.forward, from);
                }
                self.nav.current = Some(self.current_page());
                return;
            }
            // That page is gone (a deleted playlist): skip it.
            self.nav.current = from;
        }
    }

    pub fn go_forward(&mut self) {
        while let Some(page) = self.nav.forward.pop() {
            let from = self.nav.current.take();
            if self.show_page(&page) {
                if let Some(from) = from {
                    push(&mut self.nav.back, from);
                }
                self.nav.current = Some(self.current_page());
                return;
            }
            self.nav.current = from;
        }
    }

    /// Show `page` again. False when it no longer exists.
    fn show_page(&mut self, page: &Page) -> bool {
        match page {
            Page::Home => self.view = View::Home,
            Page::Search(query) => {
                // The last search's results are still here; an older one
                // is searched again.
                if *query != self.results_query && !query.is_empty() {
                    self.search.clone_from(query);
                    self.search();
                }
                self.view = View::Search;
            }
            Page::Playlist(id) => {
                if self.library.playlists.get(*id).is_none() {
                    return false;
                }
                self.open_playlist(*id);
            }
            Page::Artist(name) => self.open_artist(name),
            Page::Profile(user) => self.open_user_profile(user),
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_pages_go_on_the_back_list_and_clear_forward() {
        let mut history = Navigation::default();
        history.visit(Page::Home);
        assert!(!history.can_back());
        history.visit(Page::Artist("Hardwell".into()));
        history.visit(Page::Artist("Hardwell".into()));
        assert_eq!(history.back, vec![Page::Home]);
        history.forward.push(Page::Playlist(3));
        history.visit(Page::Profile("ana".into()));
        assert!(!history.can_forward());
        assert_eq!(history.back.len(), 2);
    }

    #[test]
    fn the_back_list_is_capped() {
        let mut history = Navigation::default();
        for id in 0..(LIMIT as u64 + 10) {
            history.visit(Page::Playlist(id));
        }
        assert_eq!(history.back.len(), LIMIT);
        assert_eq!(history.back[0], Page::Playlist(9));
    }
}
