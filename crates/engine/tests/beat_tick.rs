//! The beat tick of a deck in the headphones. Behaviour note: `docs/behavior/beatgrid.md`,
//! Beat tick.

// The helpers that read the output are not tests themselves.
#![allow(clippy::unwrap_used)]

mod common;

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use common::{RATE, Rig, rate, sine_file};
use platter_core::{DeckId, SampleRate};
use platter_engine::{Analyser, Command, Engine, TrackAnalysis};
use platter_testkit::files::wav_pcm16;
use platter_testkit::signal::{channel, stereo, to_i16};

const A: DeckId = DeckId::A;
const B: DeckId = DeckId::B;
const BPM: f64 = 120.0;
const BEAT: f64 = 60.0 / BPM;
/// A tick starts with a sine from zero: its second sample is already above this.
const ONSET: f32 = 0.02;
const MS: f64 = 0.001;
/// The deck starts a frame after the play, and a beat on a whole frame may be passed on
/// that frame or the next one after rounding.
const FRAMES: f64 = 2.5 / RATE as f64;

/// Finds no tempo and no key, so the grid is the one the test sets.
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

/// Twenty seconds of silence: whatever the headphones play is the tick.
fn silence() -> PathBuf {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| sine_file("beat-tick-silence", 440.0, 20.0, 0.0, RATE))
        .clone()
}

/// Twenty seconds with a burst of 1 kHz starting on each beat at 120 BPM from 0: the
/// master says when a beat is heard.
fn bursts() -> PathBuf {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| {
        let rate = f64::from(RATE);
        let beat = (BEAT * rate) as usize;
        let burst = (0.01 * rate) as usize;
        let mono: Vec<f32> = (0..20 * RATE as usize)
            .map(|frame| {
                let into = frame % beat;
                if into < burst {
                    (0.5 * (std::f64::consts::TAU * 1_000.0 * into as f64 / rate).sin()) as f32
                } else {
                    0.0
                }
            })
            .collect();
        let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("beat-tick-bursts.wav");
        std::fs::write(&path, wav_pcm16(RATE, 2, &to_i16(&stereo(&mono)))).unwrap();
        path
    })
    .clone()
}

/// Twenty seconds at 440 Hz, for the master.
fn tone() -> PathBuf {
    static FILE: OnceLock<PathBuf> = OnceLock::new();
    FILE.get_or_init(|| sine_file("beat-tick-tone", 440.0, 20.0, 0.4, RATE))
        .clone()
}

/// Deck A loaded with `path` and its analysis in, at 120 BPM from a marker at 0 unless
/// `bpm` is `None`, with the beat tick on, paused at the start.
fn ticking(path: PathBuf, bpm: Option<f64>) -> Rig {
    let (engine, processor) = Engine::with_analyser(rate(), Arc::new(Nothing));
    let mut rig = Rig { engine, processor };
    rig.send(Command::MixerCrossfaderSet { position: 0.0 });
    rig.load(A, path);
    while rig.engine.snapshot().decks[A.index()].analysis.is_none() {
        rig.processor.process(&mut [], 2);
        rig.engine.poll();
        std::thread::yield_now();
    }
    if let Some(bpm) = bpm {
        rig.send(Command::DeckGridBpmSet { deck: A, bpm });
    }
    rig.send(Command::DeckBeatTickToggle { deck: A });
    rig
}

/// The headphones of `seconds` of a four-channel device.
fn headphones(rig: &mut Rig, seconds: f64) -> Vec<f32> {
    channel(&rig.run_channels(seconds, 4), 4, 2)
}

/// When each tick starts, in seconds from the start of `samples`: the sample before the
/// first one above the threshold after 20 ms of quiet.
fn onsets(samples: &[f32]) -> Vec<f64> {
    let quiet = (0.02 * f64::from(RATE)) as usize;
    let mut found = Vec::new();
    let mut last_loud: Option<usize> = None;
    for (index, sample) in samples.iter().enumerate() {
        if sample.abs() > ONSET {
            if last_loud.is_none_or(|last| index - last > quiet) {
                found.push(index.saturating_sub(1) as f64 / f64::from(RATE));
            }
            last_loud = Some(index);
        }
    }
    found
}

/// `found` are `expected`, each within `tolerance` seconds.
fn assert_ticks(found: &[f64], expected: &[f64], tolerance: f64) {
    assert_eq!(
        found.len(),
        expected.len(),
        "{found:?} against {expected:?}"
    );
    for (found, expected) in found.iter().zip(expected) {
        assert!(
            (found - expected).abs() <= tolerance,
            "{found} against {expected}"
        );
    }
}

/// The output time at which a deck playing from 0 at `tempo` reaches each beat before
/// `seconds` of output.
fn beats(tempo: f64, seconds: f64) -> Vec<f64> {
    (0..)
        .map(|beat| f64::from(beat) * BEAT / tempo)
        .take_while(|at| *at < seconds)
        .collect()
}

#[test]
fn a_tick_starts_on_each_beat_at_any_tempo() {
    for tempo in [1.0, 1.08] {
        let mut rig = ticking(silence(), Some(BPM));
        rig.send(Command::DeckTempoSet {
            deck: A,
            ratio: tempo,
        });
        // A paused deck takes the tempo at once, without the ramp.
        rig.run_channels(0.05, 4);
        rig.send(Command::DeckPlay { deck: A });
        let heard = headphones(&mut rig, 4.0);
        // One frame of the device apart at most: the tick starts on the frame that passes
        // the beat.
        assert_ticks(&onsets(&heard), &beats(tempo, 3.98), FRAMES);
        let quiet = heard.iter().zip(0..).filter(|(_, index)| {
            let at = f64::from(*index) / f64::from(RATE) % (BEAT / tempo);
            at > 0.04 && at < BEAT / tempo - 0.001
        });
        assert!(
            quiet.into_iter().all(|(sample, _)| *sample == 0.0),
            "only ticks"
        );
    }
}

#[test]
fn a_tick_follows_what_key_lock_plays() {
    for semitones in [0.0, 2.0] {
        let mut rig = ticking(bursts(), Some(BPM));
        rig.send(Command::DeckTempoSet {
            deck: A,
            ratio: 1.08,
        });
        rig.send(Command::DeckKeySet { deck: A, semitones });
        rig.run_channels(0.05, 4);
        rig.send(Command::DeckPlay { deck: A });
        let output = rig.run_channels(4.0, 4);
        let (master, ticks) = (channel(&output, 4, 0), channel(&output, 4, 2));
        // The stretch starts after the bridge: its beats are heard where the master has them.
        let heard: Vec<f64> = onsets(&master).into_iter().filter(|at| *at > 0.1).collect();
        let ticks: Vec<f64> = onsets(&ticks).into_iter().filter(|at| *at > 0.1).collect();
        assert_eq!(heard.len(), 8, "{heard:?}");
        assert_ticks(&ticks, &heard, MS);
    }
}

#[test]
fn a_marker_off_the_beats_ticks_too() {
    let mut rig = ticking(silence(), Some(BPM));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.2,
    });
    rig.send(Command::DeckGridMarkerSet { deck: A });
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 0.0,
    });
    rig.send(Command::DeckPlay { deck: A });
    let heard = headphones(&mut rig, 2.0);
    assert_ticks(&onsets(&heard), &[0.0, 0.5, 1.0, 1.2, 1.7], FRAMES);
}

#[test]
fn a_loop_on_the_beat_ticks_once_on_each_return() {
    let mut rig = ticking(silence(), Some(BPM));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.0,
    });
    // Four beats, from 1 s to 3 s.
    rig.send(Command::DeckLoopToggle { deck: A });
    rig.send(Command::DeckPlay { deck: A });
    let heard = headphones(&mut rig, 6.0);
    assert_ticks(&onsets(&heard), &beats(1.0, 5.98), FRAMES);
}

#[test]
fn a_seek_into_a_beat_waits_for_the_next_one() {
    let mut rig = ticking(silence(), Some(BPM));
    rig.send(Command::DeckPlay { deck: A });
    rig.run_channels(0.1, 4);
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 1.25,
    });
    let heard = headphones(&mut rig, 1.0);
    // The seek waits for the fade out before it lands.
    let found = onsets(&heard);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!((found[1] - found[0] - BEAT).abs() < FRAMES);
    assert!(found[0] > 0.25 && found[0] < 0.27, "{found:?}");
}

#[test]
fn a_deck_in_reverse_ticks_on_the_beats_it_passes_back() {
    let mut rig = ticking(silence(), Some(BPM));
    rig.send(Command::DeckSeek {
        deck: A,
        seconds: 3.25,
    });
    rig.send(Command::DeckPlay { deck: A });
    rig.send(Command::DeckReverse {
        deck: A,
        pressed: true,
    });
    let heard = headphones(&mut rig, 2.0);
    // From 3.25 back: 3.0, 2.5, 2.0 and 1.5.
    assert_ticks(&onsets(&heard), &[0.25, 0.75, 1.25, 1.75], FRAMES);
}

#[test]
fn no_tick_while_paused_without_a_tempo_or_with_the_tick_off() {
    let mut paused = ticking(silence(), Some(BPM));
    assert!(onsets(&headphones(&mut paused, 2.0)).is_empty());
    let mut playing = ticking(silence(), Some(BPM));
    playing.send(Command::DeckPlay { deck: A });
    playing.run_channels(1.1, 4);
    playing.send(Command::DeckPause { deck: A });
    playing.run_channels(0.05, 4);
    assert!(onsets(&headphones(&mut playing, 2.0)).is_empty());
    let mut no_tempo = ticking(silence(), None);
    no_tempo.send(Command::DeckPlay { deck: A });
    assert!(onsets(&headphones(&mut no_tempo, 2.0)).is_empty());
    let mut off = ticking(silence(), Some(BPM));
    off.send(Command::DeckBeatTickToggle { deck: A });
    off.send(Command::DeckPlay { deck: A });
    assert!(onsets(&headphones(&mut off, 2.0)).is_empty());
}

#[test]
fn the_tick_never_reaches_the_master() {
    for channels in [2, 4] {
        let mut on = ticking(tone(), Some(BPM));
        let mut off = ticking(tone(), Some(BPM));
        off.send(Command::DeckBeatTickToggle { deck: A });
        for rig in [&mut on, &mut off] {
            rig.send(Command::DeckPlay { deck: A });
        }
        let (on, off) = (
            on.run_channels(2.0, channels),
            off.run_channels(2.0, channels),
        );
        for side in 0..2 {
            assert_eq!(channel(&on, channels, side), channel(&off, channels, side));
        }
    }
}

#[test]
fn the_tick_is_heard_whatever_the_mix_and_follows_the_volume() {
    let mut rig = ticking(silence(), Some(BPM));
    rig.send(Command::MixerCueMixSet { position: 1.0 });
    rig.send(Command::DeckPlay { deck: A });
    let full = headphones(&mut rig, 1.0);
    assert_eq!(onsets(&full).len(), 2);
    rig.send(Command::MixerCueVolumeSet { db: -12.0 });
    rig.run_channels(0.1, 4);
    let quieter = headphones(&mut rig, 1.0);
    let peak = |samples: &[f32]| samples.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
    let ratio = f64::from(peak(&quieter) / peak(&full));
    assert!((ratio - 0.25).abs() < 0.02, "{ratio}");
}

#[test]
fn the_tick_stays_on_for_the_next_track_and_a_new_device_rate() {
    let mut rig = ticking(silence(), Some(BPM));
    rig.load(A, silence());
    rig.processor.process(&mut [], 2);
    rig.engine.poll();
    let decks = rig.engine.snapshot().decks;
    assert!(decks[A.index()].beat_tick);
    assert!(!decks[B.index()].beat_tick);
    let other = SampleRate::new(44_100).unwrap();
    let (engine, mut processor) = Engine::with_controls_of(other, &rig.engine);
    processor.process(&mut [], 2);
    let decks = engine.snapshot().decks;
    assert!(decks[A.index()].beat_tick);
    assert!(!decks[B.index()].beat_tick);
}
