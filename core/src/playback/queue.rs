use crate::domain::Track;
use std::collections::VecDeque;

/// The single owner of tracks waiting for playback.
///
/// History and the currently playing track are intentionally kept by the
/// application coordinator for now; this type owns only queued tracks and all
/// collection-level queue operations.
#[derive(Default)]
pub struct Queue {
    items: VecDeque<Track>,
}

impl Queue {
    #[cfg(any(test, feature = "testing"))]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Track> {
        self.items.iter()
    }

    pub fn push_back(&mut self, track: Track) {
        self.items.push_back(track);
    }

    pub fn push_front(&mut self, track: Track) {
        self.items.push_front(track);
    }

    pub fn pop_next(&mut self) -> Option<Track> {
        self.items.pop_front()
    }

    pub fn remove(&mut self, index: usize) -> Option<Track> {
        self.items.remove(index)
    }

    pub fn remove_id(&mut self, id: &str) -> Option<Track> {
        let position = self.items.iter().position(|track| track.id == id)?;
        self.remove(position)
    }

    pub fn contains_id(&self, id: &str) -> bool {
        self.items.iter().any(|track| track.id == id)
    }

    pub fn enqueue_unique(&mut self, track: Track) -> bool {
        if self.contains_id(&track.id) {
            return false;
        }
        self.push_back(track);
        true
    }

    /// Insert at `index` (clamped to the end) unless the song is queued.
    pub fn insert_unique(&mut self, index: usize, track: Track) -> bool {
        if self.contains_id(&track.id) {
            return false;
        }
        self.items.insert(index.min(self.items.len()), track);
        true
    }

    /// Position of the first track matching `predicate`.
    pub fn position(&self, predicate: impl Fn(&Track) -> bool) -> Option<usize> {
        self.items.iter().position(predicate)
    }

    pub fn retain(&mut self, keep: impl FnMut(&Track) -> bool) {
        self.items.retain(keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            title: id.into(),
            id: id.into(),
            duration: Some(60.0),
            channel: "Test".into(),
        }
    }

    #[test]
    fn unique_enqueue_preserves_order_and_rejects_duplicate_ids() {
        let mut queue = Queue::default();
        assert!(queue.enqueue_unique(track("one")));
        assert!(!queue.enqueue_unique(track("one")));
        assert!(queue.enqueue_unique(track("two")));
        assert_eq!(queue.len(), 2);
        assert_eq!(
            queue.iter().last().map(|track| track.id.as_str()),
            Some("two")
        );
    }

    #[test]
    fn inserts_and_filters_in_place() {
        let mut queue = Queue::default();
        queue.push_back(track("a"));
        queue.push_back(track("b"));
        assert!(queue.insert_unique(1, track("mine")));
        assert!(!queue.insert_unique(0, track("a")));
        assert!(queue.insert_unique(99, track("end")));
        let ids = |queue: &Queue| queue.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&queue), ["a", "mine", "b", "end"]);
        assert_eq!(queue.position(|t| t.id == "b"), Some(2));
        queue.retain(|t| t.id != "a");
        assert_eq!(ids(&queue), ["mine", "b", "end"]);
    }

    #[test]
    fn next_and_previous_operations_preserve_expected_order() {
        let mut queue = Queue::default();
        queue.push_back(track("one"));
        queue.push_back(track("two"));
        assert_eq!(queue.pop_next().map(|track| track.id), Some("one".into()));
        queue.push_front(track("previous"));
        assert_eq!(
            queue.pop_next().map(|track| track.id),
            Some("previous".into())
        );
        assert_eq!(queue.pop_next().map(|track| track.id), Some("two".into()));
        assert!(queue.is_empty());
    }
}
