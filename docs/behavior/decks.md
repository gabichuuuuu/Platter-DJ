# Decks C and D

Platter has four track decks, A to D, each with its own channel in the mixer. This note covers
what decks C and D add to the engine: the commands that name them, their channels, the
crossfader, the tempo master and the outputs of external mixer mode. What one deck does is in
`docs/behavior/transport.md` and the notes of each function; the channel in
`docs/behavior/mixer.md`.

## What the public manual says

Sections of the public manual, read on 17 November 2026:

- *Overview*, the list of the parts of the window, item *Decks*; section *Mixer*; section
  *Crossfade controls*
- *Preferences*, section *Decks Layout page*, *Deck Layout*
- *Preferences*, section *Output Routing page*, *External Mixing Mode* and *Internal Mixing
  Mode*
- *Global concepts*, section *Tempo Master and Tempo Master Assignment*

The documented application has four decks, A to D. A DJ who plays with two can show the lower
decks C and D with an option of the decks layout page, which also sets their size; a deck that
is hidden is muted. The mixer has four channels, one per deck. Each channel has keys that put it
on the left or the right side of the crossfader; the manual does not give the side each channel
starts on. In the external mixing mode the output routing has a pair of outputs for each of the
four decks, and several decks can share one pair when the audio interface has few outputs. The
automatic choice of the tempo master names no deck: the first deck started takes it, and when
it stops a synced deck takes over. The manual gives no order of the mixer channels and no rule
that only concerns decks C and D.

## What Platter does

- Every command with a `deck` parameter takes `a`, `b`, `c` or `d`, in either case: transport,
  cues, loops, beatjump, sync, key lock, flux, the channel controls of the mixer and loading.
  Decks C and D do everything decks A and B do.
- The mixer has four channels with the same controls. The engine processes the four decks in
  every block; a deck with no track adds silence.
- The crossfader starts with decks A and C on the left and decks B and D on the right. Each
  channel can be moved to the left, the right or past the crossfader
  (`mixer.crossfader.assign`), as before.
- The tempo master is chosen among the four decks by the rules of `docs/behavior/sync.md`; with
  several decks live, the automatic choice keeps the deck that took over first.
- The headphones get the pre-listened channels of the four decks and their beat ticks.
- In external mixer mode the decks of the left column, A and C, go to the master outputs of the
  routing, summed, and the decks of the right column, B and D, to the headphone outputs, summed
  (`docs/behavior/external-mixer.md`). With a device that cannot carry the headphones the four
  decks share the master outputs.
- The snapshot holds the four decks, deck A first.
- The window shows decks A and B, or the four decks in the four-deck layout chosen in the
  preferences (`docs/behavior/interface.md`, *Deck layout*). With two decks shown, decks C and
  D are driven through commands: the command line render script and MIDI mappings.
- The four channels of the mixer go in the order A, B, C, D in the four-deck layout, as the
  maintainer decided on 18 November 2026 (`docs/design/DESIGN.md`, *Four-deck layout (M4)*);
  the manual gives no order. The engine keeps deck A first in every per-deck array: the order is the interface's.

## Differences

- Decks C and D are not muted while the two-deck layout hides them: a track loaded into them
  by a command or a mapping plays and is heard.
- The external mode has no pair of its own for decks C and D: they share the pairs of the decks
  above them. A routing with a pair per deck waits for the output routing of the four-deck
  layout.
- The starting sides of the crossfader for decks C and D are Platter's choice, since the
  manual gives none: each takes the side of the deck above it. The maintainer confirmed it on
  18 November 2026.

## Edge cases

| Input | Result |
| --- | --- |
| A `deck` other than `a`, `b`, `c` or `d` | The command is rejected; nothing changes |
| Decks A and C both on the left with the crossfader at the far right | Both are silent in the master |
| Four decks playing at full scale | The safety limiter of the master holds the peak |
| External mode, decks A and C playing, with a four-output device | Both are heard on the first pair, summed |
| A file dropped on the window away from the decks, with decks A and B loaded and two decks shown | It loads into deck A, never into a deck the window does not show |
| A history entry stored with a deck number Platter does not know | It reads as deck A |
| A MIDI mapping or a render script written for two decks | It loads as before |

## Tests

`crates/core/src/deck.rs` (letters and indices), `crates/engine/src/rt/clock.rs` (tempo
master among four decks), `crates/engine/tests/four_decks.rs`,
`crates/engine/tests/realtime_memory.rs` (four decks without allocation) and
`crates/ui-egui/tests/panels.rs` (the window with decks C and D in use, and a dropped file).
