# 0016. The master clock lives in the processor and beat sync moves the deck

Status: accepted (M3).

## Context

Sync (`docs/behavior/sync.md`) makes a deck play at the tempo of the tempo master, a deck or
the master clock, and in the beat mode keeps its beats on those of the tempo master. Three
things had to be decided: where the clock and the choice of the tempo master live, how a deck
gets its speed from them, and how a deck is brought back onto the beat.

The tempo master changes with what the decks do: in the automatic mode the deck that starts to
play takes over, and a deck that reaches the end of its track hands over. The control side
learns of the end of a track a block late, through the atomics.

## Options

Where the clock lives:

1. On the control side, which would send each deck its speed. A change of tempo of the tempo
   master would reach the synced deck a block or more late, and a track that ends would leave
   the choice of the tempo master to the next poll of the interface.
2. In the processor, beside the decks.

How a synced deck stays on the beat:

1. By speed alone: play a little faster or slower until the beats meet. Half a beat at 2 %
   takes twelve seconds at 120 BPM, and the pitch moves all that time, as there is no key
   lock.
2. By moving the deck alone, whenever its beat is off. The speed of a deck is a 32-bit ratio
   and ramps for 20 ms after each change, so a deck would drift and be moved again and again.
3. Both: move the deck at once for an offset a DJ would hear, and close the rest by speed.

## Choice

The clock lives in the processor (`crates/engine/src/rt/clock.rs`): a tempo, a place in the
beat and the tempo master. The processor chooses the tempo master once per block, after the
commands. Each frame the clock takes the tempo of the deck that is the tempo master and, while
that deck plays, its place in the beat; without such a deck it counts on at its own tempo.
Each synced deck then sets its speed to the tempo of the clock over the tempo of its grid.

For the beat, the third option:

- An offset above 1/32 of a beat, 16 ms at 120 BPM, is closed by a move of the play position
  of at most half a beat, with the crossfade of `0015-jumps-that-keep-the-time.md`. The move
  waits for the ramp of the speed to end and for the crossfade of the move before.
- A smaller offset changes the speed by twice the offset in beats, within 2 %.
- The move stays inside the run of beats of the marker the deck is in, where the offset was
  measured, so a marker off the beat cannot send the deck back and forth.
- Nothing is done while the deck is bent, while a cue key holds it playing, while it sits in
  a loop that is not a whole number of beats long, or when the tempo of the clock asks more
  than double speed.

The sync of a deck and the choice of the tempo master are toggled and set in the processor, so
commands in a row act in the order they were sent. What the interface shows goes through the
atomics, published once per block: the tempo master, the tempo of the clock, and the sync, the
speed and the phase of each deck. The mode of the clock and the sync mode are set by commands
alone, so the control side keeps them. The tempo fader of a synced deck is refused by the
processor, which knows whether the deck follows; when the sync goes off the deck keeps its
speed.

## How it was checked

`crates/engine/tests/sync.rs`, on Windows x64 at 48 kHz, with a deck at 120 BPM as the tempo
master and a deck at 100 BPM in sync. The offset between the beats of the two decks is worked
out from their play positions and grids:

- Under 0.001 frames 0.1 s after the synced deck starts 0.6 of a beat late, and three seconds
  later; after a seek, after a bend ends, in a loop of four beats, and 0.3 s after the sync of
  a playing deck is switched on.
- 0.02 frames 1.5 s after the tempo master changes its tempo by 5 %. With the correction by
  speed taken out the same test ends 76 frames off.
- The largest step between two samples stays under 0.04 through the move that the sync of a
  playing deck causes, where the tone itself has a slope of 0.028 per sample.

`crates/engine/tests/realtime_memory.rs` runs the commands of sync with a deck that follows
and is moved; `crates/engine/tests/command_sweep.rs` sends them in random order with the
rest.
