use super::*;

fn track(id: &str, title: &str) -> Track {
    Track {
        title: title.into(),
        id: id.into(),
        duration: Some(125.0),
        channel: "Test artist".into(),
    }
}

/// An engine that finds nothing and plays nothing, so loads fail fast and
/// nothing reaches the network.
fn offline_app() -> Engine {
    Engine::default()
}

fn queued_ids(app: &Engine) -> Vec<String> {
    app.queue.iter().map(|track| track.id.clone()).collect()
}

/// Stand in for a finished mix search.
fn deliver_mix(app: &mut Engine, result: Result<Vec<Track>, String>) {
    let (sender, receiver) = mpsc::channel();
    app.automix.rx = Some(receiver);
    sender.send(result).unwrap();
    app.poll_mix();
}

#[test]
fn mix_picks_skip_what_was_played_queued_or_is_playing() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));
    app.history.push(track("played", "Played"));
    app.queue.push_back(track("queued", "Already queued"));

    let added = app.queue_mix_picks(vec![
        track("current", "Current"),
        track("played", "Played"),
        track("queued", "Already queued"),
        track("lyrics", "Current (Lyrics)"),
        track("new", "New song"),
    ]);

    assert_eq!(added, 1);
    assert_eq!(queued_ids(&app), ["queued", "new"]);
    assert!(app.automix.picks.contains("new"));
}

#[test]
fn a_new_search_keeps_the_users_songs_and_drops_old_mix_picks() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));
    app.queue_mix_picks(vec![
        track("pick-1", "Pick one"),
        track("pick-2", "Pick two"),
    ]);
    app.add_to_queue(track("mine", "My song"));

    app.reset_mix();

    assert_eq!(queued_ids(&app), ["mine"]);
    assert!(!app.automix.busy() && !app.automix.waiting);
}

#[test]
fn songs_the_user_adds_play_before_mix_picks() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));
    app.queue_mix_picks(vec![track("pick", "Pick")]);
    app.add_to_queue(track("mine-1", "Mine one"));
    app.add_to_queue(track("mine-2", "Mine two"));
    assert_eq!(queued_ids(&app), ["mine-1", "mine-2", "pick"]);
}

#[test]
fn adding_a_song_with_nothing_playing_plays_it() {
    let mut app = offline_app();
    app.add_to_queue(track("mine", "Mine"));
    assert!(app.loading);
    assert_eq!(
        app.pending_track.as_ref().map(|t| t.id.as_str()),
        Some("mine")
    );
    assert!(app.queue.is_empty());
}

#[test]
fn an_empty_queue_waits_for_auto_mix_then_plays_its_first_song() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));

    app.next();
    assert!(app.automix.waiting, "the session keeps going");
    assert_eq!(app.current.as_ref().map(|t| t.id.as_str()), Some("current"));

    app.keep_mix_filled();
    assert!(app.automix.busy(), "a refill starts right away");

    deliver_mix(
        &mut app,
        Ok(vec![track("next", "Next"), track("after", "After")]),
    );
    assert!(!app.automix.waiting);
    assert!(app.loading);
    assert_eq!(
        app.pending_track.as_ref().map(|t| t.id.as_str()),
        Some("next")
    );
    assert_eq!(queued_ids(&app), ["after"]);
}

#[test]
fn a_failed_or_crashed_refill_is_retried_later() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));
    app.next();

    deliver_mix(&mut app, Err("YouTube had no Mix".into()));
    assert!(!app.automix.busy());
    assert_eq!(app.automix.failures, 1);
    assert!(app.automix.retry_at.is_some());
    assert!(matches!(app.error, Some(AppError::Mix(_))));
    // Waiting for the retry, not hammering yt-dlp every frame.
    app.keep_mix_filled();
    assert!(!app.automix.busy());

    // A worker that disappears without answering doesn't jam Auto Mix.
    let (sender, receiver) = mpsc::channel::<Result<Vec<Track>, String>>();
    app.automix.rx = Some(receiver);
    drop(sender);
    app.poll_mix();
    assert!(!app.automix.busy());
    assert_eq!(app.automix.failures, 2);
}

#[test]
fn the_queue_is_refilled_before_it_runs_out() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));
    app.queue_mix_picks(vec![track("a", "A"), track("b", "B"), track("c", "C")]);
    app.keep_mix_filled();
    assert!(!app.automix.busy(), "enough songs are waiting");

    app.queue.pop_next();
    app.keep_mix_filled();
    assert!(app.automix.busy(), "refilled while the song still plays");
}

#[test]
fn turning_auto_mix_off_ends_the_wait() {
    let mut app = offline_app();
    app.current = Some(track("current", "Current"));
    app.next();
    app.keep_mix_filled();

    app.set_auto_mix(false);
    assert!(!app.automix.waiting && !app.automix.busy());
    assert!(app.current.is_none());
    app.keep_mix_filled();
    assert!(!app.automix.busy());
}

#[test]
fn songs_that_fail_to_load_are_skipped_a_few_times_at_most() {
    let mut app = offline_app();
    for id in ["a", "b", "c", "d", "e"] {
        app.queue.push_back(track(id, id));
    }
    for _ in 0..MAX_FAILED_SKIPS + 2 {
        app.skip_failed_track("stream unavailable".into());
    }
    // Two skips, then it stops instead of burning through the queue.
    assert_eq!(app.queue.len(), 3);
}

#[test]
fn a_preloaded_song_starts_without_looking_it_up_again() {
    let mut app = offline_app();
    app.preload = Some(Preload {
        id: "next".into(),
        rx: None,
        url: Some("https://stream.example/next".into()),
        at: Instant::now(),
    });
    app.start_track(track("next", "Next"), true);
    let (started, url, _) = app.play_rx.as_ref().unwrap().try_recv().unwrap().unwrap();
    assert_eq!(started.id, "next");
    assert_eq!(url, "https://stream.example/next");
    assert!(app.preload.is_none(), "used once");

    // A preload for another song isn't used.
    app.preload = Some(Preload {
        id: "other".into(),
        rx: None,
        url: Some("https://stream.example/other".into()),
        at: Instant::now(),
    });
    app.start_track(track("next", "Next"), true);
    // It's looked up instead (which fails offline, maybe already).
    let result = app.play_rx.as_ref().unwrap().try_recv();
    assert!(!matches!(result, Ok(Ok(_))));
}

#[test]
fn muting_remembers_the_volume_to_return_to() {
    let mut app = offline_app();
    app.set_volume(64);
    app.toggle_mute();
    assert_eq!(app.player.volume, 0);
    app.toggle_mute();
    assert_eq!(app.player.volume, 64);
    // Dragged to zero, then unmuted: a sensible level comes back.
    app.set_volume(0);
    app.unmute_volume = 0;
    app.toggle_mute();
    assert_eq!(app.player.volume, 50);
}

#[test]
fn the_app_starts_on_home_and_searching_switches_to_results() {
    let mut app = offline_app();
    assert_eq!(app.view, View::Home);
    app.search = "hardstyle".into();
    app.search();
    assert_eq!(app.view, View::Search);
    assert_eq!(app.settings.recent_searches, ["hardstyle"]);
}

#[test]
fn a_resumed_song_loads_from_where_it_was_left_on_play() {
    let mut app = offline_app();
    app.current = Some(track("last", "Last"));
    app.player.show_paused_at(80.0);
    app.resume_at = Some(80.0);

    app.seek(95.0);
    assert_eq!(app.resume_at, Some(95.0));
    assert!(!app.loading, "seeking doesn't load anything yet");

    app.toggle_pause();
    assert!(app.loading);
    assert_eq!(app.resume_at, None);
    assert_eq!(app.start_at, 95.0);
    assert_eq!(
        app.pending_track.as_ref().map(|t| t.id.as_str()),
        Some("last")
    );
}

#[test]
fn the_player_bar_shows_the_song_being_loaded() {
    let mut app = offline_app();
    app.current = Some(track("old", "Old"));
    app.pending_track = Some(track("new", "New"));
    app.loading = true;
    assert_eq!(app.shown_track().map(|t| t.id.as_str()), Some("new"));
    app.loading = false;
    assert_eq!(app.shown_track().map(|t| t.id.as_str()), Some("old"));
}
