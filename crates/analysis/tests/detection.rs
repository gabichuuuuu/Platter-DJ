//! Detection on synthesised signals. Behaviour note: `docs/behavior/analysis.md`.

// The helpers that build the signals and read the detection are not tests themselves.
#![allow(clippy::unwrap_used)]

use std::f32::consts::TAU;

use platter_analysis::{AnalysisError, Key, Mode, TempoRange, detect};

const RATE: u32 = 44_100;

/// A small deterministic noise source.
struct Noise(u32);

impl Noise {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// A kick on every beat and a hi-hat on every off-beat, the first kick at `offset` seconds.
fn drums(bpm: f64, offset: f64, seconds: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut out = vec![0.0f32; frames];
    let beat = 60.0 / bpm;
    let mut noise = Noise(7);
    let mut time = offset;
    while time < seconds {
        let start = (time * f64::from(RATE)).round() as usize;
        for index in 0..RATE as usize / 4 {
            let Some(sample) = out.get_mut(start + index) else {
                break;
            };
            let t = index as f32 / RATE as f32;
            let hz = 50.0 + 100.0 * (-t * 30.0).exp();
            *sample += 0.6 * (TAU * hz * t).sin() * (-t * 12.0).exp();
        }
        let hat = ((time + beat / 2.0) * f64::from(RATE)).round() as usize;
        for index in 0..RATE as usize / 25 {
            let Some(sample) = out.get_mut(hat + index) else {
                break;
            };
            *sample += 0.15 * noise.next() * (-(index as f32) / RATE as f32 * 80.0).exp();
        }
        time += beat;
    }
    out
}

/// Chords of three notes, given in semitones from A4, each held two seconds, with three
/// harmonics above each note.
fn chords(progression: &[[f32; 3]], seconds: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(RATE)) as usize;
    let hold = 2 * RATE as usize;
    (0..frames)
        .map(|frame| {
            let chord = progression[(frame / hold) % progression.len()];
            let t = frame as f32 / RATE as f32;
            let mut sample = 0.0;
            for semitone in chord {
                let hz = 440.0 * 2f32.powf(semitone / 12.0);
                for (harmonic, level) in [(1.0, 1.0), (2.0, 0.5), (3.0, 0.3), (4.0, 0.2)] {
                    sample += level * (TAU * hz * harmonic * t).sin();
                }
            }
            0.08 * sample
        })
        .collect()
}

fn mono(samples: &[f32]) -> platter_analysis::Detection {
    detect(samples, 1, RATE, TempoRange::default()).unwrap()
}

#[test]
fn the_tempo_and_the_first_beat_of_a_drum_pattern_are_found() {
    for (bpm, offset) in [
        (90.0, 0.0),
        (120.0, 0.25),
        (128.0, 0.1),
        (127.3, 0.3),
        (140.0, 0.05),
        (174.0, 0.2),
        (180.0, 0.1),
    ] {
        let tempo = mono(&drums(bpm, offset, 30.0)).tempo.unwrap();
        assert!((tempo.bpm - bpm).abs() < 0.005, "{bpm}: {tempo:?}");
        let beat = 60.0 / bpm;
        let off =
            ((tempo.first_beat - offset + beat / 2.0).rem_euclid(beat) - beat / 2.0) * 1_000.0;
        assert!(off.abs() < 5.0, "{bpm}: first beat {off:.2} ms off");
    }
}

#[test]
fn a_stereo_track_at_another_rate_gives_the_same_tempo() {
    let left = drums(124.0, 0.15, 30.0);
    let stereo: Vec<f32> = left.iter().flat_map(|sample| [*sample, *sample]).collect();
    // The same samples read as 48 kHz play faster by 48 / 44.1.
    let tempo = detect(&stereo, 2, 48_000, TempoRange::default())
        .unwrap()
        .tempo
        .unwrap();
    let expected = 124.0 * 48_000.0 / 44_100.0;
    assert!((tempo.bpm - expected).abs() < 0.01, "{tempo:?}");
}

/// I-IV-V-I in the major key on `tonic`, and i-iv-V-i in the minor one.
fn cadence(tonic: f32, mode: Mode) -> Vec<[f32; 3]> {
    let triad = |root: f32, third: f32| [root, root + third, root + 7.0];
    let (third, fourth) = match mode {
        Mode::Major => (4.0, 4.0),
        Mode::Minor => (3.0, 3.0),
    };
    vec![
        triad(tonic, third),
        triad(tonic + 5.0, fourth),
        triad(tonic + 7.0, 4.0),
        triad(tonic, third),
    ]
}

#[test]
fn the_key_of_a_cadence_is_found_in_major_and_minor() {
    // Semitones from A4 to the tonic, and its pitch class.
    for (semitones, tonic) in [
        (-9.0, 0),
        (0.0, 9),
        (-3.0, 6),
        (-5.0, 4),
        (-2.0, 7),
        (1.0, 10),
    ] {
        for mode in [Mode::Major, Mode::Minor] {
            let key = mono(&chords(&cadence(semitones, mode), 24.0)).key;
            assert_eq!(key, Some(Key { tonic, mode }), "{semitones} {mode:?}");
        }
    }
}

#[test]
fn drums_under_chords_give_both_the_tempo_and_the_key() {
    let mut mix = drums(124.0, 0.2, 30.0);
    for (sample, chord) in mix.iter_mut().zip(chords(&cadence(0.0, Mode::Minor), 30.0)) {
        *sample += 0.5 * chord;
    }
    let found = mono(&mix);
    assert_eq!(found.tempo.map(|tempo| tempo.bpm), Some(124.0));
    assert_eq!(
        found.key,
        Some(Key {
            tonic: 9,
            mode: Mode::Minor
        })
    );
}

#[test]
fn silence_a_constant_noise_and_a_held_note_have_no_tempo() {
    let mut noise = Noise(3);
    let white: Vec<f32> = (0..RATE as usize * 20)
        .map(|_| 0.3 * noise.next())
        .collect();
    let note: Vec<f32> = (0..RATE as usize * 20)
        .map(|frame| 0.3 * (TAU * 440.0 * frame as f32 / RATE as f32).sin())
        .collect();
    for (name, samples) in [
        ("silence", vec![0.0; RATE as usize * 20]),
        ("constant", vec![0.5; RATE as usize * 20]),
        ("noise", white.clone()),
        ("note", note),
    ] {
        assert_eq!(mono(&samples).tempo, None, "{name}");
    }
    for (name, samples) in [("silence", vec![0.0; RATE as usize * 20]), ("noise", white)] {
        assert_eq!(mono(&samples).key, None, "{name}");
    }
}

#[test]
fn a_track_shorter_than_eight_slow_beats_has_no_tempo() {
    // Eight beats at 70 BPM take 6.9 s.
    assert_eq!(mono(&drums(120.0, 0.0, 6.0)).tempo, None);
}

#[test]
fn unusable_input_is_an_error() {
    let range = TempoRange::default();
    assert_eq!(detect(&[], 1, RATE, range), Err(AnalysisError::NoSamples));
    assert_eq!(
        detect(&[0.0; 4], 0, RATE, range),
        Err(AnalysisError::NoFormat)
    );
    assert_eq!(detect(&[0.0; 4], 1, 0, range), Err(AnalysisError::NoFormat));
    let mut samples = vec![0.0; 1_000];
    samples[500] = f32::NAN;
    assert_eq!(
        detect(&samples, 1, RATE, range),
        Err(AnalysisError::NotFinite)
    );
    assert_eq!(
        detect(&[0.1; 300], 1, RATE, range),
        Ok(platter_analysis::Detection {
            tempo: None,
            key: None
        })
    );
}

/// The figures of `docs/decisions/0020-own-analysis.md`. Run with
/// `cargo test --release -p platter-analysis --test detection measure -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure() {
    for bpm in [70.0, 87.5, 100.0, 122.0, 126.0, 133.33, 150.0, 172.0, 180.0] {
        let tempo = mono(&drums(bpm, 0.123, 60.0)).tempo;
        let beat = 60.0 / bpm;
        let off = tempo.map(|tempo| {
            ((tempo.first_beat - 0.123 + beat / 2.0).rem_euclid(beat) - beat / 2.0) * 1_000.0
        });
        println!("drums {bpm}: {tempo:?}, first beat off {off:?} ms");
    }
    let long = drums(128.0, 0.05, 480.0);
    let start = std::time::Instant::now();
    let tempo = mono(&long).tempo;
    println!(
        "8 minutes at 128: {tempo:?} in {} ms",
        start.elapsed().as_millis()
    );
    let start = std::time::Instant::now();
    let _ = detect(
        &vec![0.1f32; RATE as usize * 1_200 * 2],
        2,
        RATE,
        TempoRange::default(),
    );
    println!("20 minutes of stereo: {} ms", start.elapsed().as_millis());
    let mut kicks = drums(128.0, 0.0, 30.0);
    let mut noise = Noise(11);
    for sample in &mut kicks {
        *sample += 0.05 * noise.next();
    }
    println!("drums alone, with noise: {:?}", mono(&kicks));
}
