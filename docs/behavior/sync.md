# Sync, tempo master and master clock

Sync makes a deck play at the tempo of another one, or of a clock, without the DJ matching it
by hand. This note covers the tempo master, the master clock, the sync of a deck in its two
modes, the phase meter, and Snap and Quantize, which put cues, loops and jumps on the beat.
The beatgrid all of them read is in `docs/behavior/beatgrid.md`.

## What the public manual says

Sections of the reference manual, read on 14 November 2026:

- *Global concepts*, section *Tempo synchronization*
- *Overview*, sections *Synchronization and tempo controls* and *Master control panel*

There is one tempo master at a time, a deck or the master clock, and decks can be synced to
it. With the automatic mode off, the MASTER button of a deck makes that deck the tempo master
and the MASTER button of the master panel makes the clock the tempo master. With the automatic
mode on, a deck that starts to play becomes the tempo master; when it stops or its track ends,
a synced deck takes over, and when there is none the clock does. The clock follows the deck
that is the tempo master.

A synced deck cannot have its tempo changed by hand: only the tempo master changes it. Sync
has two modes, a preference. In the first, tempo and phase stay locked through every action
on the transport, a beatjump included. In the second only the tempo is locked: the phase
meter shows the offset, and bending the tempo while the deck plays corrects it. The phase
meter of a deck shows how far its beats are from those of the tempo master.

The master panel shows the tempo of the clock, which can be typed, and has a tap button,
buttons that bend the clock slower or faster for a moment, and buttons that step its tempo
down and up. Everything sync does depends on the beatgrid of the track, not on its cues.

With SNAP on, cue points and loops are set to the closest beat. With QUANT on, a jump to a
cue point or a stored loop keeps the track in sync.

## What Platter does

The master clock has a tempo and counts beats; it starts at 120 BPM. The tempo master is the
clock or one deck. A deck has a tempo to sync with when its beatgrid has one: that tempo times
the speed of the deck.

| Command | Effect |
| --- | --- |
| `deck.sync.toggle` | Switches the sync of the deck on or off |
| `deck.master.set` | Makes the deck the tempo master, while the mode of the clock is not `auto` |
| `clock.mode.set` | `mode` `auto` or `master`: the tempo master is chosen automatically, or is the clock |
| `clock.tempo.set` | `bpm` 20 to 400: sets the tempo of the clock |
| `clock.tempo.step` | `amount` -10 to 10: adds BPM to the tempo of the clock |
| `clock.tap` | One tap; the fourth one in a row sets the tempo of the clock |
| `clock.bend` | `amount` -0.5 to 0.5: the clock runs that fraction slower or faster until it is set back to 0 |
| `global.sync.mode.set` | `mode` `tempo` or `beat`: what sync locks |
| `global.snap.toggle` | Switches Snap on or off |
| `global.quantize.toggle` | Switches Quantize on or off |

### The tempo master

- The mode of the clock starts as `auto` and the sync mode as `beat`.
- In `auto`, while the clock is the tempo master, a deck that starts to play becomes the tempo
  master. When the deck that is the tempo master stops, reaches the end of its track or loses
  its track, the other deck takes over if it plays and its sync is on; otherwise the clock
  does, at the tempo it had.
- A deck counts as playing here when it plays, has a track with a tempo in its grid, and is
  not held playing by a cue key or a hotcue.
- `clock.mode.set` to `master` makes the clock the tempo master. From then on
  `deck.master.set` makes a deck the tempo master, when it has a track with a tempo, and it
  stays so while it is paused. A deck that loses its track hands over to the clock.
- `clock.mode.set` to `auto` keeps a tempo master that plays; otherwise deck A, or else deck
  B, takes over if it plays, or else the clock.
- While a deck is the tempo master the clock follows it: the clock has the tempo of the deck,
  bend included, and, while the deck plays, its beats. A tempo master that is paused, or
  held playing by a cue key or a hotcue, leaves the clock counting on at its tempo.
- The commands that set, step, tap and bend the tempo of the clock act only while the clock is
  the tempo master.
- Tap works as the tap of the grid does (`docs/behavior/beatgrid.md`), with a count of its
  own.

### Sync of a deck

- A deck whose sync is on, and that is not the tempo master, plays at the tempo of the clock:
  its speed is the tempo of the clock divided by the tempo of its grid. `deck.tempo.set` does
  not move it meanwhile.
- When the sync goes off, or the deck becomes the tempo master, the deck keeps the speed it
  has. The next `deck.tempo.set` sets it as always.
- Sync can be on for a deck without a track or without a tempo; it follows once it has both.
- Bend (`deck.nudge`) still acts on a synced deck.
- In the mode `tempo` nothing else happens: the beats of the deck stay as far from those of
  the clock as they are.
- In the mode `beat` the deck also keeps its beats on those of the clock while it plays. An
  offset of more than 1/32 of a beat is closed at once: the deck moves by that offset, at most
  half a beat either way, with the crossfade of a beatjump (`docs/behavior/loops.md`). That
  is what happens when the sync goes on, when the deck starts, after a seek or a jump, and
  when a bend ends. A smaller offset is closed by playing up to 2 % slower or faster.
- The phase is left alone while the deck is bent, while a cue key or a hotcue holds it
  playing, so that it plays from its cue, and while a loop is on that is not a whole number
  of beats long: such a loop cannot stay on the beat.
- The move stays inside the run of beats of the marker the deck is in
  (`docs/behavior/beatgrid.md`) and inside the loop that is on.
- The tempo master is left alone too: its beats are the reference.

### Snap

- Snap starts off. While it is on, a cue or a loop that is set lands on the beat of the grid
  of the deck nearest to where it would be set: the floating cue (`deck.cue.set` and the cue
  key on a paused deck), a hotcue stored into an empty slot, `deck.cue.store`, In, Out and the
  start of a loop from `deck.loop.toggle`.
- A paused deck whose floating cue is set this way also moves to it, so the next press of the
  cue key plays from there.
- An Out that lands after the play position lets the deck play on into the end of the loop.
  One that lands before it sends the deck as far into the loop as it is past the end, so it
  stays on the beat.
- The nearest beat is taken in the run of beats the position is in: it is never past the next
  marker. A beat before the start of the track or after its end is not taken; the beat on the
  other side is.
- A deck without a tempo in its grid places them where they are, as without Snap.
- Moves of a loop or of its ends, a change of the size of a loop and the markers of the grid
  are not snapped: they keep their own steps.

### Quantize

- Quantize starts off. While it is on, a playing deck that goes to a stored cue (a hotcue,
  `deck.cue.select`) lands as far into the beat as it was before: at the cue plus the offset
  of the play position in its beat, from half a beat before the cue to half a beat after it.
- A stored loop is landed in from its start up to one beat after it, so the deck is inside
  the loop. A loop shorter than a beat is gone to at its start.
- The offset is measured with the grid of the deck, so it keeps the deck on the beat of the
  tempo master only when the deck was on it.
- A paused deck, and a deck without a tempo, go to the cue itself.
- A beatjump already keeps the place in the beat; a seek, a click in a waveform or a jump to
  the floating cue are not quantized.

### What the snapshot shows

- For the clock: its mode, the tempo master, its tempo, the sync mode, Snap and Quantize.
- For each deck: whether its sync is on, its speed as a ratio of the original, and its phase:
  how far its beat is from the beat of the clock, from -0.5 (behind) to 0.5 (ahead) of a
  beat. A deck without a tempo has no phase.

The window has no controls for any of this yet; they come with the interface part of this
unit.

## Differences

- The reference product has four decks and lets a preference keep some of them out of the
  automatic choice. Platter has two and no such preference.
- The design system lists the commands of the clock without one that sets its tempo to a
  value and without one for the sync mode. `clock.tempo.set` and `global.sync.mode.set` are
  added for them.
- The reference manual does not say what Snap does to a deck that is paused. Platter moves
  the deck to its new floating cue, as the cue key would otherwise set it again on every
  press.
- The reference manual does not say where a quantized jump lands. Platter keeps the offset
  of the deck in its beat, the way a beatjump does.
- The sync mode starts as `beat`. That is the choice of Platter; the reference manual was
  not read for its default.
- The reference manual does not say what speed a deck has after its sync goes off. Platter
  keeps the speed, so nothing is heard at that moment.
- The reference manual does not say how the phase is brought back. Platter moves the deck at
  once for an offset a DJ would hear as a flam and corrects the rest by speed, within 2 %.
- A deck needs a tempo in its grid to sync, and until tempo detection arrives later in M3
  that tempo is typed or tapped on the GRID page.
- Synchronisation to an external clock is M5.
- Key lock is not there yet, so a synced deck changes pitch with its speed.

## Edge cases

| Input | Result |
| --- | --- |
| `deck.master.set` while the mode is `auto` | Nothing happens |
| `deck.master.set` for a deck without a track or without a tempo | Nothing happens |
| A tempo of the clock command while a deck is the tempo master | Nothing happens |
| `clock.tempo.step` that would leave 20 to 400 BPM | Nothing changes |
| `bpm`, `amount` out of range or not a number | The command is rejected |
| The tempo master deck is ejected or gets another track | The other deck or the clock takes over, as when it stops |
| Both decks stopped in `auto` | The clock is the tempo master and keeps the last tempo |
| The tempo of the grid of a synced deck is edited | The speed follows at once: the deck stays at the tempo of the clock |
| The tempo of the grid of the tempo master is edited | The clock and the synced deck follow |
| A tempo of the clock that asks more than double speed of a deck | The deck plays at double speed, out of sync, and its phase is left alone |
| `deck.tempo.set` on a synced deck | The value is accepted and has no effect on the speed |
| `deck.tempo.range.cycle` on a synced deck | The range changes; the speed does not |
| The tempo master sits in a loop that is not a whole number of beats | The clock counts on at its tempo and takes the beats of the deck again when the loop ends |
| A run of beats with no room for the move | The deck is not moved until it is in a run that has room |
| Snap on and a position exactly between two beats | The later beat |
| Quantize on and a hotcue near the start of the track with the deck late in its beat | The beat after the cue, so it stays in the track |
| Quantize on and a cue on a marker that is off the beat of the run before it, with the deck early in its beat | The deck lands before the marker, in the run before, so off the beats of the cue's run |
| A change of sample rate of the audio device | Mode, sync mode, Snap, Quantize, tempo of the clock and the sync of each deck are kept; the tempo master is chosen again |

## Tests

`crates/core/src/grid.rs` for the phase of a position, its run and its nearest beat;
`crates/engine/src/rt/clock.rs` for the choice of the tempo master;
`crates/engine/tests/sync.rs` for the commands of sync, with the offset between two decks
measured in frames after a render; `crates/engine/tests/snap.rs` for Snap and Quantize.
