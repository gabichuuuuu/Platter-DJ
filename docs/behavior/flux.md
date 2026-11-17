# Flux and reverse

Flux keeps the place of a deck in the track while a loop, a hotcue or reverse takes it
elsewhere: when they end, the deck goes on from where it would have been had it played on.
Reverse plays the track backwards while its button is held. Both live in the engine,
`crates/engine/src/rt/deck.rs`; the choices behind them are in
`docs/decisions/0030-flux-and-reverse.md`.

## What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Overview*, section *Transport controls*
- *Basic usage tutorials*, section *Working with hotcue buttons and flux mode*

The transport controls of a deck have a FLUX button, which turns flux on for the deck, and a
REVERSE button, which turns on reverse and flux together. With flux on, a second play position
goes on through the track while the deck loops or jumps to a cue; when the loop or the cue is
released with its hotcue button, playback goes on from that second position, further on the
longer the loop lasted. The waveform shows it as a marker. Reverse plays the track backwards
from the play position while its button is held, and normal playback comes back on release.

## What Platter does

### The commands

- `deck.flux.toggle` turns flux on or off for a deck. It is a setting of the deck, like key
  lock: it stays for the next track and when the engine is built again for another sample
  rate.
- `deck.reverse` with `action = "press"` or `"release"` holds reverse, like the CUE button.

### The timeline

While flux has something to keep, the deck has a timeline: a second play position that
starts where the deck is and moves forward at the speed of the deck on every frame the deck
plays, whatever the deck hears. A detour starts the timeline when the deck has none:

- A loop switched on while the deck plays with flux on. Each pass of the loop, a beatjump or a
  move of the loop moves what is heard, not the timeline.
- A stored cue reached with the key of its hotcue while the deck plays with flux on. The key
  is held: releasing it ends the detour and switches off the loop the cue stored.
- Reverse pressed, with or without flux, on a loaded deck.

When the last detour ends (the loop switched off, the hotcue key released, reverse released)
the deck goes to the timeline at once, with the crossfade of a beatjump, and the timeline
ends. A deck already there does not move.

The timeline ends without moving the deck when the deck seeks, goes to its cue point with the
CUE or CUP button, or loads a track, and when flux is switched off; reverse held then keeps a
timeline of its own, started where the deck is. A timeline that reaches the end of the track
stays there, and the deck stops at the end when it goes there.

On a paused deck flux changes nothing: a hotcue keeps its preview of the cue button
(`docs/behavior/cues.md`).

### Reverse

- The deck reads its track backwards at the speed it has, from the play position. A loop holds
  it: passing the start of the loop backwards takes it to the end, with the crossfade of a
  pass of the loop.
- Pressing and releasing reverse crossfade from one direction to the other, as a beatjump
  does, so the turn does not click.
- At the start of the track the deck stays at the start and is silent until reverse is
  released.
- Key lock does not act while the deck plays backwards: the pitch follows the speed. At the
  press, what the stretch played goes on for the crossfade, as at a jump, so the turn does not
  click at another tempo either. Key lock comes back, with the bridge it uses after a jump,
  when the deck plays forward again.

### Sync

The beat a deck gives the clock as tempo master, and the beat it keeps when synced, is that of
its timeline while it has one, so a deck in a loop or in reverse with flux gives the clock the
beat of the track as it would have played on. A deck does not pull itself back to the beat
while it has a timeline: the timeline already moves at the tempo of the clock, and the deck
comes back on the beat when it returns to it.

### What the snapshot shows

Per deck: flux on or off, reverse held or not, and the position of the timeline in seconds
while there is one, for the marker of the waveform.

## Differences

- The public manual does not say how flux treats a beatjump, Snap or Quantize, nor what
  ends the second play position. Platter treats a beatjump as a move of what is heard during a
  detour and as a plain jump outside one, and ends the timeline as listed above.
- Releasing the hotcue of a stored loop switches the loop off so the deck can return; this is
  Platter's reading of "released with its hotcue button".
- The public manual does not say what reverse does at the start of the track nor how it
  sounds with key lock. Both are Platter's choices.
- FLUX, lit while on, and REVERSE, lit while held, sit beside play in the transport row of
  each deck, with the LED of the design system. While a deck has a timeline, a dimmed playhead
  shows where it is, in the scrolling waveform and in the overview. The design system gives
  the deck header no indicator of flux; the lit key and the dimmed line say it.
- Nobody has listened to flux or reverse yet (`ROADMAP.md`, listening with real tracks).

## Edge cases

| Input | Result |
| --- | --- |
| A loop on with flux, held for three passes, then switched off | The deck plays where it would be after three passes' time, not after the loop |
| Reverse pressed and released after one second | The deck plays one second after where it was on the press |
| Reverse held past the start of the track | Silent at the start; on release, the timeline |
| Flux switched off during a loop | The loop goes on; switching it off later does not move the deck |
| A hotcue of a playing deck with flux off | It jumps there and stays, as before |
| The timeline at the end of the track on return | The deck goes to the end and stops |
| A reversed tempo master | The clock keeps the beat of the timeline, forward |

## Tests

`crates/engine/tests/flux.rs` on synthesised tracks whose samples say their own position:
a deck that takes a detour and one that does not end at the same place and play the same
sound; reverse reads backwards, wraps inside a loop, stays silent at the start; no click at
press, release and wrap; flux with a synced deck; a track at 44.1 kHz; the snapshot.
