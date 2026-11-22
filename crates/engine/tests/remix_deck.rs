//! The remix deck: cells that start on the beats of the master clock, loop or play once, give
//! way to another cell of their slot and follow the tempo of the clock.
//! Behaviour note: `docs/behavior/remix-deck.md`.

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{RATE, Rig, rate};
use platter_core::{Bpm, DeckId, SampleRate};
use platter_engine::command::{Args, Value};
use platter_engine::{
    AnalysedTempo, Analyser, CellId, CellMode, Command, DeckKind, Engine, Event, TrackAnalysis,
};
use platter_testkit::files::wav_pcm16;
use platter_testkit::measure::sine_fit;
use platter_testkit::signal::{stereo, to_i16};

const A: DeckId = DeckId::A;
/// Frames of one beat of the clock, which starts at 120 BPM.
const BEAT: usize = RATE as usize / 2;

/// Finds no tempo, so a cell has the tempo the test gives it.
struct Nothing;

impl Analyser for Nothing {
    fn analyse(&self, _: &[f32], _: SampleRate) -> TrackAnalysis {
        TrackAnalysis {
            tempo: None,
            key: None,
            loudness: None,
        }
    }
}

/// Finds 125 BPM in every track, so a track deck can be the tempo master.
struct Steady;

impl Analyser for Steady {
    fn analyse(&self, _: &[f32], _: SampleRate) -> TrackAnalysis {
        TrackAnalysis {
            tempo: Some(AnalysedTempo {
                bpm: Bpm::new(125.0).unwrap(),
                first_beat: 0.0,
            }),
            key: None,
            loudness: None,
        }
    }
}

/// A rig with deck A a remix deck alone on the master.
fn remix_rig() -> Rig {
    let (mut engine, processor) = Engine::with_analyser(rate(), Arc::new(Nothing));
    engine.dispatch(Command::MixerAutogainToggle).unwrap();
    let mut rig = Rig { engine, processor };
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::DeckTypeSet {
        deck: A,
        kind: DeckKind::Remix,
    });
    rig
}

/// Writes `mono` on both sides of a WAV file at the rate of the engine.
fn sample_file(name: &str, mono: &[f32]) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("remix-{name}.wav"));
    std::fs::write(&path, wav_pcm16(RATE, 2, &to_i16(&stereo(mono)))).unwrap();
    path
}

fn sine(hz: f64, seconds: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(RATE)) as usize;
    (0..frames)
        .map(|frame| {
            (0.5 * (std::f64::consts::TAU * hz * frame as f64 / f64::from(RATE)).sin()) as f32
        })
        .collect()
}

/// A click on the first frame, then silence, `seconds` long.
fn click(seconds: f64) -> Vec<f32> {
    let mut mono = vec![0.0; (seconds * f64::from(RATE)) as usize];
    mono[0] = 0.5;
    mono
}

fn cell(slot: usize, cell: usize) -> CellId {
    CellId::new(slot, cell).unwrap()
}

fn load(rig: &mut Rig, at: CellId, path: PathBuf) {
    rig.send(Command::RemixCellLoad { deck: A, at, path });
    let events = rig.engine.wait_for_loads();
    assert!(events.is_empty(), "{events:?}");
}

fn trigger(rig: &mut Rig, at: CellId) {
    rig.send(Command::RemixCellTrigger { deck: A, at });
}

/// The first frame of the left side louder than `level`.
fn onset(left: &[f32], level: f32) -> Option<usize> {
    left.iter().position(|sample| sample.abs() > level)
}

/// The frames of the left side louder than `level`, each counted once per group of loud
/// frames that are close together.
fn onsets(left: &[f32], level: f32) -> Vec<usize> {
    let mut found: Vec<usize> = Vec::new();
    for (frame, sample) in left.iter().enumerate() {
        if sample.abs() > level && found.last().is_none_or(|last| frame - last > BEAT / 4) {
            found.push(frame);
        }
    }
    found
}

fn amplitude(left: &[f32], hz: f64) -> f64 {
    sine_fit(left, hz, f64::from(RATE)).amplitude
}

#[test]
fn a_cell_starts_on_the_next_beat_of_the_clock() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("start", &click(0.25)));
    // A tenth of a second into the first beat.
    let before = rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let after = rig.run(1.0);
    assert_eq!(onset(&before, 0.01), None);
    let heard = onset(&after, 0.1).unwrap() + before.len();
    assert!(
        // The clock places its beat on a frame, either side of the exact time.
        (BEAT - 1..BEAT + 8).contains(&heard),
        "the cell starts at frame {heard}, the beat is at {BEAT}"
    );
}

#[test]
fn a_quantize_of_four_beats_waits_for_the_fourth_beat() {
    let mut rig = remix_rig();
    load(&mut rig, cell(1, 0), sample_file("four", &click(0.25)));
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: Some(4),
    });
    let before = rig.run(0.6);
    trigger(&mut rig, cell(1, 0));
    // The processor takes the command with its next block.
    let first = rig.run(0.01);
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.decks[0].remix.waiting[1], Some(0));
    assert_eq!(snapshot.decks[0].remix.playing[1], None);
    let after = [first, rig.run(2.0)].concat();
    let heard = onset(&after, 0.1).unwrap() + before.len();
    assert!(
        (4 * BEAT - 1..4 * BEAT + 8).contains(&heard),
        "the cell starts at frame {heard}, the fourth beat is at {}",
        4 * BEAT
    );
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.decks[0].remix.playing[1], Some(0));
}

#[test]
fn without_quantize_a_cell_starts_at_once() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("at-once", &click(0.25)));
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: None,
    });
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let after = rig.run(0.1);
    assert!(onset(&after, 0.1).unwrap() < 8);
}

#[test]
fn a_loop_repeats_at_its_length_and_follows_the_tempo_of_the_clock() {
    let mut rig = remix_rig();
    // One beat at 120 BPM.
    load(&mut rig, cell(0, 0), sample_file("loop", &click(0.5)));
    rig.send(Command::RemixCellBpmSet {
        deck: A,
        at: cell(0, 0),
        bpm: Bpm::new(120.0).unwrap(),
    });
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let at_120 = onsets(&rig.run(3.0), 0.1);
    let steps: Vec<usize> = at_120.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(steps.len() >= 4, "{at_120:?}");
    assert!(
        steps.iter().all(|step| step.abs_diff(BEAT) <= 2),
        "{steps:?}"
    );
    // At 60 BPM the beat lasts twice as long, and so does the loop.
    rig.send(Command::ClockTempoSet { bpm: 60.0 });
    rig.run(1.5);
    let at_60 = onsets(&rig.run(5.0), 0.05);
    let steps: Vec<usize> = at_60.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(steps.len() >= 3, "{at_60:?}");
    assert!(
        steps.iter().all(|step| step.abs_diff(2 * BEAT) <= 4),
        "{steps:?}"
    );
}

#[test]
fn a_cell_without_a_tempo_plays_at_its_own_speed() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("own-speed", &click(0.3)));
    rig.send(Command::ClockTempoSet { bpm: 90.0 });
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let heard = onsets(&rig.run(2.0), 0.1);
    let steps: Vec<usize> = heard.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let length = (0.3 * f64::from(RATE)) as usize;
    assert!(!steps.is_empty(), "{heard:?}");
    assert!(
        steps.iter().all(|step| step.abs_diff(length) <= 2),
        "{steps:?}"
    );
}

#[test]
fn a_one_shot_plays_once() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("once", &click(0.25)));
    rig.send(Command::RemixCellModeSet {
        deck: A,
        at: cell(0, 0),
        mode: CellMode::OneShot,
    });
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    assert_eq!(onsets(&rig.run(2.0), 0.1).len(), 1);
    assert_eq!(rig.engine.snapshot().decks[0].remix.playing[0], None);
}

#[test]
fn a_second_cell_of_the_slot_stops_the_first_one() {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(2, 0),
        sample_file("first", &sine(440.0, 1.0)),
    );
    load(
        &mut rig,
        cell(2, 1),
        sample_file("second", &sine(1_000.0, 1.0)),
    );
    rig.run(0.1);
    trigger(&mut rig, cell(2, 0));
    let first = rig.run(1.0);
    let tail = &first[first.len() - 4_800..];
    assert!(amplitude(tail, 440.0) > 0.3);
    trigger(&mut rig, cell(2, 1));
    let second = rig.run(1.5);
    let tail = &second[second.len() - 4_800..];
    assert!(amplitude(tail, 440.0) < 0.01);
    assert!(amplitude(tail, 1_000.0) > 0.3);
}

#[test]
fn the_cell_that_played_stops_when_another_is_triggered_not_on_the_boundary() {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(2, 0),
        sample_file("at-once-first", &sine(440.0, 1.0)),
    );
    load(
        &mut rig,
        cell(2, 1),
        sample_file("at-once-second", &sine(1_000.0, 1.0)),
    );
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: Some(4),
    });
    trigger(&mut rig, cell(2, 0));
    // The first cell starts on beat 4, at 2 s; the second is triggered at 2.6 s and waits
    // for beat 8, at 4 s.
    rig.run(2.6);
    trigger(&mut rig, cell(2, 1));
    let waiting = rig.run(1.3);
    let quiet = &waiting[4_800..waiting.len() - 4_800];
    assert!(
        quiet.iter().all(|sample| sample.abs() < 1e-4),
        "the first cell plays on while the second waits"
    );
    let started = rig.run(0.5);
    assert!(amplitude(&started[started.len() - 4_800..], 1_000.0) > 0.3);
}

#[test]
fn a_remix_deck_ignores_the_transport_and_is_never_the_tempo_master() {
    let (mut engine, processor) = Engine::with_analyser(rate(), Arc::new(Steady));
    engine.dispatch(Command::MixerAutogainToggle).unwrap();
    let mut rig = Rig { engine, processor };
    rig.load(A, common::sine_file("remix-master", 440.0, 10.0, 0.5, RATE));
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.2);
    assert_eq!(rig.engine.snapshot().clock.master, Some(A));
    rig.send(Command::DeckTypeSet {
        deck: A,
        kind: DeckKind::Remix,
    });
    rig.run(0.2);
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.clock.master, None);
    assert!(!snapshot.decks[0].playing);
    rig.send(Command::DeckPlay { deck: A });
    rig.run(0.1);
    assert!(!rig.engine.snapshot().decks[0].playing);
}

#[test]
fn loads_and_clears_while_cells_play_give_every_sample_back() {
    // More swaps than the engine lets wait: a sample that never came back would make it
    // refuse the next load as busy.
    let mut rig = remix_rig();
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: None,
    });
    let first = sample_file("cycle-a", &sine(440.0, 0.2));
    let second = sample_file("cycle-b", &sine(660.0, 0.2));
    // A sample replaced while it plays is the one whose return waits for a fade: one per
    // cycle, and more cycles than the 256 swaps the engine lets wait.
    for _ in 0..270 {
        load(&mut rig, cell(0, 0), first.clone());
        trigger(&mut rig, cell(0, 0));
        rig.run(0.01);
        // Replaced while it plays: the sample goes back once its fade ends.
        load(&mut rig, cell(0, 0), second.clone());
        rig.run(0.01);
        rig.send(Command::RemixCellClear {
            deck: A,
            at: cell(0, 0),
        });
        rig.run(0.01);
    }
}

#[test]
fn a_cell_in_gate_mode_stops_when_its_key_is_released_and_a_latched_one_plays_on() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("gate", &sine(440.0, 1.0)));
    load(
        &mut rig,
        cell(1, 0),
        sample_file("latch", &sine(1_000.0, 1.0)),
    );
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: None,
    });
    rig.send(Command::RemixCellTriggerModeSet {
        deck: A,
        at: cell(0, 0),
        gate: true,
    });
    trigger(&mut rig, cell(0, 0));
    trigger(&mut rig, cell(1, 0));
    let held = rig.run(0.3);
    assert!(amplitude(&held[held.len() - 4_800..], 440.0) > 0.3);
    for at in [cell(0, 0), cell(1, 0)] {
        rig.send(Command::RemixCellRelease { deck: A, at });
    }
    let released = rig.run(0.3);
    let tail = &released[released.len() - 4_800..];
    assert!(amplitude(tail, 440.0) < 1e-3);
    assert!(amplitude(tail, 1_000.0) > 0.3);
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.decks[0].remix.playing, [None, Some(0), None, None]);
    assert!(snapshot.decks[0].remix.cells[0][0].as_ref().unwrap().gate);
}

#[test]
fn a_reversed_cell_plays_its_sample_from_the_end() {
    let mut rig = remix_rig();
    // The click is on the first frame: played backwards it comes last.
    load(&mut rig, cell(0, 0), sample_file("reverse", &click(0.3)));
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: None,
    });
    rig.send(Command::RemixCellModeSet {
        deck: A,
        at: cell(0, 0),
        mode: CellMode::OneShot,
    });
    rig.send(Command::RemixCellReverseToggle {
        deck: A,
        at: cell(0, 0),
    });
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let heard = onsets(&rig.run(1.0), 0.1);
    let length = (0.3 * f64::from(RATE)) as usize;
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert!(heard[0].abs_diff(length) <= 300, "{heard:?}");
}

/// The frame of the first click of a two-beat loop triggered at 0.8 s, on beat 2, after a
/// cell that started on beat 1 in the same slot.
fn punched_onset(punch: bool) -> usize {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(0, 0),
        sample_file("punch-first", &sine(440.0, 1.0)),
    );
    load(
        &mut rig,
        cell(0, 1),
        sample_file("punch-second", &click(1.0)),
    );
    rig.send(Command::RemixCellBpmSet {
        deck: A,
        at: cell(0, 1),
        bpm: Bpm::new(120.0).unwrap(),
    });
    if punch {
        rig.send(Command::RemixSlotPunchToggle { deck: A, slot: 0 });
    }
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let mut out = rig.run(0.7);
    trigger(&mut rig, cell(0, 1));
    out.extend(rig.run(2.0));
    // The first cell is silent from the trigger on: the clicks are the second one's.
    let start = (0.75 * f64::from(RATE)) as usize;
    onset(&out[start..], 0.1).unwrap() + start + (0.1 * f64::from(RATE)) as usize
}

#[test]
fn in_punch_mode_a_new_loop_starts_where_its_run_had_reached() {
    // Without punch the loop starts at its beginning on beat 2; with it, one beat into
    // itself, so its click comes on beat 3.
    let plain = punched_onset(false);
    let punched = punched_onset(true);
    assert!(plain.abs_diff(2 * BEAT) <= 8, "{plain}");
    assert!(punched.abs_diff(3 * BEAT) <= 8, "{punched}");
}

/// A rig with a one-beat loop of 440 Hz in cell 1 of slot 1 and a four-beat loop with a click
/// on its first frame in cell 2, both at 120 BPM, the slot punched.
fn punch_rig(name: &str) -> Rig {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(0, 0),
        sample_file(&format!("{name}-short"), &sine(440.0, 0.5)),
    );
    load(
        &mut rig,
        cell(0, 1),
        sample_file(&format!("{name}-long"), &click(2.0)),
    );
    for at in [cell(0, 0), cell(0, 1)] {
        rig.send(Command::RemixCellBpmSet {
            deck: A,
            at,
            bpm: Bpm::new(120.0).unwrap(),
        });
    }
    rig.send(Command::RemixSlotPunchToggle { deck: A, slot: 0 });
    rig
}

#[test]
fn a_punched_loop_counts_from_the_start_of_the_run_not_from_where_the_loop_before_wrapped() {
    // The short loop starts on beat 1 and has gone round twice when the long one takes over
    // on beat 4: three beats into the run, so the click of the long one comes on beat 5.
    let mut rig = punch_rig("run");
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let mut out = rig.run(1.7);
    trigger(&mut rig, cell(0, 1));
    out.extend(rig.run(1.5));
    let start = (1.85 * f64::from(RATE)) as usize;
    let heard = onset(&out[start..], 0.1).unwrap() + start + (0.1 * f64::from(RATE)) as usize;
    assert!(heard.abs_diff(5 * BEAT) <= 8, "{heard}");
}

#[test]
fn a_stop_ends_the_run_of_a_punched_slot() {
    let mut rig = punch_rig("stop-run");
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    rig.run(0.7);
    // The long cell waits for beat 2 and is stopped before: the slot is silent.
    trigger(&mut rig, cell(0, 1));
    rig.run(0.1);
    rig.send(Command::RemixSlotStop { deck: A, slot: 0 });
    rig.run(0.6);
    // A new run on beat 4: the click comes on it, not three beats into the sample.
    trigger(&mut rig, cell(0, 1));
    let out = rig.run(1.0);
    let heard = onset(&out, 0.1).unwrap() + (1.5 * f64::from(RATE)) as usize;
    assert!(heard.abs_diff(4 * BEAT) <= 8, "{heard}");
}

#[test]
fn a_capture_copies_the_part_of_the_track_from_where_the_deck_is() {
    let (mut engine, processor) = Engine::with_analyser(rate(), Arc::new(Steady));
    engine.dispatch(Command::MixerAutogainToggle).unwrap();
    let mut rig = Rig { engine, processor };
    // A click at 1.5 s in ten seconds of silence; the deck stands at 1 s.
    let mut track = vec![0.0; 10 * RATE as usize];
    track[(1.5 * f64::from(RATE)) as usize] = 0.5;
    rig.load(DeckId::B, sample_file("capture-click", &track));
    rig.send(Command::DeckSeek {
        deck: DeckId::B,
        seconds: 1.0,
    });
    rig.run(0.1);
    rig.send(Command::RemixCellCapture {
        deck: A,
        at: cell(0, 0),
        from: DeckId::B,
    });
    rig.engine.wait_for_loads();
    rig.send(Command::RemixCellBpmClear {
        deck: A,
        at: cell(0, 0),
    });
    rig.send(Command::RemixCellModeSet {
        deck: A,
        at: cell(0, 0),
        mode: CellMode::OneShot,
    });
    rig.send(Command::RemixQuantizeSet {
        deck: A,
        beats: None,
    });
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::DeckTypeSet {
        deck: A,
        kind: DeckKind::Remix,
    });
    rig.run(0.1);
    trigger(&mut rig, cell(0, 0));
    let heard = onset(&rig.run(1.5), 0.1).unwrap();
    let half = (0.5 * f64::from(RATE)) as usize;
    assert!(heard.abs_diff(half) <= 300, "{heard}");
}

#[test]
fn the_loop_size_of_a_deck_is_captured_into_a_cell_with_its_tempo() {
    let (mut engine, processor) = Engine::with_analyser(rate(), Arc::new(Steady));
    engine.dispatch(Command::MixerAutogainToggle).unwrap();
    let mut rig = Rig { engine, processor };
    assert!(matches!(
        rig.engine.dispatch(Command::RemixCellCapture {
            deck: A,
            at: cell(0, 0),
            from: DeckId::B,
        }),
        Err(platter_engine::EngineError::NothingToCapture)
    ));
    rig.load(
        DeckId::B,
        common::sine_file("remix-capture", 440.0, 10.0, 0.5, RATE),
    );
    // The analysis gives the grid its tempo.
    rig.run(0.1);
    rig.engine.wait_for_loads();
    rig.run(0.1);
    rig.send(Command::RemixCellCapture {
        deck: A,
        at: cell(2, 3),
        from: DeckId::B,
    });
    let events = rig.engine.wait_for_loads();
    assert!(events.is_empty(), "{events:?}");
    let info = rig.engine.snapshot().decks[0].remix.cells[2][3]
        .clone()
        .unwrap();
    // Four beats at 125 BPM.
    assert!((info.seconds - 1.92).abs() < 1e-3, "{}", info.seconds);
    assert_eq!(info.bpm, Some(Bpm::new(125.0).unwrap()));
    assert_eq!(&*info.name, "remix-capture");
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::DeckTypeSet {
        deck: A,
        kind: DeckKind::Remix,
    });
    trigger(&mut rig, cell(2, 3));
    let out = rig.run(1.5);
    // The cell follows the tempo of the clock from its own, and its pitch with it.
    let clock = rig.engine.snapshot().clock.bpm;
    let hz = 440.0 * clock / 125.0;
    assert!(amplitude(&out[out.len() - 4_800..], hz) > 0.3, "at {hz} Hz");
}

#[test]
fn the_loop_that_is_on_is_captured_whatever_its_length() {
    let (mut engine, processor) = Engine::with_analyser(rate(), Arc::new(Steady));
    engine.dispatch(Command::MixerAutogainToggle).unwrap();
    let mut rig = Rig { engine, processor };
    rig.load(
        DeckId::B,
        common::sine_file("remix-capture-loop", 440.0, 10.0, 0.5, RATE),
    );
    rig.send(Command::DeckLoopIn { deck: DeckId::B });
    rig.send(Command::DeckPlay { deck: DeckId::B });
    rig.run(1.0);
    rig.send(Command::DeckLoopOut { deck: DeckId::B });
    rig.run(0.1);
    rig.engine.poll();
    let region = rig.engine.snapshot().decks[1].looping.region.unwrap();
    rig.send(Command::RemixCellCapture {
        deck: A,
        at: cell(0, 0),
        from: DeckId::B,
    });
    rig.engine.wait_for_loads();
    let info = rig.engine.snapshot().decks[0].remix.cells[0][0]
        .clone()
        .unwrap();
    assert!((info.seconds - region.seconds()).abs() < 1e-3);
    assert!((info.seconds - 1.92).abs() > 0.1, "{}", info.seconds);
}

#[test]
fn cells_of_different_slots_play_together() {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(0, 0),
        sample_file("together-a", &sine(440.0, 1.0)),
    );
    load(
        &mut rig,
        cell(3, 5),
        sample_file("together-b", &sine(1_000.0, 1.0)),
    );
    trigger(&mut rig, cell(0, 0));
    trigger(&mut rig, cell(3, 5));
    let out = rig.run(1.5);
    let tail = &out[out.len() - 4_800..];
    assert!(amplitude(tail, 440.0) > 0.3);
    assert!(amplitude(tail, 1_000.0) > 0.3);
}

#[test]
fn a_stop_waits_for_the_boundary_and_fades() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("stop", &sine(440.0, 1.0)));
    trigger(&mut rig, cell(0, 0));
    rig.run(1.1);
    rig.send(Command::RemixSlotStop { deck: A, slot: 0 });
    // Until the next beat, at 1.5 s, the cell plays on.
    let out = rig.run(1.0);
    let before = &out[..9_600];
    let after = &out[out.len() - 9_600..];
    assert!(amplitude(before, 440.0) > 0.3);
    assert!(after.iter().all(|sample| sample.abs() < 1e-4));
    let snapshot = rig.engine.snapshot();
    assert_eq!(snapshot.decks[0].remix.playing[0], None);
}

#[test]
fn volume_and_mute_of_a_slot_silence_it() {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(0, 0),
        sample_file("volume", &sine(440.0, 1.0)),
    );
    trigger(&mut rig, cell(0, 0));
    rig.run(0.6);
    rig.send(Command::RemixSlotVolumeSet {
        deck: A,
        slot: 0,
        level: 0.5,
    });
    let half = rig.run(0.5);
    assert!((amplitude(&half[half.len() - 4_800..], 440.0) - 0.25).abs() < 0.02);
    rig.send(Command::RemixSlotMuteToggle { deck: A, slot: 0 });
    let muted = rig.run(0.2);
    assert!(
        muted[muted.len() - 4_800..]
            .iter()
            .all(|sample| sample.abs() < 1e-4)
    );
    assert!(rig.engine.snapshot().decks[0].remix.muted[0]);
}

#[test]
fn the_gain_of_a_cell_comes_before_the_volume() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("gain", &sine(440.0, 1.0)));
    rig.send(Command::RemixCellGainSet {
        deck: A,
        at: cell(0, 0),
        db: -6.0,
    });
    trigger(&mut rig, cell(0, 0));
    let out = rig.run(1.0);
    let expected = 0.5 * 10f64.powf(-6.0 / 20.0);
    assert!((amplitude(&out[out.len() - 4_800..], 440.0) - expected).abs() < 0.02);
}

#[test]
fn a_track_deck_does_not_play_its_grid() {
    let mut rig = remix_rig();
    load(
        &mut rig,
        cell(0, 0),
        sample_file("track-deck", &sine(440.0, 1.0)),
    );
    trigger(&mut rig, cell(0, 0));
    rig.run(0.6);
    rig.send(Command::DeckTypeSet {
        deck: A,
        kind: DeckKind::Track,
    });
    let out = rig.run(0.5);
    assert!(
        out[out.len() - 4_800..]
            .iter()
            .all(|sample| sample.abs() < 1e-4)
    );
    // The grid ran on unheard: back as a remix deck, the cell plays.
    rig.send(Command::DeckTypeSet {
        deck: A,
        kind: DeckKind::Remix,
    });
    let out = rig.run(0.2);
    assert!(amplitude(&out[out.len() - 4_800..], 440.0) > 0.3);
}

#[test]
fn an_empty_cell_does_nothing_and_a_cleared_cell_stops() {
    let mut rig = remix_rig();
    trigger(&mut rig, cell(0, 3));
    rig.run(1.0);
    assert_eq!(rig.engine.snapshot().decks[0].remix.playing[0], None);
    load(
        &mut rig,
        cell(0, 0),
        sample_file("clear", &sine(440.0, 1.0)),
    );
    trigger(&mut rig, cell(0, 0));
    rig.run(0.6);
    rig.send(Command::RemixCellClear {
        deck: A,
        at: cell(0, 0),
    });
    let out = rig.run(0.2);
    assert!(
        out[out.len() - 4_800..]
            .iter()
            .all(|sample| sample.abs() < 1e-4)
    );
    let snapshot = rig.engine.snapshot();
    assert!(snapshot.decks[0].remix.cells[0][0].is_none());
    assert_eq!(snapshot.decks[0].remix.playing[0], None);
}

#[test]
fn values_outside_their_range_are_refused() {
    let mut rig = remix_rig();
    load(&mut rig, cell(0, 0), sample_file("range", &click(0.5)));
    let gain = |db| Command::RemixCellGainSet {
        deck: A,
        at: cell(0, 0),
        db,
    };
    assert!(rig.engine.dispatch(gain(13.0)).is_err());
    assert!(rig.engine.dispatch(gain(f64::NAN)).is_err());
    let volume = |level| Command::RemixSlotVolumeSet {
        deck: A,
        slot: 0,
        level,
    };
    assert!(rig.engine.dispatch(volume(1.5)).is_err());
    assert!(rig.engine.dispatch(volume(-0.1)).is_err());
}

#[test]
fn the_snapshot_names_a_loaded_cell() {
    let mut rig = remix_rig();
    load(&mut rig, cell(1, 2), sample_file("named", &click(0.5)));
    let snapshot = rig.engine.snapshot();
    let info = snapshot.decks[0].remix.cells[1][2].clone().unwrap();
    assert_eq!(&*info.name, "remix-named");
    assert_eq!(info.mode, CellMode::Loop);
    assert_eq!(info.bpm, None);
    assert!((info.seconds - 0.5).abs() < 1e-3);
}

#[test]
fn a_sample_longer_than_a_minute_is_refused() {
    let mut rig = remix_rig();
    // Mono at 8 kHz keeps the file small.
    let frames = 65 * 8_000;
    let samples = vec![0i16; frames];
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("remix-too-long.wav");
    std::fs::write(&path, wav_pcm16(8_000, 1, &samples)).unwrap();
    rig.send(Command::RemixCellLoad {
        deck: A,
        at: cell(0, 0),
        path,
    });
    let events = rig.engine.wait_for_loads();
    assert!(
        matches!(
            events.as_slice(),
            [Event::RemixLoadFailed {
                deck: DeckId::A,
                ..
            }]
        ),
        "{events:?}"
    );
    assert!(rig.engine.snapshot().decks[0].remix.cells[0][0].is_none());
}

#[test]
fn the_commands_parse_their_parameters() {
    let parse = |id: &str, pairs: &[(&str, &str)]| {
        let args: Args = pairs
            .iter()
            .map(|(name, value)| {
                // Choices are text, as the command line gives them.
                let numeric = ["slot", "cell", "db", "bpm", "level"].contains(name);
                let value = match value.parse::<f64>() {
                    Ok(number) if numeric => Value::Number(number),
                    _ => Value::Text((*value).to_owned()),
                };
                ((*name).to_owned(), value)
            })
            .collect();
        Command::from_args(id, &args)
    };
    assert!(matches!(
        parse("deck.type.set", &[("deck", "c"), ("type", "remix")]),
        Ok(Command::DeckTypeSet {
            deck: DeckId::C,
            kind: DeckKind::Remix
        })
    ));
    assert!(matches!(
        parse(
            "remix.cell.trigger",
            &[("deck", "a"), ("slot", "4"), ("cell", "16")]
        ),
        Ok(Command::RemixCellTrigger {
            deck: DeckId::A,
            at: CellId { slot: 3, cell: 15 }
        })
    ));
    assert!(
        parse(
            "remix.cell.trigger",
            &[("deck", "a"), ("slot", "5"), ("cell", "1")]
        )
        .is_err()
    );
    assert!(
        parse(
            "remix.cell.trigger",
            &[("deck", "a"), ("slot", "1"), ("cell", "17")]
        )
        .is_err()
    );
    assert!(matches!(
        parse(
            "remix.cell.mode.set",
            &[
                ("deck", "b"),
                ("slot", "1"),
                ("cell", "1"),
                ("mode", "one_shot")
            ]
        ),
        Ok(Command::RemixCellModeSet {
            mode: CellMode::OneShot,
            ..
        })
    ));
    assert!(matches!(
        parse("remix.quantize.set", &[("deck", "a"), ("beats", "none")]),
        Ok(Command::RemixQuantizeSet { beats: None, .. })
    ));
    assert!(matches!(
        parse("remix.quantize.set", &[("deck", "a"), ("beats", "8")]),
        Ok(Command::RemixQuantizeSet { beats: Some(8), .. })
    ));
    assert!(parse("remix.quantize.set", &[("deck", "a"), ("beats", "3")]).is_err());
}
