# 0020. Platter detects tempo, beatgrid and key with its own code

Status: accepted (M3).

## Context

The roadmap had the analysis of M3 start with a community crate behind a project trait, and
`docs/dependencies.md` named one, stratum-dsp, on the condition that it was measured before it
was adopted. What the detection must give is set in `docs/behavior/analysis.md`: a tempo to a
hundredth of a BPM, a first beat within a few milliseconds, and no answer rather than a wrong
one. A tempo off by 0.2 % at 128 BPM puts a grid one beat off within four minutes, so a synced
deck drifts out of the beat during one track.

## Options

1. stratum-dsp 1.0.0, as approved.
2. Platter's own detection, in a crate of its own, without any dependency.

## Measurement of option 1

Measured on 15 November 2026 in a project outside the repository, on synthesised signals only:
two minutes of a kick on every beat and a hi-hat on every off-beat, chord progressions of one
minute, and unusable input. Release build, Windows x64.

| Signal | Expected | Found |
| --- | --- | --- |
| Drums at 120, first beat at 250 ms | 120, 250 ms | 119.86, 23 ms |
| Drums at 128, first beat at 100 ms | 128, 100 ms | 127.75, 12 ms |
| Drums at 127.3 | 127.3 | 127.75 |
| Drums at 140 | 140 | 139.74 |
| Drums at 174 | 174 | 86.9 |
| Drums at 90, 100 | 90, 100 | 90.00, 100.15 |
| I-IV-V-I in C major, F sharp major | C major, F sharp major | E minor, A sharp minor |
| i-iv-V-i in A minor, E minor | A minor, E minor | A minor, E minor |
| A constant value, NaN samples | An error | 40 BPM, C minor and C major, as results |
| Eight minutes of drums | | 14 s |

Its tempo errors would put a grid off the beat within minutes, its first beat was wrong in
every case but one, half of the keys were wrong, and it answers bad input with a result. It
also brings in symphonia 0.5 with every format, which Platter does not use, beside symphonia
0.6, and rayon and serde_json.

## Choice

Option 2, chosen by the maintainer on 15 November 2026 once these figures were shown. The crate
`platter-analysis` (layer L1) holds a radix-2 FFT and two detections:

- Tempo and first beat: the spectral flux of the log-magnitude spectrum, over the whole
  spectrum and over its band below about 200 Hz, at about 11 kHz with a hop of 5.8 ms. The
  autocorrelation of the flux gives three candidate periods; each is refined by the sharpness
  of its phase histogram, then by the comb that collects the most flux over the whole track. The
  low band decides between candidates and between a beat and its off-beat, because a bright
  hi-hat can carry more flux than the kick. The tempo is then taken to the slowest of its
  octaves in the range and doubled while the off-beats are nearly as strong as the beats.
- Key: the energy of each pitch class from 55 Hz to 2 kHz in frames of 0.74 s, each frame
  weighted alike, correlated with the Krumhansl and Kessler profiles of the 24 keys.

The detection is a function and not yet a trait: it has one implementation, and the trait
comes with the engine, which is its first user, so a second implementation can take its place.

## How it was measured

`cargo test --release -p platter-analysis --test detection measure -- --ignored --nocapture`,
Windows x64, on the signals of the tests with one minute of drums each:

| Signal | Found |
| --- | --- |
| Drums at 70, 87.5, 100, 122, 126, 133.33, 150, 172 and 180, first beat at 123 ms | The same tempos to the hundredth; first beat from 0.07 to 0.49 ms late |
| Drums at 128 with noise 22 dB under the kick | 128; first beat 8.8 ms late |
| Eight minutes of drums at 128 | 128 in 1.2 s |
| Twenty minutes of stereo | 2.2 s |

The flux peaks about 13.8 ms before the start of a kick, as the window takes in the first of
the sound; the first beat is corrected by that amount, measured from 13 to 15 ms across the
tempos. The tests of `crates/analysis/tests/detection.rs` check every tempo to 0.005 BPM and
every first beat to 5 ms, the keys of twelve cadences, and the inputs of the edge cases.

All of this is on synthesised signals. A collection of real tracks with known tempos and keys
has not been measured yet; it stays outside the repository and only its aggregate results will
be added here.
