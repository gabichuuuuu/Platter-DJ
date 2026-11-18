# Recording

The mix recorder: it writes what the master output plays into audio files, counts the time
of the recording and cuts it into a new file on demand. M4 delivers the recorder and its
indicator in the status header. The loop recorder, recordings in the tree of the browser and
broadcasting are later items of `docs/parity.toml`. How the samples travel from the audio
thread to the disk: `docs/decisions/0034-mix-recorder.md`.

## What the public manual says

Sections of the public manual, read on 18 November 2026:

- *Advanced operation*, the section on recording the mix with the audio recorder and its
  subsections on its preferences, on the recording level and on recording
- *Preferences*, the page of the mix recorder

**Panel.** The recorder has a panel in the global section, opened by its own key. It holds a
level meter of what is recorded, a gain knob for the recording level, a record key, a cut key
and a display of the size of the current file and of the time recorded. When the meter does
not move, the manual points at the channel fader and the crossfader: the recorder takes what
the mix sends out.

**Record and stop.** The record key starts the recording; pressed again, it stops it and the
file is saved.

**Cut.** The cut key ends the current file and goes on in a new one at once, without a gap in
the recording. The recorder does the same by itself when the file reaches the size the
preferences set.

**Preferences.** The page of the mix recorder chooses:

- the source: the internal mix, which is the master output, or an external input such as a
  turntable, a microphone or a hardware mixer, with the input to record;
- the folder the files go to;
- a prefix put in front of the name of each file;
- the size at which a file is split, at most 2048 MB.

The sections read name no file format, no default folder, no rule for the names beyond the
prefix and no indicator in the header, and they do not say whether the time shown starts again
after a cut.

## What Platter does

**Keys.** The middle panel of the effect row holds the recorder, as the design system draws it
(`docs/design/DESIGN.md`, *Effect row*): the label *Mix recorder* over Rec, the time recorded
and Cut.

- Rec (`recorder.toggle`) starts a recording and, pressed again, stops it. Its strip is lit in
  the error colour while it records.
- Cut (`recorder.cut`) closes the file being written and opens the next one. No frame is lost
  or doubled between the two files: the second starts with the frame after the last one of the
  first. Cut is disabled while nothing is recorded.
- The time reads hours, minutes and seconds (`00:42:17`) in a block of fixed width. It counts
  the frames the master output played since Rec, those the disk could not take included, so
  it is the length of the set. A cut does not set it back to zero. After a stop it keeps the
  length of the last recording until the next one starts.

**Indicator.** While a recording runs, the status header shows a red dot, the label *Rec* and
the same time, after the MIDI indicator. Without a recording it is not drawn. The design system
places it there (`docs/design/DESIGN.md`, *StatusHeader*); the public manual does not describe
one.

**What is recorded.** The master output after the master volume and the safety limiter, in
stereo, whatever the number of channels of the device: the mix the first pair of outputs
plays in the internal mode. The test tone, which goes to the master, is recorded too; the
headphones, the metronome, the beat ticks and the preview player are not. In external mixer
mode the engine still makes the internal mix of the decks, and the recorder records that, not
what the external mixer makes (*Differences*).

**Files.** WAV with 32-bit float samples at the sample rate of the engine, so the file holds
the samples of the output exactly. The folder is `Platter DJ Recordings` inside the music
folder of the user (`Music` in the home folder) or, when the home folder has none, inside the
home folder itself; without a home folder it is the folder `Recordings` beside the settings
file. It is created by the first recording. Each recording is named by the moment it started,
in UTC, as the backups of the collection are (`mix-20261118-214105.wav`); the files after a
cut add a part number (`mix-20261118-214105-2.wav`). A file is never written over: a name in
use takes the next number.

**Split.** A file is closed and the next one opened, as a cut does, when it reaches 2048 MiB,
the largest split the public manual offers. A WAV file stores its size in 32 bits and cannot
pass 4 GiB; at 48 kHz a split comes every 93 minutes.

**Stop.** A stop ends the recording at the end of the block the processor plays after it,
then the file is finished: its header gets the sizes. The application stops a running
recording when it closes and waits up to two seconds for the file to be finished.

**Problems.** A folder that cannot be created, a file that cannot be opened or written, a full
disk: the recording stops, the reason goes to the log, the time turns to the warning colour
and the tooltip of Rec gives the reason until the next recording starts. What was written
before the problem stays in the file, finished as far as the disk allows. When the disk falls
behind for more than four seconds, the frames it could not take are counted, not waited for:
the file then lacks them, the time turns to the warning colour and the log says how many were
lost when the recording ends.

## Differences

- No gain knob and no level meter in the panel: the design system draws neither, and the
  recording is the master output at its own level, which the master meter of the header
  already shows. The *level* of the inventory item is therefore the master level; a gain of
  its own is a question for the maintainer.
- No external source: Platter has no audio input until M5 (input routing).
- No preferences page for the recorder: the folder, the names and the split size are fixed
  (*Files*, *Split*). The page of the inventory (`preferences`, `recorders`) stays planned.
- The display shows the time, not the size of the file.
- In external mixer mode the file holds the internal mix, which the collapsed mixer of that
  mode does not show; recording the external mixer needs an input.
- Names in UTC: Platter has no source for the local time zone (`docs/behavior/browser.md`,
  *Differences*).
- At 1440 points the status header has no room left for the indicator. The master clock is
  centred in what the indicators leave and, when it is wider, runs to the right: without a
  recording it already covers part of the label *Master*, and while recording it covers the
  start of the master meter too. The captures of 18 November 2026 show it; where the
  indicators should give way waits for the maintainer.

## Edge cases

| Input | Result |
| --- | --- |
| Rec without a folder for the recordings (no home folder and no settings folder) | The command is refused and logged; nothing changes |
| Rec while the last recording is still being finished | Refused as busy; a recording takes a block and the writing of its last frames to finish |
| Cut while nothing is recorded | Nothing |
| Two cuts within one second | Two files, each with its own part number |
| A name already used in the folder | The next part number |
| The folder cannot be created or the file opened | The recording stops at once; the reason is shown and logged |
| The disk fails during a recording | The recording stops; the file keeps what was written |
| The disk is slower than the audio for more than four seconds | The frames it cannot take are counted and left out |
| A file reaches 2048 MiB | It is closed and the next part opened, with no frame lost |
| A change of sample rate while recording | The engine is built again at the new rate: the recording ends and its file is finished; Rec starts a new one |
| A change of device at the same rate | The recording goes on; the frames the device did not ask for while it changed are not in the file |
| No audio device, and no silent output either | The processor does not run, so no frame is recorded; a stop finishes the file after two seconds |
| The application closes while recording | The recording is stopped and its file finished before the window goes |

## Tests

- `crates/engine/tests/recorder.rs`: a recording with a cut gives two files whose frames,
  read back, are exactly the frames the processor sent to the master, the time counts them,
  a stop finishes the header, a folder that cannot be created reports its reason and stops
  the recording, an engine dropped while recording still finishes its file, and the
  commands are refused or ignored as the table says.
- `crates/engine/src/recorder.rs`: the split at a size, with no frame lost between the parts,
  the part numbers and the frames of another recording left in the queue being skipped.
- `crates/engine/src/rt/recorder.rs`: a full queue counts the frames it drops and delivers
  the marks of the start and the end before any later frame.
- `crates/engine/tests/realtime_memory.rs`: recording, a cut, a stop and a full queue in
  `process` neither allocate nor free.
- `crates/ui-egui/tests/panels.rs`: Rec and Cut emit their commands, Cut is disabled without a
  recording, the indicator of the header shows only while recording, in both languages and
  without overlaps.
