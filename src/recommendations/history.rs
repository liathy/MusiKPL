use crate::domain::Track;

/// In-memory playback history used by automatic mix filtering and previous.
#[derive(Default)]
pub(crate) struct History {
    items: Vec<Track>,
}

impl History {
    pub(crate) fn push(&mut self, track: Track) {
        self.items.push(track);
    }

    pub(crate) fn pop(&mut self) -> Option<Track> {
        self.items.pop()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Track> {
        self.items.iter()
    }
}
