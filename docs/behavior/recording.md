# Recording

The mix recorder: it writes what the master output plays into audio files, counts the time
of the recording and cuts it into a new file on demand. M4 delivers the recorder and its
indicator in the status header, and the loop recorder (*Loop recorder*, below). Broadcasting
is a later item of `docs/parity.toml`. How the samples travel from the audio
thread to the disk: `docs/decisions/0035-mix-recorder.md`.

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

**Indicator.** While a recording runs, the status header shows a red dot and the label *Rec*
after the MIDI indicator. Without a recording it is not drawn. The design system places it
there (`docs/design/DESIGN.md`, *StatusHeader*); the public manual does not describe one. The
time stays in the panel: the header has no room for it (*Differences*).

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
use takes the next number. *Recordings*, last in the tree of the browser, shows the folder
(`docs/behavior/browser.md`, *Recordings*).

**Split.** A file is closed and the next one opened, as a cut does, when it reaches the split
size of the preferences, 2048 MiB by default, the largest split the public manual offers. A WAV file stores its size in 32 bits and cannot
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

**Preferences.** The Recording page of the preferences (`docs/design/DESIGN.md`,
*Preferences: Recording and Layouts pages*) holds, in its section *Mix recorder*:

- *Folder*: the folder of the recordings. *Use explorer folder* (`recorder.folder.set`) takes
  the folder the explorer of the browser shows, as *Add* of the music folders does; *Default*
  (`recorder.folder.reset`) goes back to the folder of *Files*. *Recordings* in the tree shows
  the folder chosen.
- *Prefix* (`recorder.prefix.set`): the start of the name of each file, `mix` by default, then
  a dash and the moment, as in *Files*. Enter keeps it. Characters a file name cannot hold
  (`< > : " / \ | ? *` and control characters) are left out, and spaces at either end; at most
  32 characters are kept. An empty prefix is `mix`.
- *Split at* (`recorder.split.set`): 512, 1024 or 2048 MiB.

The three are kept in the settings file under `[recorder]` and apply from the next recording:
a recording that runs keeps its folder, its name and its split size until it stops.

## Differences

- No gain knob and no level meter in the panel: the design system draws neither, and the
  recording is the master output at its own level, which the master meter of the header
  already shows. The *level* of the inventory item is therefore the master level; a gain of
  its own is a question for the maintainer.
- **The external source is an input routing.** `recorder.source.set` `internal` records the
  master, `external` the input routed to the recorders on the Input page
  (`input.routing.set` with the target `recorder`, `docs/behavior/live-input.md`), where the
  source is chosen too; the input is recorded as it arrives, without the master volume or the
  limiter.
- Three split sizes instead of any size up to 2048 MB: the manual gives the largest only, and
  three steps of a segmented choice are quicker to set than a number.
- The folder is taken from the explorer of the browser, as the music folders are: Platter has
  no dialog of the system to pick a folder.
- The source is the internal mix unless the external one is chosen.
- The display shows the time, not the size of the file.
- In external mixer mode the internal mix is the one the collapsed mixer of that mode does not
  show; the external source records the hardware mixer through an input.
- Names in UTC: Platter has no source for the local time zone (`docs/behavior/browser.md`,
  *Differences*).
- At 1440 points the status header has no room left for the indicator. The master clock is
  centred in what the indicators leave and, when it is wider, runs to the right: without a
  recording it already covers part of the label *Master*, and with the indicator, though it
  shows no time, it covers the whole label and reaches the edge of the master meter. Captures
  of the four-deck state made on 18 November 2026 show it; where the indicators should give
  way waits for the maintainer.

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
| A file reaches the split size | It is closed and the next part opened, with no frame lost |
| A prefix of spaces or of characters a file name cannot hold | `mix` |
| A split size in the settings file other than 512, 1024 or 2048 | 2048 |
| A folder in the settings file that no longer exists | Kept; the first recording creates it, or fails as a folder that cannot be created does |
| The folder or the prefix changed while recording | The recording goes on as it started; the next one uses the new ones |
| A change of sample rate while recording | The engine is built again at the new rate: the recording ends and its file is finished; Rec starts a new one |
| A change of device at the same rate | When the stream hands the processor back, the recording goes on and the frames the device did not ask for while it changed are not in the file; when it does not, the engine is built again and the recording ends as on a change of rate. No test covers it |
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
- `crates/engine/tests/recorder.rs`, `the_split_size_set_closes_a_file_at_that_size`: the split
  size of the preferences closes the first file at that size, with no frame lost.
- `crates/ui-egui/tests/settings_mixer.rs`: the Recording page takes the explorer folder, sets
  the split size and keeps a typed prefix on Enter; `apps/platter/src/config.rs`: the three
  are kept in the settings file, and a prefix or a size that cannot be used is the default.

# Loop recorder

The loop recorder records a loop of a few bars from the mix or from one deck, plays it back
over the mix and records more on top of it, with undo and redo of the last layer.

## What the public manual says

Sections of the public manual, read on 21 November 2026:

- *Overview*, *Global Section*, *Loop Recorder*
- *Preferences*, *Loop Recorder page*

**Panel.** The loop recorder shares the left side of the global section with the first effect
unit. It records loops on the fly from the main output of the mixer, from one deck or from an
external input. Its controls:

- a dry / wet knob, the ratio of the main signal to the recorded one;
- a progress bar, the current length of the recording;
- a size key, the initial length of the recording;
- Delete, which removes the recorded loop; Undo, which undoes the last overdub; Redo, which
  brings back what Undo removed;
- a play key, which starts and stops the playback of the recording;
- a record key, which starts and stops the recording and the overdubbing;
- a menu of the source.

With an external mixer the only source is the external input.

**Preferences.** *Rec. Latency* corrects the latency of the recording in external mixer mode.
*LoopDecay* sets, as a percentage, how long recorded audio takes to fade out while overdubbing.

The sections read do not say when a recording starts, which sizes exist, whether the loop
follows the tempo after it is recorded, what the ends of the dry / wet knob do, or what 0 and
100 percent of the decay mean.

## What Platter does

**Panel.** In the middle panel of the effect row, left of the mix recorder, as the design system
draws it (`docs/design/DESIGN.md`, *Loop recorder (M4)*): the source, the size, Rec, play,
Undo, the dry / wet knob and the progress bar under them. Redo and Delete are in the context
menu of Undo, opened with the right button or Shift+F10, and have keys of the keyboard:
Ctrl+Z undoes, Ctrl+Y redoes and Ctrl+Delete deletes. Rec is lit in the error colour while it
waits for the beat, records or overdubs; play is lit while the loop plays. The bar fills as
the loop plays, in the error colour during the first pass. Undo keeps its English name in
Spanish, as Rec does: the key is 32 points wide.

**Size and tempo.** 1, 2, 4 or 8 bars of four beats (`loop_recorder.size.set`), 4 by default,
at the tempo of the master clock when Rec is pressed. The clock always has a tempo: that of the
tempo master deck or its own (`docs/behavior/sync.md`). The size is chosen while nothing is
recorded. The longest loop is 32 seconds, 8 bars at 60 beats per minute; at a slower tempo a
size that does not fit is halved until it does. The loop keeps its length when the tempo
changes later: it is not stretched.

**Record.** Rec (`loop_recorder.record`) on an empty recorder waits for the next beat of the
clock and records one pass of the size; at the end of the pass the loop plays. Rec pressed
during the first pass ends it: the rest of the loop stays silent and it plays. Rec pressed
while the loop plays starts an overdub, recording on top of the loop where it is, and pressed
again ends it. The overdub does not start by itself after the first pass: with the mix as the
source it would add the mix to the loop on every pass. Rec pressed while it waits for
the beat cancels the wait.

**Play.** Play (`loop_recorder.play`) pauses the loop where it is and plays it again from there.
Pausing ends an overdub. Rec on a paused loop plays it and overdubs.

**Undo and redo.** Each overdub, from Rec to Rec, is a layer. Undo (`loop_recorder.undo`)
removes the last layer, ending an overdub that runs; Redo (`loop_recorder.redo`) puts it back.
One layer is kept: a new overdub after an undo replaces what Redo would bring back, and Undo
after Undo does nothing. The first pass is not a layer: Delete removes it.

**Delete.** Delete (`loop_recorder.delete`) stops the loop and empties the recorder; the size
can be chosen again.

**Source.** `Master` (default), the mix of the decks before the master volume, or one deck,
`Deck A` to `Deck D`, after its equaliser, filter and effects and before its fader, so a deck
heard only in the headphones can be recorded (`loop_recorder.source.set`), or `External`, the
input routed to the recorders on the Input page (`docs/behavior/live-input.md`). The source can
change during an overdub. The loop is never recorded into itself: the source is taken before
the loop is added to the mix.

**Where the loop plays.** The loop is added to the mix of the decks before the master volume, so
the master volume, the maximiser and the safety limiter act on it and the mix recorder records
it. The dry / wet knob (`loop_recorder.dry_wet.set`, 0 to 1, 0.5 by default) sets both: from
0 to 0.5 the loop rises to full level with the mix at full level, from 0.5 to 1 the mix falls to
silence. The knob acts only while the loop plays; with nothing playing the mix is at full level.

**Decay.** *Overdub decay* on the Recording page (`recorder.loop_decay.set`, 0 to 100 percent,
0 by default) is how much of what the loop held is taken away on each pass of an overdub: 0
keeps it all, 50 halves it on every pass, 100 replaces it with what the overdub records. It is
kept in the settings file under `[recorder]`.

**Without clicks.** The loop fades in and out over 5 ms at its seam, when it starts and stops
playing and around an undo or a redo; an overdub fades its input in and out over 5 ms.

## Differences

- Rec waits for the next beat instead of starting at once, so the loop lies on the beats of the
  mix and repeats in time.
- No *Rec. Latency*. In external mixer mode the panel is disabled and Rec is refused, the
  external source too, since the mix the loop plays into is not heard: the manual makes the
  external input the only source there, and where its loop is heard is a question for the
  maintainer.
- One layer of undo, as the manual describes Undo as undoing the last overdub.
- The loop is not stretched when the tempo changes; nothing in the sections read says it is.
- The size is a menu of four lengths, as the design system draws it.

## Edge cases

| Input | Result |
| --- | --- |
| Play, Undo, Redo or the knob with nothing recorded | Nothing; the keys are disabled |
| Undo with a loop but no overdub to undo | Nothing; Undo stays enabled so its menu opens |
| Size while a loop is recorded or Rec waits | Refused; the key is disabled |
| 8 bars at 50 beats per minute | 4 bars, 19.2 seconds |
| Undo right after an overdub starts, before the previous state is copied | Done as soon as the copy ends, within a few blocks |
| Redo after a new overdub | Nothing: the new overdub replaced the undone layer |
| A source deck that is empty | Silence is recorded |
| Rec in external mixer mode | Refused |
| A new audio device | The engine is built again and the loop is lost |

## Tests

- `crates/engine/tests/loop_recorder.rs`: a pass waits for the beat and has the length of the
  size at the tempo, the loop plays and overdubs, decay takes away what the note says, undo
  and redo, delete, a size that does not fit, the refused commands.
- `crates/engine/tests/realtime_memory.rs`: record, overdub, undo, redo and delete in `process`
  neither allocate nor free.
- `crates/ui-egui/tests/panels.rs`: the keys of the panel emit their commands and are
  disabled as above; `crates/ui-egui/tests/settings_mixer.rs`: the decay on the Recording page.
