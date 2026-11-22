# 0042. Remix deck: a grid of samples on the audio thread, following the master clock

Status: accepted (M5).

## Context

M5 adds the remix deck (`docs/behavior/remix-deck.md`): four slots of sixteen cells per deck, each
cell a sample that starts on a beat of the clock. A deck already plays a track or an input in the
place of its track (`docs/decisions/0040-audio-input.md`). A sample, like a track, is decoded from a
file and must reach the audio thread without the audio thread allocating or freeing it
(`AGENTS.md`, *Real time*). The grid component of the interface is listed for M5 in
`docs/design/DESIGN.md` but not drawn yet, so this version is driven by commands only.

## Options

- **Where the cells play.** Inside the deck player, beside the track, which mixes two concerns
  in a module of 1 400 lines; or in a player of their own per deck, which the processor mixes in
  place of the track as it does with a live input.
- **The tempo of the deck.** A tempo control of the remix deck with its own sync, as in the public
  manual; or the master clock, which already carries the tempo and the beat of the tempo master
  and of a received MIDI clock.
- **Speed.** Key lock per slot through the stretch threads of the decks
  (`docs/decisions/0039-key-lock-threads.md`), which would need up to sixteen more streams; or
  plain resampling, the speed and pitch following the tempo.
- **Memory.** Samples of any length; or a bound before decoding, so that a long file loaded into a
  cell by mistake does not take the memory of a track per cell.

## Choice

- A remix player per deck on the audio thread, beside the deck player, mixed in its place with a
  smoothed gain as the live input is. Its 64 cells are fixed arrays; a sample arrives inside a
  command and leaves through the retired queue, one item per load or clear, so the count of swaps in
  flight that bounds that queue covers it.
- The master clock is the tempo and the beat of the deck. The beats are counted from the phase of
  the clock, as the effect units count them; a boundary of `n` beats is a multiple of `n`.
- Resampling with linear interpolation, as the preview player does, at
  `rate of the sample / rate of the engine × tempo of the clock / tempo of the cell`. Key lock per slot
  is left for later.
- Samples up to 64 seconds; the decoded length is checked on the worker before it is sent.
- Each slot keeps two voices: the cell that plays and the one that fades out, so a change of cell
  never clicks and never waits for the old one.

## How it was measured

`cargo test -p platter-engine --test remix_deck` renders the processor with synthesised samples (a
click on the first frame, a constant level) and checks the frame where each cell starts against the
beats of the clock at a fixed tempo, the length of a synced loop at two tempos, that a one-shot ends,
that a second cell stops the first and that a slot at zero volume or muted is silent.
`--test realtime_memory` runs loads, triggers and stops under the test allocator. Nothing was listened
to.
