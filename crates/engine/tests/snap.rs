//! Snap and Quantize. Behaviour notes: `docs/behavior/sync.md`, sections Snap and Quantize.

mod common;

use common::{RATE, Rig, rate, sine_file};
use platter_core::{DeckId, HotcueSlot};
use platter_engine::{Command, DeckSnapshot, Engine};

const A: DeckId = DeckId::A;
/// One beat at 120 BPM, in seconds.
const BEAT: f64 = 0.5;

/// A twelve-second track on deck A with a grid at 120 BPM from its start.
fn rig(name: &str) -> Rig {
    let mut rig = Rig::with_deck_a(sine_file(name, 440.0, 12.0, 0.4, RATE));
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 120.0,
    });
    rig
}

/// Lets the processor apply what was sent and returns deck A. A paused deck gets a single
/// frame, which is when it takes a seek that waits; a playing one gets an empty block.
fn deck(rig: &mut Rig) -> DeckSnapshot {
    let frames = usize::from(!rig.engine.snapshot().decks[0].playing);
    rig.processor.process(&mut [0.0; 2][..frames * 2], 2);
    rig.engine.poll();
    rig.engine.snapshot().decks[0].clone()
}

fn seek(rig: &mut Rig, seconds: f64) {
    rig.send(Command::DeckSeek { deck: A, seconds });
    deck(rig);
}

fn hotcue(rig: &mut Rig, number: usize) {
    for pressed in [true, false] {
        rig.send(Command::DeckHotcueTrigger {
            deck: A,
            slot: HotcueSlot::ALL[number - 1],
            pressed,
        });
    }
}

fn stored(rig: &mut Rig, number: usize) -> f64 {
    let cues = deck(rig).cues;
    // Not a number when the slot is empty, which no comparison accepts.
    cues.get(HotcueSlot::ALL[number - 1])
        .map_or(f64::NAN, |cue| cue.position)
}

fn close(left: f64, right: f64) -> bool {
    (left - right).abs() < 1e-6
}

/// How far into its beat a position is, from minus a half up to a half.
fn offset_in_beat(seconds: f64) -> f64 {
    (seconds / BEAT + 0.5).rem_euclid(1.0) - 0.5
}

#[test]
fn snap_sets_the_floating_cue_on_the_nearest_beat_and_moves_a_paused_deck_there() {
    let mut rig = rig("snap-floating");
    seek(&mut rig, 1.1);
    rig.send(Command::DeckCueSet { deck: A });
    assert!(close(deck(&mut rig).cue_seconds, 1.1), "Snap is off");

    rig.send(Command::GlobalSnapToggle);
    assert!(rig.engine.snapshot().clock.snap);
    rig.send(Command::DeckCueSet { deck: A });
    let now = deck(&mut rig);
    assert!(close(now.cue_seconds, 1.0), "{}", now.cue_seconds);
    assert!(close(now.position_seconds, 1.0), "{}", now.position_seconds);

    // The cue key sets the cue on the beat, and the next press plays from it.
    seek(&mut rig, 1.3);
    let press = |rig: &mut Rig, pressed| rig.send(Command::DeckCue { deck: A, pressed });
    press(&mut rig, true);
    press(&mut rig, false);
    let now = deck(&mut rig);
    assert!(close(now.cue_seconds, 1.5) && close(now.position_seconds, 1.5));
    press(&mut rig, true);
    assert!(deck(&mut rig).playing, "the deck stands on its cue");
}

#[test]
fn snap_puts_hotcues_and_new_loops_on_the_beat() {
    let mut rig = rig("snap-hotcues");
    rig.send(Command::GlobalSnapToggle);
    seek(&mut rig, 1.3);
    hotcue(&mut rig, 1);
    assert!(close(stored(&mut rig, 1), 1.5));
    rig.send(Command::DeckCueSet { deck: A });
    seek(&mut rig, 3.0);
    rig.send(Command::DeckCueStore { deck: A });
    assert!(
        close(stored(&mut rig, 2), 1.5),
        "the floating cue was on the beat"
    );

    seek(&mut rig, 2.2);
    rig.send(Command::DeckLoopToggle { deck: A });
    let region = deck(&mut rig).looping.region.unwrap();
    assert!(
        close(region.start, 2.0) && close(region.end, 4.0),
        "{region:?}"
    );
    rig.send(Command::DeckLoopToggle { deck: A });

    seek(&mut rig, 5.7);
    rig.send(Command::DeckLoopIn { deck: A });
    assert!(close(deck(&mut rig).looping.in_point.unwrap(), 5.5));
    seek(&mut rig, 6.8);
    rig.send(Command::DeckLoopOut { deck: A });
    let region = deck(&mut rig).looping.region.unwrap();
    assert!(
        close(region.start, 5.5) && close(region.end, 7.0),
        "{region:?}"
    );

    // A move keeps its own step.
    rig.send(Command::DeckMoveFineToggle { deck: A });
    rig.send(Command::DeckMoveModeSet {
        deck: A,
        mode: platter_engine::MoveMode::LoopOut,
    });
    rig.send(Command::DeckMove {
        deck: A,
        direction: platter_engine::Direction::Forward,
    });
    let region = deck(&mut rig).looping.region.unwrap();
    assert!(close(region.end, 7.005), "{region:?}");
}

#[test]
fn snap_leaves_a_deck_without_a_tempo_where_it_is() {
    let mut rig = Rig::with_deck_a(sine_file("snap-bare", 440.0, 4.0, 0.4, RATE));
    rig.send(Command::GlobalSnapToggle);
    seek(&mut rig, 1.1);
    rig.send(Command::DeckCueSet { deck: A });
    hotcue(&mut rig, 1);
    let now = deck(&mut rig);
    assert!(close(now.cue_seconds, 1.1) && close(now.position_seconds, 1.1));
    assert!(close(stored(&mut rig, 1), 1.1));
}

#[test]
fn quantize_lands_a_playing_deck_as_far_into_the_beat_as_it_was() {
    let mut rig = rig("quantize-hotcue");
    seek(&mut rig, 1.0);
    hotcue(&mut rig, 1);
    seek(&mut rig, 8.0);
    hotcue(&mut rig, 2);
    seek(&mut rig, 3.0);
    rig.send(Command::DeckPlay { deck: A });

    for (seconds, slot) in [(0.37, 1), (0.21, 2), (0.6, 1)] {
        rig.run(seconds);
        rig.send(Command::GlobalQuantizeToggle);
        let quantize = rig.engine.snapshot().clock.quantize;
        let before = deck(&mut rig).position_seconds;
        hotcue(&mut rig, slot);
        let after = deck(&mut rig).position_seconds;
        let cue = [1.0, 8.0][slot - 1];
        let expected = if quantize {
            cue + offset_in_beat(before) * BEAT
        } else {
            cue
        };
        assert!(
            close(after, expected),
            "{after} from {before}, quantize {quantize}"
        );
    }
}

#[test]
fn quantize_lands_inside_a_stored_loop() {
    let mut rig = rig("quantize-loop");
    seek(&mut rig, 2.0);
    rig.send(Command::DeckLoopToggle { deck: A });
    hotcue(&mut rig, 1);
    rig.send(Command::DeckLoopToggle { deck: A });
    seek(&mut rig, 6.0);
    rig.send(Command::GlobalQuantizeToggle);
    rig.send(Command::DeckPlay { deck: A });
    // 0.4 of a beat before the next one: the deck lands 0.6 of a beat into the loop.
    rig.run(0.3);
    let before = deck(&mut rig).position_seconds;
    hotcue(&mut rig, 1);
    let now = deck(&mut rig);
    let region = now.looping.region.unwrap();
    assert!(close(region.start, 2.0) && close(region.end, 4.0));
    let expected = 2.0 + offset_in_beat(before).rem_euclid(1.0) * BEAT;
    assert!(
        close(now.position_seconds, expected),
        "{}",
        now.position_seconds
    );
}

#[test]
fn quantize_leaves_a_paused_deck_on_the_cue() {
    let mut rig = rig("quantize-paused");
    seek(&mut rig, 1.0);
    hotcue(&mut rig, 1);
    rig.send(Command::GlobalQuantizeToggle);
    seek(&mut rig, 3.2);
    rig.send(Command::DeckHotcueTrigger {
        deck: A,
        slot: HotcueSlot::ALL[0],
        pressed: true,
    });
    assert!(close(deck(&mut rig).position_seconds, 1.0));
}

#[test]
fn quantize_keeps_a_jump_near_the_start_inside_the_track() {
    let mut rig = rig("quantize-start");
    seek(&mut rig, 0.0);
    hotcue(&mut rig, 1);
    seek(&mut rig, 3.0);
    rig.send(Command::GlobalQuantizeToggle);
    rig.send(Command::DeckPlay { deck: A });
    // 0.4 s in: 0.8 of a beat, which is 0.2 of a beat before the next one.
    rig.run(0.4);
    hotcue(&mut rig, 1);
    let position = deck(&mut rig).position_seconds;
    assert!(close(position, 0.4), "{position}");
}

#[test]
fn a_new_processor_keeps_snap_and_quantize() {
    let mut rig = rig("snap-carry");
    rig.send(Command::GlobalSnapToggle);
    rig.send(Command::GlobalQuantizeToggle);
    let (mut engine, _processor) = Engine::with_controls_of(rate(), &rig.engine);
    let clock = engine.snapshot().clock;
    assert!(clock.snap && clock.quantize);
    engine.dispatch(Command::GlobalSnapToggle).unwrap();
    assert!(!engine.snapshot().clock.snap);
}

#[test]
fn a_snapped_out_keeps_a_playing_deck_on_the_beat() {
    // Out pressed 0.2 s before a beat, then 0.1 s after one.
    for (played, expected) in [(1.8, None), (2.1, Some(5.1))] {
        let mut rig = rig(&format!("snap-out-{played}"));
        rig.send(Command::GlobalSnapToggle);
        seek(&mut rig, 5.0);
        rig.send(Command::DeckPlay { deck: A });
        rig.send(Command::DeckLoopIn { deck: A });
        rig.run(played);
        let before = deck(&mut rig).position_seconds;
        rig.send(Command::DeckLoopOut { deck: A });
        let now = deck(&mut rig);
        let region = now.looping.region.unwrap();
        assert!(
            close(region.start, 5.0) && close(region.end, 7.0),
            "{region:?}"
        );
        let landed = expected.map_or(before, |at: f64| at + (before - 5.0 - played));
        assert!(
            (now.position_seconds - landed).abs() < 1e-4,
            "{} from {before}",
            now.position_seconds
        );
    }
}
