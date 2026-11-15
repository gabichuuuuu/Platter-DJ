# 0030. Flux and reverse: one timeline per deck, kept by its detours

Status: accepted (M3).

## Context

The public manual describes flux as a second play position that goes on while the deck
loops or jumps to a cue, and reverse as backward playback that also turns flux on
(`docs/behavior/flux.md`). It leaves open what starts and ends that second position, how it
meets loops, beatjumps, sync and key lock, and what reverse does at the start of the track.

## Options

For the second position:

1. Start it at the first pass of a loop, at the point the deck would have reached.
2. Start it where the deck is when a detour begins (a loop switched on, a hotcue held,
   reverse pressed), and move it forward on every frame the deck plays.

For sync during a detour:

1. Keep giving the clock the beat of what is heard.
2. Give the clock the beat of the second position.

## Choice

- Option 2 for the position: one timeline per deck, started by the first detour, moved
  forward by the step of the deck on every frame it plays, and ended when the last detour
  ends. Option 1 misses what a loop does before its first pass: a loop switched on behind the
  play position folds the deck back at once, and a beatjump or a move inside the loop changes
  what is heard without a pass.
- The return uses the jump of the deck, with the 5 ms crossfade of a beatjump, and only when
  the deck is a frame or more away. Seeking, the CUE and CUP buttons, loading a track and
  switching flux off end the timeline without a return: they are deliberate moves.
- Option 2 for sync. A deck in a loop or in reverse with flux is still in the track, and its
  timeline is where its beat is: as tempo master it keeps the clock going forward, and as a
  follower it does not pull what is heard towards a beat it will leave. A reversed master
  that gave the clock its backward beat would drag the other deck backwards.
- Reverse reads the track backwards at the speed of the deck. A turn of direction crossfades
  the sound of the old direction under the new one, like a jump. Key lock stays out while the
  deck plays backwards: the stretch only moves forward, and after a reverse it comes back with
  the bridge it uses after any jump (`docs/decisions/0021-key-lock-stretch.md`).
- At the start of the track a reversed deck stays there in silence, rather than stopping:
  stopping would change the state of the play button behind the back of the DJ.

## How it was checked

`cargo test -p platter-engine --test flux` on Windows x64, with synthesised tracks:

- A deck that took a detour through reverse is at the same frame as one that played straight
  on, and both play the same samples afterwards, within 1e-6.
- A loop with flux returns to where the track would be; without flux it stays.
- A ramp read backwards falls by the slope of the ramp, with steps below 0.001 at both turns.
- Reverse wraps inside a loop without a step above that of the tone.
- A reversed deck stays below -180 dB at the start of the track.
- A hotcue held with flux returns on release.
- A seek, or flux switched off, ends the timeline.
- A timeline past the end brings the deck to the end.
- A synced deck stays within 0.01 beats of a reversed tempo master.
- A 44.1 kHz track counts in the time of the track.
- With key lock at a tempo of 1.08, both turns stay below a step of 0.04, and key lock is back
  at 440 Hz half a second after the release. Before the stretch rendered what it was playing
  at the press, that turn stepped by 0.147.

`realtime_memory` runs flux and reverse under the test allocator, and the command sweep sends
them at random.

## Pending

Listening with real tracks (`ROADMAP.md`, checks waiting for equipment or material): the turn
of reverse and the return of flux on music, and the come-back of key lock after a reverse.
