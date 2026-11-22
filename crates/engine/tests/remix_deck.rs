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
use platter_engine::{Analyser, CellId, CellMode, Command, DeckKind, Engine, Event, TrackAnalysis};
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
