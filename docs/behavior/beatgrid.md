# Beatgrid

The tempo of a track in beats per minute and the grid markers that place its beats in time.
Sync, quantized jumps, snapped cues and loops all read the grid, so it comes first in M3. This
note covers the grid itself, the manual editing of it and the beat tick. Detection by analysis
and the reset to the analysed values are in `docs/behavior/analysis.md`.

## What the public manual says

Sections of the public manual, read on 14 November 2026:

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

The GRID page also has a beat tick: an audible tick on each beat, a metronome to check the grid
while the deck plays. With the internal mixer it is heard only in the headphones; with an
external mixer it is on the signal of the deck, so the manual advises using it only while
pre-listening.

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
  field, lower and raise the tempo by 0.01 BPM, halve, double, tap, move the marker earlier
  or later by 5 ms, and the lock at its end, lit while the grid is locked. The second: the
  previous marker, set a marker, the next marker and delete.
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

### Beat tick

`deck.tick.toggle` turns the beat tick of a deck on and off. While it is on and the deck plays,
each beat of its grid that the play position passes plays a tick in the headphones: a 1.5 kHz
sine of 30 ms that decays from 12 dB below full scale, one per deck, so two decks tick each on
its own beats.

- The beats are those of the grid the waveform draws: each marker is a beat, and a marker off
  the beats of the run before it ticks too.
- The position is the one heard: with key lock, the position the stretch plays; on a detour of
  flux, the position of the detour, not of the timeline the deck comes back to; in reverse,
  the beats the position passes going back.
- A jump (a seek, a cue, a beatjump, the return of a loop) ticks only when it lands on a beat,
  within the frames the deck plays in one step: a loop that starts on a beat ticks on each
  pass, a seek to the middle of a beat does not tick until the next one.
- The tick joins the headphones after the mix between pre-listen and master and before their
  volume and limiter, so it is heard whatever the mix knob says and follows the volume.
- It is never on the master. A device without a pair for the headphones, two channels, does
  not play it in internal mode. In external mixer mode it is on the pair of its deck.
- It stays on for the next track and when the audio device changes, as key lock does.

## Differences

- One tempo per track in M3: the markers re-anchor the beats, they do not start a new tempo.
  The tempo buttons act on the whole track, not on the zone of the play position.
- The step buttons of the documented application add or remove one beat in a closed zone; with a
  single tempo Platter steps the tempo by the amount the command carries.
- The tempo field cannot be typed into yet: it is dragged or stepped. Typing comes with the
  text field of the browser search, later in M3.
- The reset commands are in `docs/behavior/analysis.md`; their keys on the GRID page come later
  in M3.
- The beat tick has its command but no key on the GRID page yet; the key comes with the next
  change to the deck panel.
- The sound of the tick, its level and its length are Platter's: the manual describes none of
  them.
- In external mixer mode the tick is on the signal of the deck, on its pair of outputs, as
  the manual says (`docs/behavior/external-mixer.md`).
- The selected marker is the one on or before the play position, as the documented application
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
| A new track | Its grid starts empty and unlocked, until the analysis fills it (`docs/behavior/analysis.md`) |
| Beat tick on a deck without a grid tempo, paused, empty or loading | No tick |
| Beat tick with two output channels | No tick in internal mode: there are no headphones; in external mode it is on the shared pair |
| Beat tick while the deck plays through a beat with its fader down or not pre-listened | Ticks: the tick does not go through the channel |

## Tests

`crates/core/src/grid.rs` for the grid itself and its beats; `crates/engine/tests/beatgrid.rs`
for the commands; the tap in `crates/engine/src/tap.rs`; the GRID page in
`crates/ui-egui/tests/panels.rs`, which operates each of its controls and checks that every
grid command has one. `crates/engine/tests/beat_tick.rs` for the beat tick: the onsets of the
ticks against the beats of the grid at tempos 1 and 1.08 and with key lock, a marker off the
beats, a loop, a seek, reverse, pause, a grid without tempo, two channels, the master unchanged,
the next track and a new device rate.
