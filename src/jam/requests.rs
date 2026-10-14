//! Host-authoritative guest request queue.
//!
//! This list is separate from the playback queue: a request only reaches
//! playback when the host accepts it, and nothing here can play, pause, seek,
//! or skip.

use std::collections::HashMap;

use super::protocol::{checked_track, RequestEntry, RequestStatus, TrackRef};

/// Open (pending) requests one guest may hold at a time.
const MAX_PENDING_PER_GUEST: usize = 5;
/// Entries kept in total, including finished ones shown as history.
const MAX_ENTRIES: usize = 40;
/// Minimum spacing between one guest's submissions.
const MIN_INTERVAL_MS: u64 = 1_500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestError {
    InvalidTrack,
    Duplicate,
    TooManyPending,
    QueueFull,
    TooFast,
}

impl RequestError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::InvalidTrack => "That track can't be requested",
            Self::Duplicate => "That track has already been requested",
            Self::TooManyPending => "Wait for the host to answer your earlier requests",
            Self::QueueFull => "The request list is full right now",
            Self::TooFast => "You're requesting too quickly; try again in a moment",
        }
    }
}

#[derive(Default)]
pub(crate) struct RequestQueue {
    entries: Vec<RequestEntry>,
    next_id: u64,
    last_submit_ms: HashMap<u64, u64>,
}

impl RequestQueue {
    pub(crate) fn entries(&self) -> &[RequestEntry] {
        &self.entries
    }

    /// Record a guest's request as pending, after validation and limits.
    pub(crate) fn submit(
        &mut self,
        requester_id: u64,
        requester: &str,
        track: &TrackRef,
        now_ms: u64,
    ) -> Result<u64, RequestError> {
        let track = checked_track(track).ok_or(RequestError::InvalidTrack)?;
        if self
            .last_submit_ms
            .get(&requester_id)
            .is_some_and(|last| now_ms.saturating_sub(*last) < MIN_INTERVAL_MS)
        {
            return Err(RequestError::TooFast);
        }
        let open = |entry: &RequestEntry| {
            matches!(
                entry.status,
                RequestStatus::Pending | RequestStatus::Accepted
            )
        };
        if self
            .entries
            .iter()
            .any(|entry| open(entry) && entry.track.video_id == track.video_id)
        {
            return Err(RequestError::Duplicate);
        }
        let pending = self
            .entries
            .iter()
            .filter(|entry| {
                entry.requester_id == requester_id && entry.status == RequestStatus::Pending
            })
            .count();
        if pending >= MAX_PENDING_PER_GUEST {
            return Err(RequestError::TooManyPending);
        }
        if self.entries.len() >= MAX_ENTRIES && !self.drop_oldest_finished() {
            return Err(RequestError::QueueFull);
        }

        self.last_submit_ms.insert(requester_id, now_ms);
        self.next_id += 1;
        self.entries.push(RequestEntry {
            id: self.next_id,
            track,
            requester_id,
            requester: requester.to_owned(),
            status: RequestStatus::Pending,
        });
        Ok(self.next_id)
    }

    /// Host accepts a pending request; returns the track to enqueue.
    pub(crate) fn accept(&mut self, id: u64) -> Option<TrackRef> {
        let entry = self.pending_mut(id)?;
        entry.status = RequestStatus::Accepted;
        Some(entry.track.clone())
    }

    /// Host declines a pending request.
    pub(crate) fn reject(&mut self, id: u64) -> bool {
        self.pending_mut(id)
            .map(|entry| entry.status = RequestStatus::Rejected)
            .is_some()
    }

    /// Host withdraws an open request. Returns the track when it had already
    /// been accepted, so the caller can take it out of the playback queue.
    pub(crate) fn remove(&mut self, id: u64) -> Option<Option<TrackRef>> {
        let entry = self.entries.iter_mut().find(|entry| entry.id == id)?;
        let queued = match entry.status {
            RequestStatus::Pending => None,
            RequestStatus::Accepted => Some(entry.track.clone()),
            _ => return None,
        };
        entry.status = RequestStatus::Removed;
        Some(queued)
    }

    /// Follow accepted requests through the host's playback queue: one that
    /// left the queue is now either playing or was removed by the host.
    pub(crate) fn reconcile(
        &mut self,
        current: Option<&str>,
        in_queue: impl Fn(&str) -> bool,
    ) -> bool {
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.status != RequestStatus::Accepted || in_queue(&entry.track.video_id) {
                continue;
            }
            entry.status = if current == Some(entry.track.video_id.as_str()) {
                RequestStatus::Played
            } else {
                RequestStatus::Removed
            };
            changed = true;
        }
        changed
    }

    fn pending_mut(&mut self, id: u64) -> Option<&mut RequestEntry> {
        self.entries
            .iter_mut()
            .find(|entry| entry.id == id && entry.status == RequestStatus::Pending)
    }

    fn drop_oldest_finished(&mut self) -> bool {
        let finished = self.entries.iter().position(|entry| {
            matches!(
                entry.status,
                RequestStatus::Played | RequestStatus::Rejected | RequestStatus::Removed
            )
        });
        finished.map(|index| self.entries.remove(index)).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jam::protocol::track_ref;

    fn track(id: &str) -> TrackRef {
        track_ref(id, "Song", "Artist", Some(120.0)).unwrap()
    }

    fn status(queue: &RequestQueue, id: u64) -> RequestStatus {
        queue
            .entries()
            .iter()
            .find(|entry| entry.id == id)
            .unwrap()
            .status
    }

    #[test]
    fn requests_move_from_pending_to_accepted_to_played() {
        let mut queue = RequestQueue::default();
        let id = queue.submit(1, "Ana", &track("one"), 0).unwrap();
        assert_eq!(status(&queue, id), RequestStatus::Pending);

        assert_eq!(queue.accept(id), Some(track("one")));
        assert_eq!(status(&queue, id), RequestStatus::Accepted);
        // Accepting twice must not enqueue the track twice.
        assert_eq!(queue.accept(id), None);

        // Still waiting in the host's playback queue.
        assert!(!queue.reconcile(Some("other"), |id| id == "one"));
        assert_eq!(status(&queue, id), RequestStatus::Accepted);

        // The host's player reached it.
        assert!(queue.reconcile(Some("one"), |_| false));
        assert_eq!(status(&queue, id), RequestStatus::Played);
    }

    #[test]
    fn accepted_request_dropped_from_the_queue_is_reported_removed() {
        let mut queue = RequestQueue::default();
        let id = queue.submit(1, "Ana", &track("one"), 0).unwrap();
        queue.accept(id);
        assert!(queue.reconcile(Some("other"), |_| false));
        assert_eq!(status(&queue, id), RequestStatus::Removed);
    }

    #[test]
    fn host_can_reject_and_remove() {
        let mut queue = RequestQueue::default();
        let rejected = queue.submit(1, "Ana", &track("one"), 0).unwrap();
        assert!(queue.reject(rejected));
        assert!(!queue.reject(rejected));
        assert_eq!(queue.accept(rejected), None);

        let pending = queue.submit(1, "Ana", &track("two"), 5_000).unwrap();
        assert_eq!(queue.remove(pending), Some(None));

        let accepted = queue.submit(1, "Ana", &track("three"), 10_000).unwrap();
        queue.accept(accepted);
        assert_eq!(queue.remove(accepted), Some(Some(track("three"))));
        assert_eq!(queue.remove(accepted), None);
        assert_eq!(queue.remove(999), None);
    }

    #[test]
    fn invalid_duplicate_and_spammed_requests_are_refused() {
        let mut queue = RequestQueue::default();
        let mut invalid = track("one");
        invalid.video_id = "../secret".into();
        assert_eq!(
            queue.submit(1, "Ana", &invalid, 0),
            Err(RequestError::InvalidTrack)
        );

        queue.submit(1, "Ana", &track("one"), 0).unwrap();
        assert_eq!(
            queue.submit(1, "Ana", &track("two"), 100),
            Err(RequestError::TooFast)
        );
        // Another guest asking for the same open track is a duplicate.
        assert_eq!(
            queue.submit(2, "Ben", &track("one"), 100),
            Err(RequestError::Duplicate)
        );

        for (index, id) in ["a", "b", "c", "d"].into_iter().enumerate() {
            queue
                .submit(1, "Ana", &track(id), 10_000 * (index as u64 + 1))
                .unwrap();
        }
        assert_eq!(
            queue.submit(1, "Ana", &track("f"), 90_000),
            Err(RequestError::TooManyPending)
        );
        // The limit is per guest.
        assert!(queue.submit(2, "Ben", &track("f"), 90_000).is_ok());
    }

    #[test]
    fn a_finished_track_can_be_requested_again_and_history_is_bounded() {
        let mut queue = RequestQueue::default();
        let first = queue.submit(1, "Ana", &track("one"), 0).unwrap();
        queue.reject(first);
        assert!(queue.submit(1, "Ana", &track("one"), 5_000).is_ok());

        let mut queue = RequestQueue::default();
        for index in 0..(MAX_ENTRIES as u64 + 10) {
            let id = queue
                .submit(index, "Guest", &track(&format!("id{index}")), 0)
                .unwrap();
            queue.reject(id);
        }
        assert_eq!(queue.entries().len(), MAX_ENTRIES);

        // Open requests are never evicted to make room.
        let mut queue = RequestQueue::default();
        for index in 0..MAX_ENTRIES as u64 {
            queue
                .submit(index, "Guest", &track(&format!("id{index}")), 0)
                .unwrap();
        }
        assert_eq!(
            queue.submit(999, "Guest", &track("overflow"), 0),
            Err(RequestError::QueueFull)
        );
    }
}
