# 0015. Loops and beatjumps keep the time with a crossfade

Status: accepted (M3).

## Context

A seek fades the deck out over 5 ms, moves it and fades it in (`docs/behavior/transport.md`).
The deck keeps playing while it fades out, and the target was fixed when the command arrived,
so a seek costs about 5 ms of time and leaves a gap of about 10 ms in the sound. Neither
matters for a jump to another part of a track.

A loop and a beatjump (`docs/behavior/loops.md`) are different: the track has to stay on the
beat. A loop of four beats must last four beats on every pass, and the smallest loop, 1/32 of a
beat, is shorter than the gap of a seek. A beatjump must land the given number of beats from
where the deck is, to the frame, or a deck that is in sync leaves the beat.

## Options

1. The seek as it is. Every pass of a loop would be 5 ms long and have a hole; ruled out.
2. A seek whose target is relative to the position when it is applied. It keeps the time but
   keeps the gap, which a short loop cannot afford.
3. A jump at once, with no fade. It keeps the time and clicks: the waveform has a step at the
   jump.
4. A jump at once with a crossfade. The deck moves to the target in one frame; what it was
   playing goes on from where it was for a few milliseconds and fades out under what it
   plays now.

## Choice

Option 4, for the return of a loop to its start, for the beatjumps and for the moves that take
the play position along. The seek stays as it is.

- The crossfade is linear over 5 ms, or half a pass of the loop when that is shorter, so one
  crossfade ends before the next return. It reads the track a second time while it lasts;
  nothing is allocated.
- The return of a loop carries over what the deck overshot the end by, counted around the
  length of the loop with one remainder, never with a loop of its own. A loop is at least
  1 ms long.
- The loop catches a deck on the step that takes it through the end, not whenever it is after
  the end: a seek past a loop leaves it behind.
- A jump sent while a seek or a new track still waits changes the target of that seek, so
  commands in a row keep their order.

The loop lives in the processor, as the beatgrid does (`0014-beatgrid-in-the-processor.md`),
and the queue of that record now carries one `DeckReport` per deck and block with whichever of
the grid and the loop changed. The selected sizes, the move mode and the fine steps stay on the
control side, which sends the processor what they amount to with each command.

## How it was checked

`crates/engine/tests/loops.rs`, on Windows x64 at 48 kHz:

- 659 passes of a 1/32-beat loop at a speed of 1.03 end within two frames of where the time
  played puts them. With the overshoot dropped the same run ends 178 frames off.
- A beatjump of one beat while playing lands within two frames of the position plus the beat.
- The largest step between two samples of a 440 Hz tone of amplitude 0.5 stays under 0.04
  through the returns of a loop and through a beatjump, where the tone itself has a slope of
  0.029 per sample. Without the crossfade both tests fail.

`crates/engine/tests/realtime_memory.rs` runs a loop that returns several times per block,
jumps and moves, and counts no allocation.
