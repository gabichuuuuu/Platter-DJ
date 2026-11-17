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
- The window shows decks A and B. Decks C and D are driven through commands: the command line
  render script, MIDI mappings and, later, the four-deck layout.

## Differences

- The window does not show decks C and D yet, and there is no option to show or hide them. The
  design system places them under decks A and B in the four-deck layout of M4, whose component
  sections are not written yet (`docs/design/DESIGN.md`, *Still to define*). Until then decks
  C and D are not muted when they are not shown: a track loaded into them by a command or a
  mapping plays and is heard.
- The external mode has no pair of its own for decks C and D: they share the pairs of the decks
  above them. A routing with a pair per deck waits for the output routing of the four-deck
  layout.
- The starting sides of the crossfader for decks C and D are Platter's choice, since the
  manual gives none: each takes the side of the deck above it.

## Edge cases

| Input | Result |
| --- | --- |
| A `deck` other than `a`, `b`, `c` or `d` | The command is rejected; nothing changes |
| Decks A and C both on the left with the crossfader at the far right | Both are silent in the master |
| Four decks playing at full scale | The safety limiter of the master holds the peak |
| External mode, decks A and C playing, with a four-output device | Both are heard on the first pair, summed |
| A history entry stored with a deck number Platter does not know | It reads as deck A |
| A MIDI mapping or a render script written for two decks | It loads as before |

## Tests

`crates/core/src/deck.rs` (letters and indices), `crates/engine/tests/four_decks.rs`.
