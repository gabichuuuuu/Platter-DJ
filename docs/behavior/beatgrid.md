# Beatgrid

The tempo of a track in beats per minute and the grid markers that place its beats in time.
Sync, quantized jumps, snapped cues and loops all read the grid, so it comes first in M3. This
note covers the grid itself and the manual editing of it; detection by analysis, the reset to
the analysed values and the beat tick come later in M3.

## What the public manual says

Sections of the reference manual, read on 14 November 2026:

- *Global concepts*, section *Beatgrid*
- *Advanced usage tutorials*, section *Adjusting beatgrids*
- *Overview*, section *Advanced panel*, the GRID page

A beatgrid is made of one or more grid markers; each sets the tempo up to the next one, so a
track can hold several tempos. The analysis sets a single marker with one tempo; the user can
add, select, move and delete markers and change the tempo. Cue points are independent of the
grid. The GRID page has buttons to move the selected marker backward and forward, to go to the
previous and the next marker, to set a marker at the play position and to delete the marker on
or before it; a tempo display that can be typed into or dragged; a tap button that sets the
tempo from the fourth tap on; buttons to double, halve, lower and raise the tempo; and a lock
that keeps the analysis and the user from changing anything. A tutorial uses several markers on
one tempo to follow a live recording whose beat drifts.

## What Platter does

A grid has a tempo, from 20 to 400 BPM or none yet, up to 128 markers and a lock. Each marker is
a position in seconds of the track and stands on a beat; the beats follow from the last marker on
or before them, and before the first marker from the first one, at the one tempo of the grid.
Several tempos along one track are M4.

| Command | Effect |
| --- | --- |
| `deck.grid.bpm.set` | Sets the tempo, 20 to 400 BPM |
| `deck.grid.bpm.scale` | `factor` `double` or `half`: doubles or halves the tempo |
| `deck.grid.bpm.step` | Adds `amount` BPM to the tempo, -10 to 10 |
| `deck.grid.tap` | One tap; see below |
| `deck.grid.marker.set` | Sets a marker at the play position |
| `deck.grid.marker.move` | Moves the marker on or before the play position by `seconds`, -1 to 1 |
| `deck.grid.marker.delete` | Deletes the marker on or before the play position |
| `deck.grid.marker.previous` | Jumps to the marker before the play position |
| `deck.grid.marker.next` | Jumps to the marker after the play position |
| `deck.grid.lock.toggle` | Locks the grid, or unlocks it |

- The play position is the one the deck has once the commands before are applied: a seek and a
  marker set in a row put the marker where the seek landed, as with the cue point
  (`docs/behavior/transport.md`).
- A tempo set on a grid without markers also sets a marker at the start of the track, so the
  beats have a place. A grid whose last marker is deleted keeps its tempo but has no beats
  until a marker is set or the tempo is set again.
- A marker set after a seek past the end of the track goes on the end.
- A position within 1 ms of a marker counts as on it. A new marker that close to another is not
  added; previous and next skip the marker the deck stands on.
- A moved marker stays inside the track and at least 1 ms from its neighbours, so the markers
  keep their order.
- While the grid is locked every grid command except the lock itself and the jumps to markers
  changes nothing.

### In the interface

- The scrolling waveform draws each beat as a faint line behind the waveform and each downbeat
  and marker as a brighter line over it. A marker is a downbeat and every fourth beat after it
  is the next one; before the first marker the bars count back from it. The overview shows no
  grid.
- The GRID page of a deck is two rows above its transport keys, shown when the switch of
  the advanced panel is on GRID (`docs/behavior/loops.md`). The first: the tempo
  field, halve, double, tap, move the marker earlier or later by 5 ms, and the lock at its
  end, lit while the grid is locked. The second: lower and raise the tempo by 0.01 BPM, then
  the previous marker, set a marker, the next marker and delete.
- The tempo field shows the tempo of the grid, or `--.--` without one. Dragging it up or down
  sets the tempo, 0.05 BPM per pixel, a tenth of that with Shift; with the focus the arrows
  step it by 0.01 BPM and Page Up and Page Down by 1 BPM. Without a tempo in the grid the
  field starts from the tempo of the tag, or 120 BPM.
- The tempo in the deck header is the tempo of the grid; until the grid has one, the tempo of
  the tag.
- Controls that would change nothing are disabled: the edits while the grid is locked, the
  tempo keys without a tempo, the marker keys without markers.

### Tap

Each tap is timed on the control side when the command arrives. From the fourth tap on, the
tempo is 60 divided by the mean of the intervals between the last taps, up to eight of them. A
pause longer than two seconds starts a new count. A tapped tempo outside 20 to 400 BPM changes
nothing. Taps use the clock of the computer, not the time of a render script, so a script
that taps sets no tempo; it sets the tempo with `deck.grid.bpm.set`.

## Differences

- One tempo per track in M3: the markers re-anchor the beats, they do not start a new tempo.
  The tempo buttons act on the whole track, not on the zone of the play position.
- The step buttons of the reference product add or remove one beat in a closed zone; with a
  single tempo Platter steps the tempo by the amount the command carries.
- The tempo field cannot be typed into yet: it is dragged or stepped. Typing comes with the
  text field of the browser search, later in M3.
- The CUE page comes with the hotcues. The reset buttons and the beat tick come with the
  analysis.
- The selected marker is the one on or before the play position, as the reference product
  selects it. Platter has no separate selection.
- The grid lives with the loaded track and is lost when the track is loaded again, which a
  change of sample rate of the audio device also does, until the collection stores it later
  in M3.

## Edge cases

| Input | Result |
| --- | --- |
| Any grid command on an empty deck | Nothing happens |
| Tempo or step not a number, tempo outside 20 to 400, step outside -10 to 10, move outside -1 to 1 | The command is rejected |
| Double, halve or step that would leave 20 to 400 BPM | Nothing changes |
| Double, halve or step with no tempo yet | Nothing changes |
| A 129th marker | Not added |
| Move, delete or previous before the first marker; next after the last | Nothing happens |
| A new track | Its grid starts empty and unlocked |

## Tests

`crates/core/src/grid.rs` for the grid itself and its beats; `crates/engine/tests/beatgrid.rs`
for the commands; the tap in `crates/engine/src/tap.rs`; the GRID page in
`crates/ui-egui/tests/panels.rs`, which operates each of its controls and checks that every
grid command has one.
