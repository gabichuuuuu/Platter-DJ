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

The detection is a function of the crate. The engine calls it through its trait `Analyser`, with
`PlatterAnalyser` as the implementation it starts with, so a second detection can take its
place through `Engine::with_analyser`. The tests of the engine already use one: it holds the
result back until the test lets it go, to check what happens while a track is analysed.

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

All of the above is on synthesised signals.

## Measurement on real recordings

Measured on 17 November 2026, release build, Windows x64, with `platter_analysis::detect` and
its default range of 70 to 180 BPM, called by a program outside the repository. The corpus is
the Freesound Loop Dataset (A. Ramires et al., "The Freesound Loop Dataset and Annotation
Tool", ISMIR 2020; https://zenodo.org/records/3967852): loops published on Freesound under
Creative Commons licences. Only the loops whose annotators gave a tempo, or a key, and agreed
on it were used: 2,056 with a tempo and 886 with a key, 2,248 files in all, from 0.4 s to
9.8 min long, 8 s in the median. The audio stays outside the repository. Loops are shorter
and barer than the tracks a DJ loads, so this measures the detection on hard material, not on
a collection.

Tempo, with the scores the MIREX tempo task uses: Accuracy1 counts a tempo within 4 % of the
annotated one, Accuracy2 also its double, half, triple or third.

| Loops | Answered | Accuracy1 | Accuracy2 | Within 0.5 BPM |
| --- | --- | --- | --- | --- |
| All 2,056 | 43.3 % | 31.8 % | 37.0 % | 30.8 % |
| The 1,975 annotated from 70 to 180 BPM | 43.7 % | 33.1 % | 37.6 % | 32.1 % |

By length, of the loops answered: under 4 s (468 loops) no answer; 4 to 8 s, 115 answered, 90
right; 8 to 16 s, 391 answered, 276 right; 16 to 30 s, 245 answered, 186 right; 30 s and more,
140 answered, 102 right. Of the 891 answers, 654 are right, 107 are the double, half, triple
or third, and 130 are wrong otherwise. When the detection answers, 71 % of its tempos are
within 0.5 BPM of the annotation, which is a whole number in 2,049 of the 2,056 loops.

First beat, on the 576 loops their annotators marked as cut on the beat and whose tempo was
right: the beat found lies 9.6 ms from the start in the median, within 10 ms in 50.9 % and
within 30 ms in 62.5 %.

Key, with the MIREX key score (the same key 1, a fifth 0.5, the relative key 0.3, the parallel
key 0.2): answered 70.0 %, exact 36.0 %, weighted score 43.4 % over the 886 loops. The share
exactly right is 25 % under 4 s and from 37 % to 39 % at every longer length. Of the 620 answers, 199 have the mode wrong: 125 minor keys
found as major and 74 major keys found as minor.

One file held no audio and was reported so.

What it shows: the tempo found is precise when it is right, but too many wrong tempos are
given, against the rule of `docs/behavior/analysis.md` that no answer is better than a wrong
one, and the key is right about a third of the time. Neither detection is yet what a DJ needs
from a collection; both stay in progress in `docs/parity.toml` until they improve on this
corpus and, separately, on full tracks.

## What the measurement points to

Left for a later change, found on 17 November 2026. The loops were split by the user who
uploaded them: 1,406 to try changes on and 842 kept apart, not yet looked at, to confirm them.
The published results that come with the dataset were scored the same way on the first part.

- **The annotated keys lean towards one estimator.** The paper of the dataset (section 3.2.5)
  says the annotation tool proposed to each annotator the key found by an existing EDM key
  estimator. The estimators of that family agree with the annotations in 60 to 75 % of the
  loops; an independent one shipped with the dataset agrees in 31 %, as Platter does. On this
  corpus a key score measures in part the agreement with that proposal; the annotated tempos
  came from the uploaders, not from an estimator.
- **Key.** The errors follow the harmonics of the tonic: the third harmonic falls on the
  fifth (minor keys found a fifth up) and the fifth on the major third (minor keys found as
  major). On the first part, reading only the peaks of the spectrum, adding each peak to the
  pitch classes of the fundamentals it can be the harmonic of (eight, each weighted 0.8 of
  the last) and reading from 30 Hz instead of 55 Hz raised the exact keys of the answers from
  50 % to 62 % and the answers from 71 % to 77 % of the loops, with the same profiles and
  thresholds. Estimating the tuning, a proper anti-alias filter before the reduction and
  profiles learned from the loops gave little or nothing more.
- **Tempo.** 45 of the 63 wrong tempos of the first part are 4/3 of the annotation, and 8 are
  2/3: a dotted pattern taken for the beat. Trying the periods 4/3 and 3/4 of each candidate,
  and scoring a candidate also by how regularly the bar of four of its beats repeats, brought
  the wrong answers from 12.4 % to 10.3 %. A higher salience threshold trades answers for
  fewer errors: at 5 instead of 4, 23 % fewer answers and 7.9 % wrong.

None of this is in the code yet; the changes tried are kept with the measuring program,
outside the repository.
