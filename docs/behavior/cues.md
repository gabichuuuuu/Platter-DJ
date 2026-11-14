# Stored cues and hotcues

A stored cue is a place in the track, or a loop, the DJ wants to come back to. A deck keeps
eight of them in eight numbered slots, the hotcues. This note covers the hotcues, the types of
a cue and the commands of the CUE page. The cue point of the transport keys, called here the
floating cue, is in `docs/behavior/transport.md`; the loop of a deck in
`docs/behavior/loops.md`.

## What the public manual says

Sections of the reference manual, read on 14 November 2026:

- *Advanced usage tutorials*, sections *Working with cue points* and *Working with loops*,
  *Storing loops*
- *Basic usage tutorials*, section *Working with hotcue buttons and flux mode*
- *Overview*, section *Advanced panel*, the CUE page

Cue points are stored with each track and come back in later sessions. A stored cue has a
type, picked in a menu: cue, to jump to; fade in and fade out, for automatic mixing; load,
where a track starts when it is loaded, when a preference asks for it; and loop, a loop in
point. Each type has its own colour in the waveforms. The floating cue is the starting point
of all of them: the STORE button stores it in the cue list and maps it to the next free hotcue
button.

A click on an empty hotcue button stores a cue at the play position, or the loop that is on.
A click on a stored one jumps to it and playback goes on; on a stopped deck the button works
like the cue button: the deck plays from the cue while the button is held and returns to it,
paused, on release. Buttons step to the previous and the next stored cue, and a list selects
one; both move the play position to it, and a display shows the position of the selected cue.
The selected cue can be renamed, given another type and deleted. With the MAP button on, a
click on a hotcue button maps the selected cue to it and its former button is left empty.
Buttons nudge the selected cue backward and forward.

## What Platter does

Each deck has eight slots, numbered 1 to 8, for the cues of its track. A cue has a position, a
type, a name of at most 48 bytes of UTF-8 and, with the type `loop`, a length. At most one cue
is selected: the commands of the CUE page act on it.

| Command | Effect |
| --- | --- |
| `deck.hotcue.trigger` | `slot` 1 to 8, `action` `press` or `release`: stores into an empty slot, goes to a stored cue; see below |
| `deck.hotcue.delete` | `slot` 1 to 8: empties the slot |
| `deck.hotcue.map` | `slot` 1 to 8: moves the selected cue to the slot |
| `deck.cue.store` | Stores the floating cue, or the loop that is on, in the first empty slot |
| `deck.cue.select` | `direction` `previous` or `next`: goes to the stored cue before or after the play position |
| `deck.cue.rename` | `name`: names the selected cue |
| `deck.cue.type.set` | `type` `cue`, `fade_in`, `fade_out`, `load` or `loop`: changes the type of the selected cue |
| `deck.cue.delete` | Deletes the selected cue |

- As with the floating cue, the grid and the loop, the play position is the one the deck has
  once the commands before are applied, and so are the slots and the selection: a store, a
  change of type and a rename in a row all act on the cue just stored.
- A cue that is stored, and a cue the deck goes to, becomes the selected one.

### Storing

- A press on an empty slot stores the play position as a cue of the type `cue`. While a loop
  is on it stores the loop instead, as a cue of the type `loop` at its start with its length.
- `deck.cue.store` does the same with the floating cue instead of the play position, into the
  first empty slot. With the eight slots taken it does nothing.
- A new cue has no name. The interface shows its type until it gets one.

### Going to a cue

- On a deck that plays, a press on a stored slot moves the play position to the cue, to the
  frame, with the crossfade of a beatjump (`docs/behavior/loops.md`), and the deck plays on.
- On a paused deck the press moves the floating cue and the play position to the cue, and the
  deck plays while the key is held; the release returns to the cue and pauses, as the release
  of the cue key does. Play pressed during the hold keeps the deck playing. A deck that plays
  only because a cue key or a hotcue is held counts as paused.
- Going to a cue of the type `loop` also switches its loop on, in place of the loop the deck
  had.
- `deck.cue.select` goes to the nearest cue before or after the play position the same way,
  without the hold: a paused deck stays paused. A cue within 1 ms of the play position counts
  as the one the deck stands on and is skipped.

### Editing

- `deck.cue.type.set` to `loop` gives the cue the selected loop size from its position; it
  needs a tempo in the grid and a loop that ends inside the track, or nothing changes. From
  `loop` to another type the length is dropped. The loop of the deck is not touched.
- `deck.cue.rename` cuts a name longer than 48 bytes at the last whole character that fits,
  and drops control characters. An empty name removes the name.
- `deck.hotcue.map` onto a taken slot swaps the two cues. The selection follows the cue.
- Deleting the selected cue leaves no cue selected.

### In the interface

- Each deck has its eight hotcues as two rows of four pads above the loop row. A pad that
  holds a cue has a strip of the colour of its slot, its number in that colour and the name
  of the cue, or its type until it has a name; an empty pad shows its number alone. A pad
  acts while it is held, with the pointer or with Space or Enter: it sends
  `deck.hotcue.trigger` with `press` and then with `release`. Shift and a press delete the
  cue of the pad.
- The advanced panel has a third page, CUE, between MOVE and GRID. Its first row: the keys to
  the cue before and after the play position; the selected cue, as the colour and the number
  of its slot, its name or type and its position to the millisecond; and at the right end
  Store, Map and Delete. Its second row: the type of the selected cue, a list.
- Map switches the mapping mode of the hotcues of the deck, a choice of the window:
  `deck.cue.map`. While it is on the key is lit, the pads have a dashed outline, and a press
  on a pad sends `deck.hotcue.map` for it and ends the mode. The mode also ends when no cue
  is selected any more.
- Both waveforms draw each stored cue as a line in the colour of its slot with its number on
  a flag at the top; the scrolling one writes its name beside the flag. The floating cue is a
  triangle at the bottom, in the colour of the play position. The length of a loop is
  written at the bottom of the scrolling waveform, clear of the flags.
- Controls that would change nothing are disabled: the pads of an empty deck, a step key
  with no cue on its side, Store with the eight slots taken, Map, Delete and the type without
  a selected cue, and the type `loop` without a tempo.
- While a deck shows a notice, the notice takes the place of its pads.

## Differences

- The reference product keeps a list of cues and maps some of them to the eight hotcue
  buttons. In Platter the eight slots are the list: a stored cue always has a slot.
- The types `fade_in`, `fade_out` and `load` are a label and a colour for now. Automatic
  mixing is M4, and a cue cannot act when its track is loaded while cues are lost on a load:
  see the next point.
- The cues live with the loaded track and are lost when another track is loaded or the same
  one again, which a change of sample rate of the audio device also does, until the
  collection stores them later in M3.
- The design system lists a sixth type, grid. The grid markers of Platter live in the beatgrid
  (`docs/behavior/beatgrid.md`) and the reference manual does not list the type in its menu,
  so it is left out until the maintainer decides how a cue and a marker would stay one thing.
- The reference manual does not say what MAP does to a button that already holds a cue.
  Platter swaps the two, so no cue is lost.
- The nudge buttons of the selected cue are left out: a cue is moved by deleting it and
  storing it again.
- Cues are stored at the play position as it is unless Snap is on, and a playing deck goes
  to a cue as it is unless Quantize is on (`docs/behavior/sync.md`).
- A deleted cue is gone: there is no undo yet.
- The window cannot rename a cue yet: `deck.cue.rename` has no control until the text field
  of the browser search exists, later in M3, as with the tempo field of the grid.
- The hotcues have no default shortcuts. The design system gives the keys 1 to 8 to the deck
  with the focus; Platter has no such deck and those keys are taken, so the pads are
  reached with the pointer, with Tab, or with shortcuts the user adds.
- The pads are as wide as the deck allows and 38 px high, as the screen of the design system
  draws them; its component text says 76 by 44. The CUE page takes two rows, as the other
  pages do, where that screen draws one.

## Edge cases

| Input | Result |
| --- | --- |
| Any of these commands on an empty deck | Nothing happens |
| `slot` outside 1 to 8 or not a whole number | The command is rejected |
| A release of a key that holds nothing | Nothing happens |
| A press on an empty slot while a hotcue or the cue key holds the deck playing | Stores the play position; the hold goes on |
| A stored hotcue pressed while another key holds the deck playing | The deck plays from the new cue and the hold is now of that hotcue; the release of the first key does nothing |
| `deck.cue.store` with the eight slots taken | Nothing happens |
| `deck.cue.select` with no cue on that side | Nothing happens |
| Rename, type or delete without a selected cue | Nothing happens |
| `deck.hotcue.map` without a selected cue, or onto its own slot | Nothing happens |
| A type the cue already has | Nothing changes; a loop keeps its length |
| A new track | The eight slots are empty and no cue is selected |

## Tests

`crates/core/src/cue.rs` for the slots, the selection and the names;
`crates/engine/tests/cues.rs` for the commands; `crates/ui-egui/tests/panels.rs` for the pads
and the CUE page, which it operates control by control, with the Shift press and the
mapping mode in tests of their own.
