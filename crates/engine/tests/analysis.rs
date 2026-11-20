//! The analysis of a track on load, the resets to what it found and the lock. Behaviour
//! note: `docs/behavior/analysis.md`.

// The helpers that write the signals and read the snapshot are not tests themselves.
#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use common::{RATE, Rig, sine_file};
use platter_core::{BeatGrid, Bpm, DeckId, SampleRate};
use platter_engine::{
    Analyser, Command, Engine, Event, Key, Mode, PlatterAnalyser, SharedRange, TempoRange,
    TrackAnalysis, analyse_file,
};
use platter_testkit::files::wav_pcm16;
use platter_testkit::signal::{stereo, to_i16};

const A: DeckId = DeckId::A;

/// A kick on every beat and a hi-hat on every off-beat, the first kick at `offset` seconds,
/// written as a WAV file. `name` must be unique among the tests.
fn drums_file(name: &str, bpm: f64, offset: f64, seconds: f64) -> PathBuf {
    let rate = f64::from(RATE);
    let mut mono = vec![0.0f32; (seconds * rate) as usize];
    let beat = 60.0 / bpm;
    let mut noise = 7u32;
    let mut time = offset;
    while time < seconds {
        let start = (time * rate).round() as usize;
        for (index, sample) in mono
            .iter_mut()
            .skip(start)
            .take(RATE as usize / 4)
            .enumerate()
        {
            let t = index as f32 / RATE as f32;
            let hz = 50.0 + 100.0 * (-t * 30.0).exp();
            *sample += 0.6 * (TAU * hz * t).sin() * (-t * 12.0).exp();
        }
        let hat = ((time + beat / 2.0) * rate).round() as usize;
        for (index, sample) in mono
            .iter_mut()
            .skip(hat)
            .take(RATE as usize / 25)
            .enumerate()
        {
            noise = noise.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = (noise >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
            *sample += 0.15 * white * (-(index as f32) / RATE as f32 * 80.0).exp();
        }
        time += beat;
    }
    write(name, &mono)
}

/// I-IV-V-I in A minor as held chords of three notes with harmonics, two seconds each.
fn chords_file(name: &str, seconds: f64) -> PathBuf {
    let progression: [[f32; 3]; 4] = [
        [0.0, 3.0, 7.0],
        [5.0, 8.0, 12.0],
        [7.0, 11.0, 14.0],
        [0.0, 3.0, 7.0],
    ];
    let hold = 2 * RATE as usize;
    let mono: Vec<f32> = (0..(seconds * f64::from(RATE)) as usize)
        .map(|frame| {
            let t = frame as f32 / RATE as f32;
            let mut sample = 0.0;
            for semitone in progression[(frame / hold) % progression.len()] {
                let hz = 220.0 * 2f32.powf(semitone / 12.0);
                for (harmonic, level) in [(1.0, 1.0), (2.0, 0.5), (3.0, 0.3), (4.0, 0.2)] {
                    sample += level * (TAU * hz * harmonic * t).sin();
                }
            }
            0.08 * sample
        })
        .collect();
    write(name, &mono)
}

fn write(name: &str, mono: &[f32]) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.wav"));
    std::fs::write(&path, wav_pcm16(RATE, 2, &to_i16(&stereo(mono)))).unwrap();
    path
}

/// Platter's analysis, held back until the test drops the sender of its gate.
struct Gated(Mutex<Receiver<()>>);

impl Analyser for Gated {
    fn analyse(&self, stereo: &[f32], rate: SampleRate) -> TrackAnalysis {
        // Nothing is ever sent: the gate opens when its sender is dropped.
        let _ = self.0.lock().unwrap().recv();
        PlatterAnalyser::default().analyse(stereo, rate)
    }
}

/// A rig whose analyses finish only once the returned gate is dropped.
fn gated_rig() -> (Rig, Sender<()>) {
    let (gate, wait) = channel();
    let (engine, processor) =
        Engine::with_analyser(common::rate(), Arc::new(Gated(Mutex::new(wait))));
    (Rig { engine, processor }, gate)
}

/// Starts a load on a gated rig and returns once the deck holds the track, before its
/// analysis is in.
fn load_without_analysis(rig: &mut Rig, path: PathBuf) {
    rig.send(Command::DeckLoad { deck: A, path });
    loop {
        let events = rig.engine.poll();
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::LoadFailed { .. })),
            "{events:?}"
        );
        if events
            .iter()
            .any(|event| matches!(event, Event::Loaded { .. }))
        {
            assert!(rig.engine.snapshot().decks[0].analysing);
            return;
        }
        std::thread::yield_now();
    }
}

/// Lets the processor apply what was sent and returns the grid of deck A as reported.
fn grid(rig: &mut Rig) -> BeatGrid {
    rig.run(0.01);
    rig.engine.poll();
    *rig.engine.snapshot().decks[0].grid
}

fn analysis(rig: &Rig) -> Option<TrackAnalysis> {
    rig.engine.snapshot().decks[0].analysis
}

fn bpm(value: f64) -> Bpm {
    Bpm::new(value).unwrap()
}

fn assert_found(grid: &BeatGrid, tempo: f64, first_beat: f64) {
    let found = grid.bpm().unwrap().value();
    assert!((found - tempo).abs() < 0.005, "{grid:?}");
    let [marker] = grid.markers() else {
        panic!("{grid:?}");
    };
    assert!((marker - first_beat).abs() < 0.005, "{grid:?}");
}

#[test]
fn a_loaded_track_gets_the_tempo_and_the_first_beat_found() {
    let mut rig = Rig::with_deck_a(drums_file("analysis-drums", 124.0, 0.2, 20.0));
    let snapshot = rig.engine.snapshot();
    assert!(!snapshot.decks[0].analysing);
    let found = analysis(&rig).unwrap();
    assert_eq!(found.key, None);
    assert_found(&grid(&mut rig), 124.0, 0.2);
    assert!(!grid(&mut rig).is_locked());
}

#[test]
fn the_deck_plays_and_syncs_while_its_track_is_analysed() {
    let (mut rig, gate) = gated_rig();
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.send(Command::ClockTempoSet { bpm: 130.0 });
    load_without_analysis(
        &mut rig,
        drums_file("analysis-while-playing", 124.0, 0.1, 20.0),
    );
    assert!(rig.engine.snapshot().decks[0].analysing);
    assert_eq!(analysis(&rig), None);
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckSyncToggle { deck: A });
    rig.run(0.1);
    drop(gate);
    rig.engine.wait_for_loads();
    assert_found(&grid(&mut rig), 124.0, 0.1);
    rig.run(0.1);
    let deck = &rig.engine.snapshot().decks[0];
    assert!(deck.playing && deck.sync);
    assert!(deck.position_seconds > 0.2);
    // Once its grid has a tempo, the deck has a place in the beat of the clock.
    assert!(deck.phase.is_some());
}

#[test]
fn a_grid_edited_or_locked_before_the_result_keeps_what_the_user_did() {
    let (mut rig, gate) = gated_rig();
    load_without_analysis(&mut rig, drums_file("analysis-edited", 124.0, 0.2, 20.0));
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 100.0,
    });
    drop(gate);
    rig.engine.wait_for_loads();
    let edited = grid(&mut rig);
    assert_eq!(edited.bpm(), Some(bpm(100.0)));
    assert_eq!(edited.markers(), &[0.0]);

    let (mut rig, gate) = gated_rig();
    load_without_analysis(&mut rig, drums_file("analysis-locked", 124.0, 0.2, 20.0));
    rig.send(Command::DeckGridLockToggle { deck: A });
    drop(gate);
    rig.engine.wait_for_loads();
    let locked = grid(&mut rig);
    assert!(locked.is_empty() && locked.is_locked());
    assert!(analysis(&rig).unwrap().tempo.is_some());
}

#[test]
fn a_result_for_a_track_no_longer_loaded_is_dropped() {
    let (mut rig, gate) = gated_rig();
    load_without_analysis(&mut rig, drums_file("analysis-ejected", 124.0, 0.2, 20.0));
    rig.send(Command::DeckEject { deck: A });
    drop(gate);
    rig.engine.wait_for_loads();
    assert_eq!(analysis(&rig), None);
    assert!(!rig.engine.snapshot().decks[0].analysing);

    let (mut rig, gate) = gated_rig();
    load_without_analysis(&mut rig, drums_file("analysis-replaced", 124.0, 0.2, 20.0));
    rig.send(Command::DeckLoad {
        deck: A,
        path: sine_file("analysis-replacing", 440.0, 4.0, 0.5, RATE),
    });
    drop(gate);
    rig.engine.wait_for_loads();
    assert!(grid(&mut rig).is_empty());
    assert_eq!(analysis(&rig).unwrap().tempo, None);
}

#[test]
fn a_load_that_fails_leaves_the_analysis_of_the_track_in_the_deck() {
    let (mut rig, gate) = gated_rig();
    load_without_analysis(&mut rig, drums_file("analysis-kept", 124.0, 0.2, 20.0));
    let broken = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("analysis-broken.wav");
    std::fs::write(&broken, b"not a wave file").unwrap();
    rig.send(Command::DeckLoad {
        deck: A,
        path: broken,
    });
    drop(gate);
    let events = rig.engine.wait_for_loads();
    assert!(
        matches!(events.as_slice(), [Event::LoadFailed { .. }]),
        "{events:?}"
    );
    assert!(!rig.engine.snapshot().decks[0].analysing);
    assert!(analysis(&rig).unwrap().tempo.is_some());
    assert_found(&grid(&mut rig), 124.0, 0.2);
}

#[test]
fn the_resets_bring_back_what_the_analysis_found() {
    let mut rig = Rig::with_deck_a(drums_file("analysis-resets", 124.0, 0.2, 20.0));
    rig.send(Command::DeckGridBpmSet { deck: A, bpm: 90.0 });
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 3.0,
    });
    rig.send(Command::DeckGridMarkerSet { deck: A });
    rig.send(Command::DeckGridBpmReset { deck: A });
    let tempo_reset = grid(&mut rig);
    assert!((tempo_reset.bpm().unwrap().value() - 124.0).abs() < 0.005);
    assert_eq!(tempo_reset.markers().len(), 2);

    rig.send(Command::DeckGridReset { deck: A });
    assert_found(&grid(&mut rig), 124.0, 0.2);

    // On a grid without markers, the tempo reset puts one on the first beat found.
    rig.send(Command::DeckGridMarkerDelete { deck: A });
    assert!(grid(&mut rig).markers().is_empty());
    rig.send(Command::DeckGridBpmReset { deck: A });
    assert_found(&grid(&mut rig), 124.0, 0.2);
}

#[test]
fn a_locked_grid_takes_no_reset() {
    let mut rig = Rig::with_deck_a(drums_file("analysis-reset-locked", 124.0, 0.2, 20.0));
    rig.send(Command::DeckGridBpmSet { deck: A, bpm: 90.0 });
    rig.send(Command::DeckGridLockToggle { deck: A });
    let before = grid(&mut rig);
    rig.send(Command::DeckGridReset { deck: A });
    rig.send(Command::DeckGridBpmReset { deck: A });
    assert_eq!(grid(&mut rig), before);
    rig.send(Command::DeckGridLockToggle { deck: A });
    rig.send(Command::DeckGridReset { deck: A });
    assert_found(&grid(&mut rig), 124.0, 0.2);
}

#[test]
fn a_track_without_a_steady_beat_keeps_an_empty_grid() {
    let mut rig = Rig::with_deck_a(sine_file("analysis-no-beat", 440.0, 10.0, 0.5, RATE));
    assert_eq!(analysis(&rig).unwrap().tempo, None);
    assert!(grid(&mut rig).is_empty());
    rig.send(Command::DeckGridBpmSet {
        deck: A,
        bpm: 100.0,
    });
    rig.send(Command::DeckGridBpmReset { deck: A });
    assert_eq!(grid(&mut rig).bpm(), Some(bpm(100.0)));
    rig.send(Command::DeckGridReset { deck: A });
    assert!(grid(&mut rig).is_empty());
}

#[test]
fn the_resets_do_nothing_before_the_analysis_or_on_an_empty_deck() {
    let (mut rig, _gate) = gated_rig();
    rig.send(Command::DeckGridReset { deck: A });
    rig.send(Command::DeckGridBpmReset { deck: A });
    assert!(grid(&mut rig).is_empty());

    load_without_analysis(
        &mut rig,
        drums_file("analysis-reset-early", 124.0, 0.2, 20.0),
    );
    rig.send(Command::DeckGridBpmSet { deck: A, bpm: 90.0 });
    rig.send(Command::DeckGridReset { deck: A });
    assert_eq!(grid(&mut rig).bpm(), Some(bpm(90.0)));
}

#[test]
fn the_key_of_the_track_is_published_with_the_deck() {
    let rig = Rig::with_deck_a(chords_file("analysis-key", 16.0));
    assert_eq!(
        analysis(&rig).unwrap().key,
        Some(Key {
            tonic: 9,
            mode: Mode::Minor
        })
    );
}

#[test]
fn a_new_range_of_tempos_applies_to_the_next_analysis() {
    let file = drums_file("range.wav", 90.0, 0.25, 20.0);
    let range = SharedRange::default();
    let analyser = PlatterAnalyser::with_range(range.clone());
    let bpm = |analyser: &PlatterAnalyser| {
        analyse_file(analyser, &file)
            .unwrap()
            .tempo
            .map(|tempo| tempo.bpm.value())
    };
    assert_eq!(bpm(&analyser), Some(90.0));
    // 90 BPM is out of this range: its double is found.
    range.set(TempoRange::new(120.0, 240.0).unwrap());
    assert_eq!(bpm(&analyser), Some(180.0));
    range.set(TempoRange::default());
    assert_eq!(bpm(&analyser), Some(90.0));
}
