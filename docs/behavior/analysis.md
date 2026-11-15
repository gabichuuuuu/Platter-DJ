# Analysis: tempo, beatgrid and key

Analysis reads a whole track and finds its tempo, where its beats fall and its musical key, so
a DJ does not tap or type them. This note covers what the detection finds and how it fails.
Running it in the background on load, the analysis lock, the reset of the grid to the analysed
values and the display of the key come with the engine and the interface, later in M3. The
grid it fills is described in `docs/behavior/beatgrid.md`.

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Managing your track collection*, sections *Analyzing tracks manually*, *Analysis dialog
  options*
- *Preferences*, section *Analyze options page*, subsections *Analyze options* and *Musical key*
- *Global concepts*, section *Beatgrid*

Analysis finds the tempo, the beatgrid, the key and a gain for each track. The tempo is looked
for in a range of tempos the user chooses; a narrow range is more accurate, a wide one suits a
collection of mixed tempos. Setting the beatgrid places one grid marker on a downbeat. The key
can be shown in musical notation, musical with sharps only, or Open Key. Tracks can be analysed
when they are imported and when they are loaded into a deck, and a locked track keeps its
tempo, grid and key when it is analysed again.

## What Platter does

Detection is one function of `platter-analysis` over the decoded samples of a track. It returns
a tempo with the position of a beat, and a key, each one only when it is found.

### Tempo and grid

- The tempo is looked for between 70 and 180 BPM unless another range is given; the range is
  from 20 to 400 BPM, and its top at least 1.5 times its bottom so every tempo has a place in
  it.
- The tempo is found to a hundredth of a BPM. A tempo with a hundredth part that sits within
  0.02 BPM of a whole number is given as that whole number, since most music made with a
  sequencer has one.
- When a tempo and its double both fit the range, the double is chosen only when the beats
  between the beats of the slower one are nearly as strong: a hi-hat on the off-beat does not
  double the tempo, a kick on every beat of a fast track does not halve it.
- The grid marker goes on the first beat of the track: the earliest beat at or after its start.
  It is placed to within 5 ms of where the beat starts.
- No tempo is given for a track without a steady beat: silence, noise, a single held note, or
  a track shorter than eight beats of the slowest tempo of the range.

### Key

- The key is one of the 24 major and minor keys, found from how much of the track is in each
  of the twelve pitch classes, compared with a profile of each key (Krumhansl and Kessler,
  1982, *Tracing the dynamic changes in perceived tonal organization in a spatial
  representation of musical keys*, Psychological Review 89(4), 334-368,
  <https://doi.org/10.1037/0033-295X.89.4.334>).
- Tuning is assumed at A = 440 Hz, give or take a quarter of a semitone.
- No key is given when no profile stands out: silence, noise, drums alone.

### Onsets and tempo

The tempo is found from how the spectrum of the track changes from one moment to the next, the
spectral flux (Bello and others, 2005, *A tutorial on onset detection in music signals*, IEEE
Transactions on Speech and Audio Processing 13(5), 1035-1047,
<https://doi.org/10.1109/TSA.2005.851998>), and from how that change repeats: the beats are the
period and the offset that collect the most of it over the whole track.

## Differences

- The reference product offers an automatic range that also finds the downbeat. Platter's range
  is fixed and the marker goes on the first beat, which may not be the first beat of a bar:
  the bars of the waveform then count from it until the marker is moved.
- One tempo per track, as the grid holds in M3.
- The tuning of a track is not measured; a track tuned far from A = 440 Hz may get a key a
  semitone away.
- The gain of the analysis is autogain, which comes later in M3.

## Edge cases

| Input | Result |
| --- | --- |
| No samples, a sample rate of 0, a channel count of 0 | An error |
| A sample that is not a finite number | An error |
| Silence, a constant value, white noise | No tempo and no key |
| A track shorter than eight beats of the slowest tempo of the range | No tempo |
| A range outside 20 to 400 BPM, or one whose top is less than 1.5 times its bottom | An error |
| A track longer than 20 minutes | Only its first 20 minutes are read |

## Tests

`crates/analysis/tests/detection.rs`, on synthesised signals: drum patterns at known tempos and
offsets, chord progressions in known keys, and the inputs of the edge cases. The measurements
behind the method are in `docs/decisions/0020-own-analysis.md`.
