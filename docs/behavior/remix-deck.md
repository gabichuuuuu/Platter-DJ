# Remix deck

A remix deck plays short samples, each in a cell of a grid, in time with the master clock: a
loop of drums under a track, a vocal shot on the downbeat. This note covers the deck type, the
grid, how a cell starts and stops and the controls of a slot. How the samples reach the audio
thread, and why the first version leaves parts out, is in `docs/decisions/0042-remix-deck.md`.

## What the public manual says

Sections of the public manual, read on 22 November 2026:

- *Overview*, section *Decks and Deck Types* and section *Remix Deck*, with *Advanced Panel on the
  Remix Deck*
- *Advanced Usage Tutorials*, section *Working with Remix Decks* and section *Creating Remix Sets*
- *Preferences*, section *Remix Decks page*
- *Global Concepts*, section *Tempo Master and Tempo Master Assignment*

Any deck can become a remix deck, by its type or by loading a set of samples into it. A remix
deck has four slots, side by side; each slot has sixteen cells, shown four at a time on four
pages, so a set holds up to 64 samples. Loading a sample into a cell that has one replaces it.

A cell plays its sample once (one-shot) or over and over (loop). By default a cell latches: it
plays on after its key is released, until a one-shot ends or something stops it; in gate mode it
plays only while the key is held. A cell follows the tempo of the deck when its sync is on, from
the tempo the sample has; with sync off it plays at its own speed. A cell also has a gain, a pitch
that needs the key lock of its slot, reverse, a colour and a beatgrid that can be moved.

The deck has a quantize, switched on and given a size in its header; the manual shows a size of
one beat and does not list the others. A cell triggered while quantize is on waits, its key
flashing, until the next boundary of that size and starts there. Triggering another cell of the
same slot stops the one that played at once; the new one starts on the boundary. A cell is
stopped on its own by its key with a modifier, also on the quantize. In punch mode a new cell
starts from the place the previous one had reached instead of from its beginning.

Each slot has a volume after the gain of its cells, a filter, a mute, a key lock, a send to the
effect units, a cue to the headphones and the punch mode. Samples are captured from a playing
deck, the length of its loop, or from the loop recorder; a set is saved in the collection and can
be exported.

The preferences choose whether triggering a cell starts the deck when it is stopped, whether
one-shots ignore the quantize or the punch mode, the auto-gain of samples on loading, and whether
an edited set is saved by itself.

The manual does not list the sizes of the quantize, the ranges of gain and pitch or the longest
sample, and does not say what a cell longer or shorter than the quantize does, what an empty cell
does when triggered, or whether stopping the deck stops its cells.

## What Platter does

`deck.type.set` takes `remix` as a third type beside `track` and `live`. A remix deck plays its
grid in the place of its track, which stays loaded and paused until the deck is a track deck again,
as for a live deck (`docs/behavior/live-input.md`). The change of type fades over a few
milliseconds. A remix deck goes through its mixer channel like any deck: gain, equaliser, effects,
fader and crossfader. It has no tempo of its own and is never the tempo master; it follows the
master clock (`docs/behavior/sync.md`).

The grid has four slots of sixteen cells. A cell holds a sample decoded from an audio file in any
format Platter plays, up to 64 seconds; a longer file is refused. The tempo of a cell comes from
the beatgrid kept for the file in the collection, or from the analysis when there is none, and can
be set by hand; a cell without a tempo plays at its own speed. A cell with a tempo plays at the
tempo of the master clock, its speed and so its pitch following it.

The deck has a quantize of 1, 2, 4, 8 or 16 beats, or none; it starts at one beat. Counted from the
beats of the master clock, a boundary of four beats falls on every fourth beat.

- **Trigger.** A cell starts from its beginning on the next boundary, or on the next frame without
  quantize. The cell the slot played stops at once, with a fade of a few milliseconds. A cell that
  waits can be replaced by another one before it starts. Triggering the cell that plays starts it
  again from its beginning on the boundary.
- **Loop and one-shot.** A loop goes back to its beginning when it ends; a one-shot stops at its end.
  A cell starts as a loop.
- **Stop.** `remix.slot.stop` stops the cell of a slot on the next boundary, with the fade.
- **Slot.** A volume from 0 to 1 and a mute, both smoothed. Commands for a cell that holds no
  sample (its mode, tempo or gain) leave it as it is.
- **Cell gain.** -12 to +12 dB, before the volume of the slot.
- **Latch and gate.** A cell latches by default: it plays on after its key is released. In gate
  mode it plays only while the key is held: `remix.cell.release` stops it at once, with the fade,
  or forgets it while it waits for its boundary. Releasing a latched cell does nothing.
- **Reverse.** A reversed cell plays its sample backwards, from its end: a one-shot stops at its
  beginning, a loop goes back to its end. The change takes effect from the next frame.
- **Punch.** In a punched slot a new loop does not start at its beginning but where it would be had
  it started on the beat the first cell of that run started on, counted in beats at the tempo of the
  clock. The run ends when the slot stops or its cell ends. A one-shot always starts at its
  beginning, as the manual lets a preference ask.
- **Capture.** `remix.cell.capture` copies part of the track of another deck into a cell, on a
  worker thread: the loop that is on or, without one, the size of a new loop of that deck from
  where it plays, at the tempo of its beatgrid. The cell takes that tempo, the name of the track
  and the loop mode. A deck without a track, or with neither a loop nor a tempo, is refused; a part
  that runs past the end of the track is cut there. The track is copied as it is, without its
  stems, its key lock or the effects of its channel.

Every action is a command:

| Command | Effect |
| --- | --- |
| `deck.type.set` | `type` `track`, `live` or `remix`; also chosen on the Input page of the preferences |
| `remix.cell.load` | `deck`, `slot` 1 to 4, `cell` 1 to 16, `path`: decodes the file into the cell |
| `remix.cell.clear` | Empties the cell, stopping it first |
| `remix.cell.trigger` | Starts the cell on the next boundary |
| `remix.cell.mode.set` | `mode` `loop` or `one_shot` |
| `remix.cell.bpm.set` | `bpm` 20 to 400 |
| `remix.cell.bpm.clear` | The cell plays at its own speed |
| `remix.cell.gain.set` | `db` -12 to 12 |
| `remix.slot.stop` | Stops the cell of the slot on the next boundary |
| `remix.slot.volume.set` | `level` 0 to 1 |
| `remix.slot.mute.toggle` | Mutes the slot, or lets it be heard again |
| `remix.cell.trigger_mode.set` | `mode` `latch` or `gate` |
| `remix.cell.release` | The key of the cell is released: a cell in gate mode stops |
| `remix.cell.reverse.toggle` | Plays the cell backwards, or forwards again |
| `remix.cell.capture` | `from`: the deck whose loop is copied into the cell |
| `remix.slot.punch.toggle` | Switches the punch mode of the slot |
| `remix.quantize.set` | `beats` `none`, `1`, `2`, `4`, `8` or `16` |
| `remix.page.set` | `page` 1 to 4: the four cells of each slot the face shows; kept for the session |

The snapshot holds, for each deck, the name, mode, tempo, gain, trigger mode and direction of each
cell, the cell each slot plays and the one it waits to start, and the volume, mute and punch of
each slot.

## Differences

- **The grid on the screen.** A remix deck shows its grid in the place of everything under its
  header (`RemixGrid` in `docs/design/DESIGN.md`): four cells of each slot at a time on four pages,
  the quantize, and the mute, punch, stop and volume of each slot. A cell acts while it is pressed,
  Shift and a press on the cell a slot plays stops the slot, a track dropped on a cell loads into it
  and the context menu of a cell sets its mode, latch or gate and direction, captures from another
  deck and clears it. The tempo and the gain of a cell have no control on the screen yet; they are
  set by commands. An empty cell takes no keyboard focus, so its menu, which offers the captures,
  opens with the pointer only.
- **The master clock is the tempo of the deck.** The manual gives a remix deck a tempo of its own and
  sync to the tempo master; Platter's remix deck has no tempo control and follows the clock, which a
  deck that is the tempo master drives. A remix deck is never the tempo master.
- **No transport.** A triggered cell plays: the deck has no play key to switch on first, which the
  manual makes optional with a preference.
- **Speed follows the tempo, and so does the pitch.** There is no key lock in a slot and no pitch per
  cell yet.
- **Not yet:** the filter, the effect send and the cue of a slot, colours, moving the
  beatgrid of a cell, capture from the loop recorder, and saving or exporting a set. A capture is
  chosen in the menu of the cell, where the manual drags the title of the deck onto the cell. The grid is empty when Platter starts and the type of a remix deck is
  not kept in the settings: it starts as a track deck. A restart of the engine, on a change of
  audio device, empties the grid too; the deck stays a remix deck.
- **Punch counts the run, not the loop before.** The manual starts a punched cell from the place
  the previous sample had reached, fixed to the timeline of the deck. Platter counts the beats since
  the first cell of the run started: the same place while the samples are as long as each other, and
  a place in time with the run when they are not, where the place inside a shorter loop that went
  round would jump back. A run ends when nothing in the slot plays or waits.
- **A boundary counts the beats of the clock**, not bars of a track: with four beats the boundary is
  every fourth beat since the engine started, which is not tied to the downbeat of a deck.

## Edge cases

- Triggering an empty cell does nothing; the slot keeps playing what it played.
- A file that cannot be decoded, or is longer than 64 seconds, leaves the cell as it was and
  reports the error.
- Loading into a cell that plays or waits stops it with the fade, then replaces its sample; the
  new sample starts when the cell is triggered again.
- A one-shot that ends while another cell of its slot waits leaves the slot silent until the
  boundary.
- A tempo of the clock that cannot be used (not finite, zero) plays a synced cell at its own speed.
- The grid of a deck of another type runs on unheard, as the preview player does: a cell started
  before the change of type is in its place, in time, when the deck is a remix deck again.
