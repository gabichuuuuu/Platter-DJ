# Analysis: tempo, beatgrid and key

Analysis reads a whole track and finds its tempo, where its beats fall and its musical key, so
a DJ does not tap or type them. This note covers what the detection finds, how it fails, how
the engine runs it when a track is loaded and the reset of the grid to what it found. The keys
of the GRID page for the resets and the display of the key come with the interface, later in
M3. The grid it fills is described in `docs/behavior/beatgrid.md`.

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Managing your track collection*, sections *Analyzing tracks manually*, *Analysis dialog
  options*
- *Preferences*, section *Analyze options page*, subsections *Analyze options* and *Musical key*
- *Global concepts*, section *Beatgrid*
- *Overview*, section *Advanced panel*, the GRID page

Analysis finds the tempo, the beatgrid, the key and a gain for each track. The tempo is looked
for in a range of tempos the user chooses; a narrow range is more accurate, a wide one suits a
collection of mixed tempos. Setting the beatgrid places one grid marker on a downbeat. The key
can be shown in musical notation, musical with sharps only, or Open Key. Tracks can be analysed
when they are imported and when they are loaded into a deck, and a locked track keeps its
tempo, grid and key when it is analysed again. On the GRID page, one reset analyses the tempo
and the grid again and removes the markers set by hand; another brings back the analysed tempo
without moving the markers. The lock of the page keeps both the analysis and the user from
changing the values.

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

### On load

- A track is analysed each time it is loaded into a deck, on the worker thread that decoded
  it, once the deck already holds it: the track plays while it is analysed. The snapshot of
  the deck says that it is analysing until the result arrives; the window shows it later in
  M3.
- When the result arrives, a grid that is still empty and unlocked takes the tempo found and
  one marker on the first beat. A grid the user has given a tempo or a marker in the
  meantime, or has locked, stays as it is; the result is kept for the resets.
- The key found is kept with the loaded track and published with the deck.
- The tempo of the tag is not put in the grid: it stays what the deck header shows until the
  grid has a tempo (`docs/behavior/beatgrid.md`).
- A render script waits for the analysis as it waits for the load, so the grid a script
  reads is the same on every run.
- Once another track is in the deck, or the deck is ejected, the result for the previous one
  is dropped, even when it arrives later. A load that fails leaves the deck with its track and
  the analysis of that track.
- The range of tempos is the default, 70 to 180 BPM, until the preferences choose another.

### Reset and lock

| Command | Effect |
| --- | --- |
| `deck.grid.reset` | The grid becomes what the analysis found: its tempo and one marker on the first beat. The markers set by hand go. Without a tempo found, the grid is left without tempo and markers |
| `deck.grid.bpm.reset` | The tempo becomes the one found; the markers stay. A grid without markers also gets one on the first beat found |

- Both do nothing on an empty deck, before the analysis of the loaded track has finished, and
  while the grid is locked. `deck.grid.bpm.reset` also does nothing when no tempo was found.
- The analysis lock is the lock of the grid, `deck.grid.lock.toggle`: while it is on, neither
  the result of the analysis nor a reset changes the grid, as with every other edit.

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
- The reference product analyses again on a reset of the grid; the result of Platter's
  analysis depends only on the samples, so a reset uses the result of the load.
- Analysis on import into the collection comes with the collection, later in M3. Until then
  the result lives with the loaded track and is found again on each load.
- The lock does not yet keep a key: the key cannot be edited, so the analysis of the load
  always gives it.

## Edge cases

| Input | Result |
| --- | --- |
| No samples, a sample rate of 0, a channel count of 0 | An error |
| A sample that is not a finite number | An error |
| Silence, a constant value, white noise | No tempo and no key |
| A track shorter than eight beats of the slowest tempo of the range | No tempo |
| A range outside 20 to 400 BPM, or one whose top is less than 1.5 times its bottom | An error |
| A track longer than 20 minutes | Only its first 20 minutes are read |
| The grid is edited or locked while the track is being analysed | The grid stays as the user left it |
| Another track is loaded, or the deck is ejected, while it is being analysed | The result is dropped |
| A load that fails while the track in the deck is being analysed | The track in the deck still gets its result |
| A reset before the analysis has finished, or with the grid locked | Nothing happens |
| A reset of the tempo when no tempo was found | Nothing happens |
| A reset of the grid when no tempo was found | The grid is left without tempo and markers |

## Tests

`crates/analysis/tests/detection.rs`, on synthesised signals: drum patterns at known tempos and
offsets, chord progressions in known keys, and the inputs of the edge cases. The measurements
behind the method are in `docs/decisions/0020-own-analysis.md`. The analysis on load, the
resets and the lock: `crates/engine/tests/analysis.rs`, on drum patterns written as WAV files.
