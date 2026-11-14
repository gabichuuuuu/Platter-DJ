//! The beatgrid commands of a deck. Behaviour notes: `docs/behavior/beatgrid.md`.

mod common;

use common::{RATE, Rig, sine_file};
use platter_core::{BeatGrid, Bpm, DeckId};
use platter_engine::{BpmScale, Command, EngineError};

const A: DeckId = DeckId::A;

fn rig(name: &str) -> Rig {
    Rig::with_deck_a(sine_file(name, 440.0, 4.0, 0.5, RATE))
}

/// Lets the processor apply what was sent and returns the grid of deck A as reported.
fn grid(rig: &mut Rig) -> BeatGrid {
    rig.run(0.01);
    rig.engine.poll();
    rig.engine.snapshot().decks[0].grid
}

fn position(rig: &mut Rig) -> f64 {
    rig.run(0.01);
    rig.engine.snapshot().decks[0].position_seconds
}

fn seek(rig: &mut Rig, seconds: f64) {
    rig.send(Command::DeckSeek { deck: A, seconds });
}

fn set_bpm(rig: &mut Rig, bpm: f64) {
    rig.send(Command::DeckGridBpmSet { deck: A, bpm });
}

#[test]
fn a_tempo_on_a_new_track_puts_a_marker_at_its_start() {
    let mut rig = rig("grid-start");
    assert_eq!(grid(&mut rig), BeatGrid::default());
    set_bpm(&mut rig, 124.0);
    let grid = grid(&mut rig);
    assert_eq!(grid.bpm(), Some(Bpm::new(124.0).unwrap()));
    assert_eq!(grid.markers(), &[0.0]);
}

#[test]
fn a_marker_goes_where_the_seek_before_it_landed() {
    let mut rig = rig("grid-seek-marker");
    seek(&mut rig, 1.25);
    rig.send(Command::DeckGridMarkerSet { deck: A });
    seek(&mut rig, 2.5);
    rig.send(Command::DeckGridMarkerSet { deck: A });
    assert_eq!(grid(&mut rig).markers(), &[1.25, 2.5]);
}

#[test]
fn a_marker_after_a_seek_past_the_end_lands_on_the_end() {
    let mut rig = rig("grid-seek-past-end");
    seek(&mut rig, 10.0);
    rig.send(Command::DeckGridMarkerSet { deck: A });
    assert_eq!(grid(&mut rig).markers(), &[4.0]);
}

#[test]
fn the_marker_a_repeated_tempo_adds_reaches_the_snapshot() {
    let mut rig = rig("grid-same-tempo");
    set_bpm(&mut rig, 128.0);
    rig.send(Command::DeckGridMarkerDelete { deck: A });
    assert_eq!(grid(&mut rig).markers(), &[] as &[f64]);
    set_bpm(&mut rig, 128.0);
    assert_eq!(grid(&mut rig).markers(), &[0.0]);
}

#[test]
fn a_marker_set_while_playing_lands_at_the_play_position() {
    let mut rig = rig("grid-playing-marker");
    rig.send(Command::DeckPlay { deck: A });
    rig.run(1.0);
    rig.send(Command::DeckGridMarkerSet { deck: A });
    let markers = grid(&mut rig).markers().to_vec();
    assert_eq!(markers.len(), 1);
    // One second of playback; the 5 ms fade-in starts the count at once.
    assert!((markers[0] - 1.0).abs() < 0.01, "{markers:?}");
}

#[test]
fn previous_and_next_jump_between_markers() {
    let mut rig = rig("grid-jumps");
    for at in [0.5, 1.5, 3.0] {
        seek(&mut rig, at);
        rig.send(Command::DeckGridMarkerSet { deck: A });
    }
    seek(&mut rig, 2.0);
    rig.send(Command::DeckGridMarkerNext { deck: A });
    assert!((position(&mut rig) - 3.0).abs() < 1e-9);
    rig.send(Command::DeckGridMarkerPrevious { deck: A });
    rig.send(Command::DeckGridMarkerPrevious { deck: A });
    assert!((position(&mut rig) - 0.5).abs() < 1e-9);
    // Before the first marker there is nowhere to go back to.
    rig.send(Command::DeckGridMarkerPrevious { deck: A });
    assert!((position(&mut rig) - 0.5).abs() < 1e-9);
}

#[test]
fn move_and_delete_act_on_the_marker_on_or_before_the_play_position() {
    let mut rig = rig("grid-move-delete");
    for at in [1.0, 2.0] {
        seek(&mut rig, at);
        rig.send(Command::DeckGridMarkerSet { deck: A });
    }
    seek(&mut rig, 1.8);
    rig.send(Command::DeckGridMarkerMove {
        deck: A,
        seconds: 0.25,
    });
    assert_eq!(grid(&mut rig).markers(), &[1.25, 2.0]);
    seek(&mut rig, 4.0);
    for _ in 0..3 {
        rig.send(Command::DeckGridMarkerMove {
            deck: A,
            seconds: 1.0,
        });
    }
    // The last marker stops at the end of the four-second track.
    assert_eq!(grid(&mut rig).markers(), &[1.25, 4.0]);
    rig.send(Command::DeckGridMarkerDelete { deck: A });
    assert_eq!(grid(&mut rig).markers(), &[1.25]);
    seek(&mut rig, 0.5);
    rig.send(Command::DeckGridMarkerDelete { deck: A });
    assert_eq!(grid(&mut rig).markers(), &[1.25]);
}

#[test]
fn double_halve_and_step_change_the_tempo_inside_its_range() {
    let mut rig = rig("grid-scale");
    rig.send(Command::DeckGridBpmScale {
        deck: A,
        factor: BpmScale::Double,
    });
    assert_eq!(grid(&mut rig).bpm(), None, "no tempo to double yet");
    set_bpm(&mut rig, 87.0);
    rig.send(Command::DeckGridBpmScale {
        deck: A,
        factor: BpmScale::Double,
    });
    rig.send(Command::DeckGridBpmStep {
        deck: A,
        amount: -0.5,
    });
    assert_eq!(grid(&mut rig).bpm(), Some(Bpm::new(173.5).unwrap()));
    set_bpm(&mut rig, 300.0);
    rig.send(Command::DeckGridBpmScale {
        deck: A,
        factor: BpmScale::Double,
    });
    assert_eq!(grid(&mut rig).bpm(), Some(Bpm::new(300.0).unwrap()));
}

#[test]
fn a_locked_grid_keeps_everything_but_the_jumps() {
    let mut rig = rig("grid-lock");
    set_bpm(&mut rig, 128.0);
    seek(&mut rig, 2.0);
    rig.send(Command::DeckGridMarkerSet { deck: A });
    rig.send(Command::DeckGridLockToggle { deck: A });
    let locked = grid(&mut rig);
    assert!(locked.is_locked());
    set_bpm(&mut rig, 90.0);
    rig.send(Command::DeckGridMarkerDelete { deck: A });
    rig.send(Command::DeckGridMarkerMove {
        deck: A,
        seconds: 0.5,
    });
    assert_eq!(grid(&mut rig), locked);
    rig.send(Command::DeckGridMarkerPrevious { deck: A });
    assert!(position(&mut rig).abs() < 1e-9);
    rig.send(Command::DeckGridLockToggle { deck: A });
    set_bpm(&mut rig, 90.0);
    assert_eq!(grid(&mut rig).bpm(), Some(Bpm::new(90.0).unwrap()));
}

#[test]
fn a_new_track_starts_with_an_empty_grid_and_edits_after_the_load_are_its_own() {
    let mut rig = rig("grid-swap-old");
    set_bpm(&mut rig, 128.0);
    rig.send(Command::DeckGridLockToggle { deck: A });
    assert!(grid(&mut rig).is_locked());

    rig.load(A, sine_file("grid-swap-new", 220.0, 2.0, 0.5, 44_100));
    // Sent before the processor has taken the new track: it belongs to the new one.
    set_bpm(&mut rig, 100.0);
    let grid = grid(&mut rig);
    assert!(!grid.is_locked());
    assert_eq!(grid.bpm(), Some(Bpm::new(100.0).unwrap()));
    assert_eq!(grid.markers(), &[0.0]);
}

#[test]
fn an_empty_deck_has_no_grid_to_edit() {
    let mut rig = Rig::new();
    set_bpm(&mut rig, 120.0);
    rig.send(Command::DeckGridMarkerSet { deck: A });
    rig.send(Command::DeckGridLockToggle { deck: A });
    assert_eq!(grid(&mut rig), BeatGrid::default());
}

#[test]
fn values_out_of_range_are_rejected() {
    let mut rig = rig("grid-reject");
    let rejected = [
        Command::DeckGridBpmSet { deck: A, bpm: 19.0 },
        Command::DeckGridBpmSet {
            deck: A,
            bpm: f64::NAN,
        },
        Command::DeckGridBpmStep {
            deck: A,
            amount: 10.5,
        },
        Command::DeckGridMarkerMove {
            deck: A,
            seconds: -1.5,
        },
        Command::DeckGridMarkerMove {
            deck: A,
            seconds: f64::INFINITY,
        },
    ];
    for command in rejected {
        assert!(
            matches!(rig.engine.dispatch(command), Err(EngineError::Value(_))),
            "accepted"
        );
    }
    assert_eq!(grid(&mut rig), BeatGrid::default());
}

#[test]
fn four_taps_at_once_set_no_tempo() {
    // Taps are timed when they arrive; four in a row with no pause are far above 400 BPM.
    let mut rig = rig("grid-tap");
    for _ in 0..4 {
        rig.send(Command::DeckGridTap { deck: A });
    }
    assert_eq!(grid(&mut rig).bpm(), None);
}
