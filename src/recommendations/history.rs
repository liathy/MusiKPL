use crate::domain::Track;

/// Songs remembered for Previous and for keeping Auto Mix from repeating.
/// Old entries are dropped so a long session stays small.
const MAX_HISTORY: usize = 500;

/// In-memory playback history used by automatic mix filtering and previous.
#[derive(Default)]
pub(crate) struct History {
    items: Vec<Track>,
}

impl History {
    pub(crate) fn push(&mut self, track: Track) {
        if self.items.len() >= MAX_HISTORY {
            self.items.remove(0);
        }
        self.items.push(track);
    }

    pub(crate) fn pop(&mut self) -> Option<Track> {
        self.items.pop()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &Track> {
        self.items.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_keeps_only_the_most_recent_songs() {
        let mut history = History::default();
        for index in 0..MAX_HISTORY + 3 {
            history.push(Track {
                title: index.to_string(),
                id: index.to_string(),
                duration: None,
                channel: String::new(),
            });
        }
        assert_eq!(history.iter().count(), MAX_HISTORY);
        assert_eq!(history.iter().next().map(|t| t.id.as_str()), Some("3"));
        assert_eq!(
            history.pop().map(|t| t.id),
            Some((MAX_HISTORY + 2).to_string())
        );
    }
}
