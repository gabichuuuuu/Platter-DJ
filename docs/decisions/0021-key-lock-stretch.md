# 0021. Key lock stretches the track with timestretch

Status: accepted (M3).

## Context

Key lock keeps the pitch of a track while its tempo changes (`docs/behavior/keylock.md`). The
roadmap puts it behind a project trait and asks for two time-stretching libraries to be
measured before one is chosen; `docs/dependencies.md` names timestretch and Signalsmith Stretch.
The maintainer asked for the pure Rust one first, with Platter's own stretch written later if
its quality falls short.

What the deck asks of a stretch:

- the rules of the audio callback: no allocation, no lock, bounded time;
- a tempo exact to the hundredth of a BPM, as for the analysis (ADR 0020): a tempo off by
  0.2 % puts two synced decks a beat apart within four minutes;
- a position it can trust, since sync, the phase meter, cues and loops read it;
- jumps (hotcues, beatjumps, the cue button, loop wraps) that are heard at once;
- kicks that keep their attack.

## Options

1. timestretch 0.16.0, MIT on crates.io and in its repository, pure Rust. It depends on rustfft 6.4.1, num-complex, num-integer,
   num-traits, primal-check, strength_reduce and transpose, all MIT OR Apache-2.0. Its two
   crates declare `forbid(unsafe_code)`; the default feature `accelerate` only binds a macOS
   library and is left off.
2. Signalsmith Stretch, MIT, C++, through the crate signalsmith-stretch 0.1.3. It needs a C++
   compiler, libclang for its bindings at build time, and an `unsafe` exception in `AGENTS.md`.

## How they were measured

On 15 November 2026, in projects outside the repository, on synthesised signals only. Release
build, Windows x64, an engine at 48 kHz fed in blocks of 256 frames (5.33 ms), stereo.
timestretch was driven through its streaming session with the portable backend. Signalsmith
was driven with its default preset through a shim of its header written for the measurement
(the published wrapper would not build without libclang); its allocations were counted through
the global `operator new` and `operator delete` of the C++ side, the Rust heap through a
counting allocator for both.

| Figure | timestretch | Signalsmith |
| --- | --- | --- |
| Allocations after construction: play, tempo changes, seek, loop | None | None |
| Pitch of sines at 110, 440 and 2000 Hz, tempo 0.84 to 1.16 | Exact to 0.001 cent | Exact to 0.003 cent |
| Sines: what is not the sine, tempo 0.84 to 1.16 | -61 to -90 dB | -41 to -84 dB |
| Kick at 128 BPM, rise from 10 to 50 % of the peak (0.48 ms in the source) | 2.8 to 4.4 ms | 5.8 to 11.3 ms |
| Kick: onset, mean and largest spread across beats | -5.1 to -0.8 ms, up to 3.8 ms | -7.2 to -1.9 ms, up to 5.7 ms |
| Kick: peak (0.785 in the source) | 0.90 to 0.92 | 0.75 to 0.84 |
| Kick: largest level in the 20 ms before the onset (-32 dB in the source) | -21 to -27 dB | -20 to -24 dB |
| Delay of the output against the position the host gives | 0 frames | 2880 frames (60 ms) |
| Cost of a block: mean, 99th percentile | 0.10 to 0.17 ms, 0.3 to 0.6 ms | 0.07 to 0.10 ms, 0.4 to 0.7 ms |
| Cost of the slowest block, across runs | 0.7 to 3.2 ms | 0.9 to 2.5 ms |
| Work before the first block after a seek | 301 stages, 4.6 to 5.3 ms in all, none above 0.4 ms | 0.03 ms |

At a ratio of exactly 1.0 both give the source back unchanged (-121 to -139 dB).

timestretch, further:

- By default it keeps the pitch exact and rounds the tempo to its hop: steps of about 0.2 %
  near 1.0, so 1.0001 plays at 1.00196 and 0.999 at 1.0. With the constraint
  `PreserveDuration` it plays the tempo asked for and moves the pitch instead, by at most
  3.3 cents in the measurement; four minutes at 0.999, 1.0001, 1.0123, 1.06, 0.92 or 1.16 end
  within 2 frames of the position asked for.
- The position it reports matched each click of a click train to within 9 frames (0.2 ms)
  while the tempo changed every block, at 48 kHz and from a 44.1 kHz source.
- A tempo change scheduled at the current output frame took effect 510 frames (10.6 ms) later,
  the audio it had already rendered.
- A 44.1 kHz source into the 48 kHz engine kept the delay at 0 frames, the kick onsets within
  0.5 ms and a seek on the right sample.
- A sweep of the tempo from 0.99 to 1.01 over 16 s, through 1.0, gave no step larger than the
  sine itself makes and no dip in level.
- It takes the tempo between 0.03125 and 4; it does not play backwards.
- Loops it plays itself cost no more than playing; a seek restarts it.
- The work before the first frame after a seek is 277 to 345 steps, from tempo 0.5 to 1.6,
  for 44.1 and 48 kHz tracks into a 48 or 96 kHz engine. In steady play a run of 64 frames
  needed at most 16 steps.
- A session holds about 9 MB of buffers for a 44.1 or 48 kHz track into a 48 kHz engine and
  about 16 MB at 96 kHz, beside its FFT plans, which it does not count. The engine builds one
  for each loaded track, on the load worker.

## Choice

Option 1, timestretch, with `PreserveDuration`, behind the trait of the engine, as the
maintainer chose. It is better than Signalsmith on what a DJ hears first, the attack of a kick
and the delay after a jump, it is pure Rust, and it needs no exception to the rules.

Neither keeps a kick as the source has it: timestretch slows its attack from 0.5 ms to about
4 ms, moves its onset by up to 5 ms and raises its peak by about 1.3 dB. This is the first
thing an own stretch in `platter-dsp` would have to do better.

The 4.6 to 5.3 ms of work after a seek do not fit in one callback beside a second deck. The
engine therefore spreads that work over the 32 ms after a jump, one step every few frames, and
plays the track at the speed of the deck, with the pitch following, until the stretch is ready
(`docs/behavior/keylock.md`). At a jump the stretch first renders what follows where it was,
5 ms of it, for the deck to fade out under the new place; a fade of the plain read there
stepped ten times further than the sine itself at 1.08. Each pass of a loop is such a jump: on
a loop of one beat at 128 BPM the bridge and its crossfade are about 8 % of each pass.

The tempo is scheduled once the stretch is ready, not with the seek: scheduled together, the
first frames came out off the position the session reports. The first 500 frames or so after a
jump therefore play at the tempo the stretch had before; the position stays the one heard.

Not measured: real tracks, other block sizes, other machines, an engine at 96 kHz in the
callback, the `cheaper` preset and the split computation of Signalsmith beyond the latency they
add.
