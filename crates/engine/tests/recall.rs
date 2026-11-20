//! A loaded deck starts from the grid and the cues the collection kept for its track.
//! Behaviour note: `docs/behavior/library.md`, *Cues, grid and analysis*.

// The helpers that write the signals and read the snapshot are not tests themselves.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::{RATE, Rig, sine_file};
use platter_core::{Cues, DeckId, HotcueSlot, TrackMemory};
use platter_engine::{Command, Engine};
use platter_testkit::files::wav_pcm16;
use platter_testkit::signal::{stereo, to_i16};

const A: DeckId = DeckId::A;

fn kept() -> TrackMemory {
    TrackMemory::parse(
        "platter-track 1\nbpm 120\nmarker 0.5\ncue 1 cue 1.25 0 Intro\ncue 4 loop 2 1 Short\n",
    )
    .unwrap()
}

/// A rig whose loads find `memory` for the file at `path`, and nothing for any other.
fn rig_recalling(path: PathBuf, memory: TrackMemory) -> Rig {
    let (mut engine, processor) = Engine::new(common::rate());
    engine.set_recall(Arc::new(move |asked: &Path| {
        (asked == path.as_path()).then_some(memory)
    }));
    let mut rig = Rig { engine, processor };
    rig.send(Command::MixerAutogainToggle);
    rig
}

/// A kick on every beat at `bpm`, the first at the start, written as a WAV file.
fn kicks_file(name: &str, bpm: f64, seconds: f64) -> PathBuf {
    let rate = f64::from(RATE);
    let mut mono = vec![0.0f32; (seconds * rate) as usize];
    let mut time = 0.0;
    while time < seconds {
        let start = (time * rate).round() as usize;
        for (index, sample) in mono
            .iter_mut()
            .skip(start)
            .take(RATE as usize / 4)
            .enumerate()
        {
            let t = index as f32 / RATE as f32;
            *sample += 0.6 * (TAU * 60.0 * t).sin() * (-t * 12.0).exp();
        }
        time += 60.0 / bpm;
    }
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.wav"));
    std::fs::write(&path, wav_pcm16(RATE, 2, &to_i16(&stereo(&mono)))).unwrap();
    path
}

/// Lets the processor apply what was sent and returns the grid and the cues of deck A.
fn deck_a(rig: &mut Rig) -> TrackMemory {
    rig.run(0.01);
    rig.engine.poll();
    let deck = &rig.engine.snapshot().decks[0];
    TrackMemory {
        grid: *deck.grid,
        cues: deck.cues,
    }
}

#[test]
fn a_loaded_deck_starts_with_what_was_kept_for_its_track() {
    let path = sine_file("recall-kept", 440.0, 4.0, 0.3, RATE);
    let mut rig = rig_recalling(path.clone(), kept());
    rig.load(A, path);
    let deck = deck_a(&mut rig);
    assert_eq!(deck, kept());
    assert_eq!(deck.cues.selected(), None);
    // The kept cues are stored cues like any other: a pad deletes its own.
    rig.send(Command::DeckHotcueDelete {
        deck: A,
        slot: HotcueSlot::ALL[0],
    });
    assert_eq!(deck_a(&mut rig).cues.get(HotcueSlot::ALL[0]), None);
}

#[test]
fn the_analysis_of_the_load_leaves_a_kept_grid_as_it_is() {
    let path = kicks_file("recall-kicks", 128.0, 16.0);
    let mut rig = rig_recalling(path.clone(), kept());
    // Loading waits for the analysis too.
    rig.load(A, path);
    // The analysis found a tempo, which an empty grid would have taken.
    let found = rig.engine.snapshot().decks[0]
        .analysis
        .unwrap()
        .tempo
        .unwrap();
    assert!((found.bpm.value() - 128.0).abs() < 0.5, "{found:?}");
    assert_eq!(deck_a(&mut rig).grid, kept().grid);
}

#[test]
fn another_track_starts_with_nothing_kept() {
    let kept_path = sine_file("recall-first", 440.0, 2.0, 0.3, RATE);
    let other = sine_file("recall-other", 330.0, 2.0, 0.3, RATE);
    let mut rig = rig_recalling(kept_path.clone(), kept());
    rig.load(A, kept_path);
    assert_eq!(deck_a(&mut rig), kept());
    rig.load(A, other);
    let deck = deck_a(&mut rig);
    assert_eq!(deck.cues, Cues::default());
    assert!(deck.grid.is_empty());
}

#[test]
fn an_engine_without_a_recall_loads_as_before() {
    let mut rig = Rig::with_deck_a(sine_file("recall-none", 440.0, 2.0, 0.3, RATE));
    let deck = deck_a(&mut rig);
    assert!(deck.is_empty());
}

#[test]
fn a_track_with_a_load_cue_starts_at_it_while_the_preference_asks() {
    let path = sine_file("recall-load", 440.0, 4.0, 0.3, RATE);
    let memory =
        TrackMemory::parse("platter-track 1\ncue 2 load 1.5 0 Start\ncue 3 cue 3 0 Later\n")
            .unwrap();
    let mut rig = rig_recalling(path.clone(), memory);
    assert!(rig.engine.snapshot().load_marker);
    rig.load(A, path.clone());
    rig.run(0.01);
    rig.engine.poll();
    let deck = &rig.engine.snapshot().decks[0];
    assert!(
        (deck.position_seconds - 1.5).abs() < 0.01,
        "{}",
        deck.position_seconds
    );
    assert!(
        (deck.cue_seconds - 1.5).abs() < 0.01,
        "{}",
        deck.cue_seconds
    );

    rig.send(Command::GlobalLoadMarkerToggle);
    assert!(!rig.engine.snapshot().load_marker);
    rig.load(A, path);
    rig.run(0.01);
    rig.engine.poll();
    let deck = &rig.engine.snapshot().decks[0];
    assert!(deck.position_seconds < 0.001, "{}", deck.position_seconds);
}
