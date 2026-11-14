//! Sync, the tempo master and the master clock. Behaviour notes: `docs/behavior/sync.md`.

mod common;

use std::path::PathBuf;
use std::sync::OnceLock;

use common::{RATE, Rig, rate, sine_file};
use platter_core::DeckId;
use platter_engine::{
    ClockMode, Command, DeckSnapshot, Engine, EngineError, SizeStep, Snapshot, SyncMode,
};
use platter_testkit::measure::max_step;
use platter_testkit::signal::channel;

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;

/// Two twelve-second tones, written once for all the tests of this file.
fn files() -> &'static [PathBuf; 2] {
    static FILES: OnceLock<[PathBuf; 2]> = OnceLock::new();
    FILES.get_or_init(|| {
        [
            sine_file("sync-a", 330.0, 12.0, 0.4, RATE),
            sine_file("sync-b", 440.0, 12.0, 0.4, RATE),
        ]
    })
}

/// Deck A with a grid at 120 BPM and deck B with one at 100 BPM: in sync, deck B plays at
/// 1.2 times its speed.
fn rig() -> Rig {
    let mut rig = Rig::new();
    rig.load(A, files()[0].clone());
    rig.load(B, files()[1].clone());
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    rig.send(Command::DeckGridBpmSet {
        deck: B,
        bpm: 100.0,
    });
    rig
}

/// Lets the processor apply what was sent, without any time passing, and returns the state.
fn state(rig: &mut Rig) -> Snapshot {
    rig.processor.process(&mut [], 2);
    rig.engine.poll();
    rig.engine.snapshot()
}

fn deck_b(rig: &mut Rig) -> DeckSnapshot {
    state(rig).decks[1].clone()
}

fn play(rig: &mut Rig, deck: DeckId) {
    rig.send(Command::DeckPlay { deck });
}

fn sync(rig: &mut Rig, deck: DeckId) {
    rig.send(Command::DeckSyncToggle { deck });
}

/// How far the beat of deck B is ahead of that of deck A, in frames of output, worked out
/// from the play positions and the grids. Not a number when a deck has no beat.
fn offset_frames(rig: &mut Rig) -> f64 {
    let state = state(rig);
    let phase = |deck: &DeckSnapshot| {
        let phase = deck.grid.phase_at(deck.position_seconds);
        phase.unwrap_or(f64::NAN)
    };
    let beats = (phase(&state.decks[1]) - phase(&state.decks[0]) + 0.5).rem_euclid(1.0) - 0.5;
    beats * 60.0 / state.clock.bpm * f64::from(RATE)
}

fn close(left: f64, right: f64, tolerance: f64) -> bool {
    (left - right).abs() <= tolerance
}

#[test]
fn a_synced_deck_plays_at_the_tempo_of_the_tempo_master() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(1.0);
    let before = state(&mut rig);
    assert_eq!(before.clock.master, Some(A));
    assert!(close(before.clock.bpm, 120.0, 1e-9), "{}", before.clock.bpm);
    assert!(before.decks[1].sync && !before.decks[0].sync);
    assert!(close(before.decks[1].speed, 1.2, 1e-6));
    assert_eq!(before.decks[0].speed, 1.0);
    rig.run(1.0);
    let after = state(&mut rig);
    let played = after.decks[1].position_seconds - before.decks[1].position_seconds;
    assert!(close(played, 1.2, 1e-4), "{played}");
}

#[test]
fn in_beat_mode_a_deck_that_starts_late_lands_on_the_beat_and_stays() {
    let mut rig = rig();
    play(&mut rig, A);
    rig.run(0.3);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.1);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames after the start");
    rig.run(3.0);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames three seconds later");
    let phase = deck_b(&mut rig).phase.unwrap();
    assert!(phase.abs() < 1e-3, "{phase}");
}

#[test]
fn in_tempo_mode_the_offset_stays_as_it_is() {
    let mut rig = rig();
    rig.send(Command::GlobalSyncModeSet {
        mode: SyncMode::Tempo,
    });
    play(&mut rig, A);
    rig.run(0.3);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(1.0);
    // Deck A was 0.6 of a beat in when deck B started at the top of its own.
    let first = f64::from(deck_b(&mut rig).phase.unwrap());
    assert!(close(first, 0.4, 0.02), "{first}");
    rig.run(2.0);
    let second = f64::from(deck_b(&mut rig).phase.unwrap());
    assert!(close(first, second, 1e-4), "{first} then {second}");
    assert_eq!(state(&mut rig).clock.sync_mode, SyncMode::Tempo);
}

#[test]
fn a_synced_deck_follows_a_change_of_tempo_of_the_tempo_master() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.5);
    rig.send(Command::DeckTempoSet {
        deck: A,
        ratio: 1.05,
    });
    rig.run(1.5);
    let state_now = state(&mut rig);
    assert!(close(state_now.clock.bpm, 126.0, 1e-4));
    assert!(close(state_now.decks[1].speed, 1.26, 1e-6));
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames");
    // A bend of the tempo master is followed too.
    rig.send(Command::DeckNudge {
        deck: A,
        amount: 0.04,
    });
    rig.run(1.0);
    assert!(close(state(&mut rig).clock.bpm, 126.0 * 1.04, 1e-3));
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames under the bend");
}

#[test]
fn a_seek_of_a_synced_deck_lands_on_the_beat() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.5);
    rig.send(Command::DeckSeek {
        deck: B,
        seconds: 5.13,
    });
    rig.run(0.2);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames");
    // It moved by less than half of one of its beats, which last 0.6 s, from where the
    // seek put it and the 0.2 s at 1.2 times the speed took it.
    let position = deck_b(&mut rig).position_seconds;
    assert!(close(position, 5.13 + 0.24, 0.31), "{position}");
}

#[test]
fn sync_switched_on_moves_a_playing_deck_without_a_click() {
    let mut rig = rig();
    rig.send(Command::MixerCrossfaderSet { position: 1.0 });
    play(&mut rig, A);
    rig.run(0.3);
    play(&mut rig, B);
    rig.run(0.2);
    sync(&mut rig, B);
    let output = channel(&rig.run_stereo(0.3), 2, 0);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames");
    // The tone of deck B at 1.2 times its speed has a slope of 0.028 per sample.
    let step = max_step(&output);
    assert!(step < 0.04, "{step}");
}

#[test]
fn the_tempo_fader_waits_while_the_deck_is_synced_and_the_speed_stays_when_sync_goes_off() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    rig.run(0.1);
    assert!(
        close(deck_b(&mut rig).speed, 1.2, 1e-6),
        "a paused deck too"
    );
    rig.send(Command::DeckTempoSet {
        deck: B,
        ratio: 0.9,
    });
    rig.send(Command::DeckTempoRangeCycle { deck: B });
    rig.run(0.1);
    assert!(close(deck_b(&mut rig).speed, 1.2, 1e-6));
    sync(&mut rig, B);
    rig.run(0.1);
    let deck = deck_b(&mut rig);
    assert!(!deck.sync);
    assert!(close(deck.speed, 1.2, 1e-6), "{}", deck.speed);
    rig.send(Command::DeckTempoSet {
        deck: B,
        ratio: 0.95,
    });
    assert!(close(deck_b(&mut rig).speed, 0.95, 1e-6));
}

#[test]
fn off_then_a_tempo_in_a_row_sets_the_tempo() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    rig.run(0.1);
    sync(&mut rig, B);
    rig.send(Command::DeckTempoSet {
        deck: B,
        ratio: 0.95,
    });
    assert!(close(deck_b(&mut rig).speed, 0.95, 1e-6));
}

#[test]
fn in_auto_the_tempo_master_passes_to_the_synced_deck_and_then_to_the_clock() {
    let mut rig = rig();
    assert_eq!(state(&mut rig).clock.master, None);
    assert!(state(&mut rig).clock.auto);
    rig.send(Command::DeckMasterSet { deck: B });
    assert_eq!(
        state(&mut rig).clock.master,
        None,
        "the choice is automatic"
    );
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.5);
    assert_eq!(state(&mut rig).clock.master, Some(A));
    rig.send(Command::DeckPause { deck: A });
    rig.run(0.5);
    let now = state(&mut rig);
    assert_eq!(now.clock.master, Some(B));
    assert!(close(now.clock.bpm, 120.0, 1e-4), "{}", now.clock.bpm);
    assert!(close(now.decks[1].speed, 1.2, 1e-6));
    rig.send(Command::DeckPause { deck: B });
    rig.run(0.1);
    let now = state(&mut rig);
    assert_eq!(now.clock.master, None);
    assert!(
        close(now.clock.bpm, 120.0, 1e-4),
        "the clock goes on at the tempo it had"
    );
    // A deck held playing by its cue key is not one that plays.
    rig.send(Command::DeckCue {
        deck: A,
        pressed: true,
    });
    rig.send(Command::DeckCue {
        deck: A,
        pressed: true,
    });
    rig.run(0.1);
    assert!(state(&mut rig).decks[0].playing);
    assert_eq!(state(&mut rig).clock.master, None);
    rig.send(Command::DeckCue {
        deck: A,
        pressed: false,
    });
    // The tempo master that loses its track hands over as well.
    play(&mut rig, A);
    rig.run(0.1);
    assert_eq!(state(&mut rig).clock.master, Some(A));
    rig.send(Command::DeckEject { deck: A });
    rig.run(0.1);
    assert_eq!(state(&mut rig).clock.master, None);
}

#[test]
fn without_auto_the_tempo_master_is_the_clock_or_the_deck_that_is_told() {
    let mut rig = rig();
    play(&mut rig, A);
    rig.run(0.1);
    rig.send(Command::ClockModeSet {
        mode: ClockMode::Master,
    });
    let now = state(&mut rig);
    assert_eq!((now.clock.auto, now.clock.master), (false, None));
    rig.send(Command::DeckMasterSet { deck: B });
    rig.run(0.1);
    let now = state(&mut rig);
    assert_eq!(now.clock.master, Some(B), "a paused deck can be it");
    assert!(close(now.clock.bpm, 100.0, 1e-9), "{}", now.clock.bpm);
    // Its own tempo fader moves the clock, and a synced deck with it.
    sync(&mut rig, A);
    rig.send(Command::DeckTempoSet {
        deck: B,
        ratio: 1.1,
    });
    rig.run(0.1);
    let now = state(&mut rig);
    assert!(close(now.clock.bpm, 110.0, 1e-4));
    assert!(close(now.decks[0].speed, 110.0 / 120.0, 1e-6));
    rig.send(Command::ClockTempoSet { bpm: 90.0 });
    assert!(
        close(state(&mut rig).clock.bpm, 110.0, 1e-4),
        "the clock is not the tempo master"
    );
    rig.send(Command::ClockModeSet {
        mode: ClockMode::Auto,
    });
    assert_eq!(
        state(&mut rig).clock.master,
        Some(A),
        "deck A is the one that plays"
    );
}

#[test]
fn the_clock_takes_a_tempo_steps_and_bends_while_it_is_the_tempo_master() {
    let mut rig = rig();
    rig.send(Command::ClockModeSet {
        mode: ClockMode::Master,
    });
    assert!(close(state(&mut rig).clock.bpm, 120.0, 1e-9));
    rig.send(Command::ClockTempoSet { bpm: 110.0 });
    rig.send(Command::ClockTempoStep { amount: 0.5 });
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(1.0);
    let now = state(&mut rig);
    assert!(close(now.clock.bpm, 110.5, 1e-9), "{}", now.clock.bpm);
    assert!(close(now.decks[1].speed, 1.105, 1e-6));
    let phase = now.decks[1].phase.unwrap();
    assert!(phase.abs() < 1e-3, "{phase}: on the beat of the clock");
    rig.send(Command::ClockBend { amount: 0.1 });
    rig.run(0.5);
    let now = state(&mut rig);
    assert!(close(now.clock.bpm, 121.55, 1e-4), "{}", now.clock.bpm);
    assert!(close(now.decks[1].speed, 1.2155, 1e-6));
    assert!(now.decks[1].phase.unwrap().abs() < 2e-3);
    rig.send(Command::ClockBend { amount: 0.0 });
    rig.run(0.1);
    assert!(close(state(&mut rig).clock.bpm, 110.5, 1e-9));
    rig.send(Command::ClockTempoSet { bpm: 400.0 });
    rig.send(Command::ClockTempoStep { amount: 10.0 });
    assert!(close(state(&mut rig).clock.bpm, 400.0, 1e-9));

    for command in [
        Command::ClockTempoSet { bpm: 19.0 },
        Command::ClockTempoSet { bpm: f64::NAN },
        Command::ClockTempoStep { amount: 11.0 },
        Command::ClockBend { amount: 0.6 },
    ] {
        let rejected = rig.engine.dispatch(command.clone());
        assert!(
            matches!(rejected, Err(EngineError::Value(_))),
            "{command:?}"
        );
    }
}

#[test]
fn a_tempo_out_of_reach_leaves_the_deck_at_double_speed() {
    let mut rig = rig();
    rig.send(Command::ClockModeSet {
        mode: ClockMode::Master,
    });
    rig.send(Command::ClockTempoSet { bpm: 300.0 });
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.5);
    let before = deck_b(&mut rig);
    assert_eq!(before.speed, 2.0);
    rig.run(0.5);
    let played = deck_b(&mut rig).position_seconds - before.position_seconds;
    assert!(close(played, 1.0, 1e-4), "{played}: no move to a beat");
}

#[test]
fn a_deck_without_a_tempo_follows_once_it_has_one() {
    let mut rig = Rig::new();
    rig.load(A, files()[0].clone());
    rig.load(B, files()[1].clone());
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.2);
    let deck = deck_b(&mut rig);
    assert!(deck.sync);
    assert_eq!((deck.speed, deck.phase), (1.0, None));
    rig.send(Command::DeckGridBpmSet { deck: B, bpm: 96.0 });
    rig.run(0.2);
    assert!(close(deck_b(&mut rig).speed, 1.25, 1e-6));
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames");
    // An edit of the tempo of the grid is followed at once.
    rig.send(Command::DeckGridBpmSet {
        deck: B,
        bpm: 100.0,
    });
    rig.run(0.2);
    assert!(close(deck_b(&mut rig).speed, 1.2, 1e-6));
}

#[test]
fn a_bend_moves_a_synced_deck_off_the_beat_until_it_ends() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.5);
    rig.send(Command::DeckNudge {
        deck: B,
        amount: 0.1,
    });
    rig.run(0.4);
    // A tenth faster for 0.4 s at 120 BPM is 0.08 of a beat.
    let ahead = f64::from(deck_b(&mut rig).phase.unwrap());
    assert!(close(ahead, 0.08, 0.01), "{ahead}");
    rig.send(Command::DeckNudge {
        deck: B,
        amount: 0.0,
    });
    rig.run(0.2);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames");
}

#[test]
fn a_loop_of_whole_beats_stays_on_the_beat_and_a_shorter_one_is_left_alone() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.5);
    rig.send(Command::DeckLoopToggle { deck: B });
    rig.run(3.0);
    let deck = deck_b(&mut rig);
    let region = deck.looping.region.unwrap();
    assert!((region.start..region.end).contains(&deck.position_seconds));
    let offset = offset_frames(&mut rig);
    assert!(
        offset.abs() < 0.5,
        "{offset} frames in a loop of four beats"
    );

    // Half a beat: every pass puts the deck half a beat off, and nothing moves it out.
    for _ in 0..3 {
        rig.send(Command::DeckLoopSizeStep {
            deck: B,
            direction: SizeStep::Down,
        });
    }
    rig.run(0.75);
    let deck = deck_b(&mut rig);
    let region = deck.looping.region.unwrap();
    assert!(close(region.end - region.start, 0.3, 1e-9));
    assert!((region.start..region.end).contains(&deck.position_seconds));
    rig.send(Command::DeckLoopToggle { deck: B });
    rig.run(0.2);
    assert!(deck_b(&mut rig).looping.region.is_none());
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames after the loop");
}

#[test]
fn a_new_processor_keeps_the_clock_and_the_sync_of_each_deck() {
    let mut rig = rig();
    rig.send(Command::ClockModeSet {
        mode: ClockMode::Master,
    });
    rig.send(Command::GlobalSyncModeSet {
        mode: SyncMode::Tempo,
    });
    rig.send(Command::ClockTempoSet { bpm: 128.0 });
    sync(&mut rig, B);
    rig.run(0.1);
    let (mut engine, mut processor) = Engine::with_controls_of(rate(), &rig.engine);
    processor.process(&mut [], 2);
    engine.poll();
    let carried = engine.snapshot();
    assert!(!carried.clock.auto);
    assert_eq!(carried.clock.sync_mode, SyncMode::Tempo);
    assert_eq!(carried.clock.bpm, 128.0);
    assert_eq!(carried.clock.master, None);
    assert!(carried.decks[1].sync && !carried.decks[0].sync);
}

#[test]
fn a_deck_held_playing_by_its_cue_key_plays_from_the_cue() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    rig.run(0.3);
    rig.send(Command::DeckCue {
        deck: B,
        pressed: true,
    });
    rig.run(0.2);
    // Deck A is 0.6 of a beat in; a move to its beat would be one of 0.36 s.
    let position = deck_b(&mut rig).position_seconds;
    assert!(close(position, 0.24, 0.01), "{position}");
    rig.send(Command::DeckCue {
        deck: B,
        pressed: false,
    });
    rig.run(0.1);
    let deck = deck_b(&mut rig);
    assert_eq!((deck.playing, deck.position_seconds), (false, 0.0));
}

#[test]
fn a_marker_off_the_beat_moves_a_synced_deck_once() {
    let mut rig = rig();
    rig.send(Command::DeckSeek {
        deck: B,
        seconds: 2.17,
    });
    rig.send(Command::DeckGridMarkerSet { deck: B });
    rig.send(Command::DeckSeek {
        deck: B,
        seconds: 1.5,
    });
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.2);
    assert_eq!(deck_b(&mut rig).grid.markers(), &[0.0, 2.17]);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames before the marker");
    // Through the marker in steps of 5 ms: the deck advances 6 ms of its track in each,
    // but for the one move onto the beats of the new run.
    let mut position = deck_b(&mut rig).position_seconds;
    let mut moves = 0;
    for _ in 0..200 {
        rig.run(0.005);
        let now = deck_b(&mut rig).position_seconds;
        moves += usize::from(!close(now - position, 0.006, 0.001));
        position = now;
    }
    assert!(position > 2.17, "{position}");
    assert_eq!(moves, 1);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames after the marker");
}

#[test]
fn a_tempo_master_that_reaches_the_end_of_its_track_hands_over() {
    let mut rig = rig();
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 11.5,
    });
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.2);
    assert_eq!(state(&mut rig).clock.master, Some(A));
    rig.run(0.8);
    let now = state(&mut rig);
    assert!(!now.decks[0].playing);
    assert_eq!(now.clock.master, Some(B));
    assert!(close(now.clock.bpm, 120.0, 1e-4), "{}", now.clock.bpm);
}

#[test]
fn an_edit_of_the_tempo_of_the_grid_of_the_tempo_master_is_followed() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    rig.run(0.3);
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 126.0,
    });
    rig.run(0.5);
    let now = state(&mut rig);
    assert!(close(now.clock.bpm, 126.0, 1e-9), "{}", now.clock.bpm);
    assert!(close(now.decks[1].speed, 1.26, 1e-6));
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames");
}

#[test]
fn a_tempo_master_in_a_loop_off_the_beat_leaves_the_clock_counting() {
    let mut rig = rig();
    play(&mut rig, A);
    sync(&mut rig, B);
    play(&mut rig, B);
    // Long enough for the speed to close what the ramp at the start left.
    rig.run(2.5);
    for _ in 0..3 {
        rig.send(Command::DeckLoopSizeStep {
            deck: A,
            direction: SizeStep::Down,
        });
    }
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run(0.2);
    let before = deck_b(&mut rig);
    rig.run(1.0);
    let now = state(&mut rig);
    assert!(now.decks[0].looping.region.is_some());
    let played = now.decks[1].position_seconds - before.position_seconds;
    assert!(close(played, 1.2, 1e-4), "{played}: deck B was not moved");
    assert!(close(now.clock.bpm, 120.0, 1e-9));
    assert!(now.decks[1].phase.unwrap().abs() < 1e-3);
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.run(0.3);
    let offset = offset_frames(&mut rig);
    assert!(offset.abs() < 0.5, "{offset} frames after the loop");
}
