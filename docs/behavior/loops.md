# Loops and jumps

A loop repeats a stretch of the track; a beatjump moves the play position by a number of
beats without losing the beat. This note covers the loop of a deck, the jumps and the moves of
the MOVE page. Loops stored in hotcues come with the hotcues, and Quantize and Snap with sync,
both later in M3.

## What the public manual says

Sections of the reference manual, read on 14 November 2026:

- *Advanced usage tutorials*, section *Working with loops*
- *Overview*, section *Loop controls*
- *Overview*, section *Advanced panel*, the MOVE page

Loops have preset sizes from 1/32 of a beat to 32 beats, eleven in all, each twice the one
before. A size control with two arrows steps through them and applies to the loop whether it is
on or off; the arrows signal a size that is out of range. A loop of the selected size starts at
the play position. A loop of any length is made with an In button and an Out button: Out sets
the end and switches the loop on. An ACTIVE button switches the loop off, playback goes on and
the in and out markers stay; with no loop on, it switches on the next loop stored in the track.
The loop is highlighted in both waveforms.

The MOVE page has a mode menu (beatjump, loop, loop in, loop out), a size in beats, a mode that
takes the size from the length of the loop, a fine mode for small steps, and a backward and a
forward button. In the loop mode the whole loop moves; in the loop in and loop out modes one
end moves, which makes the loop longer or shorter.

## What Platter does

Each deck has a selected loop size, one of 1/32, 1/16, 1/8, 1/4, 1/2, 1, 2, 4, 8, 16 and 32
beats, 4 at the start; at most one loop, which is on for as long as it exists; and an in point
that waits for its out point. A beat lasts 60 seconds divided by the tempo of the beatgrid
(`docs/behavior/beatgrid.md`).

| Command | Effect |
| --- | --- |
| `deck.loop.toggle` | Without a loop: starts one of the selected size at the play position. With a loop: switches it off |
| `deck.loop.in` | Sets the in point at the play position and switches off the loop |
| `deck.loop.out` | Ends the loop at the play position and switches it on |
| `deck.loop.size.step` | `direction` `down` or `up`: selects the size before or after; a loop that is on takes it |
| `deck.beatjump` | `direction` `back` or `forward`: moves the play position by the move size |
| `deck.move` | `direction` `back` or `forward`: moves what the move mode says by the move size |
| `deck.move.mode.set` | `mode` `beatjump`, `loop`, `loop_in` or `loop_out` |
| `deck.move.size.step` | `direction` `down` or `up`: selects the move size before or after |
| `deck.move.fine.toggle` | Switches the fine steps on or off |

- As with the cue point and the grid, the play position is the one the deck has once the
  commands before are applied: a seek and a loop in a row start the loop where the seek landed.
- All of this works on a paused deck too; the loop plays when the deck does.

### The loop

- While the deck plays through the end of the loop it goes on from its start. The return keeps
  the time: what the deck overshot the end by is played from the start, so a loop of four
  beats lasts four beats on every pass. The two sides of the return are crossfaded over 5 ms,
  or half the loop when that is shorter, so it does not click.
- The loop only catches a deck that plays through its end. A seek or a jump to a place after
  the loop leaves it on and the track plays on; from a place before the loop the deck plays
  into it and stays.
- `deck.loop.out` needs an in point at least 1 ms before the play position; the loop runs from
  the in point to the play position and the deck goes on from the in point. With a loop on
  and the play position inside it, it moves the end to the play position instead.
- A loop that is on takes a newly selected size from its start when the grid has a tempo and
  the loop still ends inside the track; otherwise the selection changes and the loop stays. A
  deck left after the new end goes back into the loop at the same distance from its start,
  counted around the new length.
- The loop is a stretch of the track in time. A change of the tempo of the grid does not move
  it.

### Jumps and moves

- The move size is one of the eleven sizes in beats, 1 at the start, or `loop`, the entry after
  32: the length of the loop that is on, or the selected loop size without one.
- With the fine steps on, the step is 5 ms whatever the move size says.
- A beatjump moves the play position by exactly the step, to the frame, also while playing,
  with the same crossfade as the return of a loop.
- `deck.move` by mode: `beatjump` is `deck.beatjump`; `loop` moves both ends and, when the
  play position is inside the loop, the play position with them; `loop_in` and `loop_out`
  move one end. A deck left after an end that moved back returns into the loop as after a
  change of size.

### In the interface

- Each deck has a loop row above the page of its advanced panel: the selected loop size
  between two arrows, In, Out and Active, and at the right end the two beatjump keys around
  the move size. In is lit while an in point waits for its out point and Active while a loop
  is on, both in the loop colour.
- The advanced panel shows one page at a time, MOVE or GRID, picked with a switch at the
  right end of its second row; it starts on MOVE. The page is a choice of the window, not of
  the engine: `deck.page.set`. The CUE page comes with the hotcues.
- The MOVE page has the four modes in its first row; in its second the move size between two
  arrows, the two move keys and Fine, lit while the fine steps are on. With them on the size
  reads `5 ms` and its arrows rest.
- Both waveforms draw the loop as a veil of the loop colour with a bracket at each end, and
  an in point that waits as a single bracket. The scrolling waveform also writes the length
  of the loop at its start: in beats with a tempo in the grid, in seconds without.
- Controls that would change nothing are disabled: an arrow at the end of its list, Out
  without an in point or a loop, Active and the jumps without a tempo, the moves of a loop
  without a loop.
- Default shortcuts, per deck (`docs/behavior/keyboard.md`): the loop, its size down and up,
  and the jump back and forward.

## Differences

- The reference product keeps the in and out markers of a loop that is switched off, and its
  ACTIVE button switches on the next stored loop. In Platter a loop that is switched off is
  gone, and `deck.loop.toggle` without a loop starts one of the selected size, the way its
  preset size buttons do. Stored loops come with the hotcues.
- In, Out, the start of a loop and the jumps use the play position as it is. Landing on the
  beat is Quantize and Snap, which come with sync.
- The fine step is a time, 5 ms, so it also trims a loop made with In and Out on a track
  without a grid. The reference manual gives no value for its fine step, and its extra-fine
  step is left out.
- The mode that takes the move size from the loop is the last entry of the size list instead
  of a button of its own.
- An arrow that has nowhere to step to is disabled. The design system asks for a flash of the
  arrow on a size that is out of range; a selected size the loop could not take shows in the
  waveform, where the label keeps the length the loop has.
- The design system names one set of loop keys for the deck with the focus. Platter has no
  such deck: every shortcut names its deck, so each deck has its own keys.

## Edge cases

| Input | Result |
| --- | --- |
| Any of these commands on an empty deck | Nothing happens; the selections of size and mode still change |
| A loop, a size or a step in beats on a grid without tempo | Nothing happens; In, Out and the fine steps still work |
| A loop of the selected size that would end after the end of the track | Not started |
| Out without an in point, or less than 1 ms after it | Nothing happens |
| A jump or a move that would leave the track | Nothing happens |
| An end moved to less than 1 ms from the other | Nothing happens |
| A move in a loop mode without a loop | Nothing happens |
| A size step below 1/32 or, for the loop size, above 32 | Nothing changes |
| A loop that ends at the end of the track | The deck returns to its start instead of stopping |
| A new track | No loop and no in point; the selected sizes, the mode and the fine steps stay |

## Tests

`crates/core/src/loops.rs` for the sizes; `crates/engine/src/rt/looper.rs` for the loop itself;
`crates/engine/tests/loops.rs` for the commands, the length of a pass, the place a jump lands
and the absence of clicks; `crates/ui-egui/tests/panels.rs` for the loop row and the MOVE page,
which it operates control by control.
