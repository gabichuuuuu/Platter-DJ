# 0033. The effect chain: units, instances per channel and swaps

Status: accepted (M4), with the design sections of the effect units approved on 18 November
2026.

## Context

M4 adds effect units that any channel can go through, in group mode (three effects in a
chain) or single mode (one effect with more controls), in insert or post-fader routing, and a
one-knob mixer effect per channel (`docs/behavior/effects.md`). The rules of the audio callback
hold: no allocation, no lock, bounded time. Delays and reverbs need buffers of several seconds,
so an effect cannot be built inside the callback. The engine already swaps tracks into the
processor through the command queue and sends what it replaces back on the `Retired` queue,
to be freed on a normal thread (`docs/decisions/0009-engine-control-and-snapshot.md`).

Two facts from the mixer models (`docs/decisions/0032-mixer-models.md`) bound the design:

- `Processor` is built on the stack of the thread that starts the engine. With four channels it
  measured 70 KB once every channel held four equaliser models in place, and the command-line
  renderer overflowed the 1 MB stack of its main thread in a debug build. The models moved to
  the heap. Effect state must not grow `Processor` the same way.
- When the engine restarts after a change of sample rate or device, it rebuilds the processor
  and sends it the state of the mixer again (`as_commands`). The crossfader sides of decks C
  and D were lost there once (`fix(engine)` of 18 November 2026). Effect state needs the same
  path and a test.

## Options

- **One instance per unit, fed by the sum of its channels.** Cheapest, but it cannot give
  each channel back its own processed signal, so the channel fader, the filter and the
  crossfader could not act after the effect. That is not an insert.
- **One instance per channel and unit, built when needed.** Each channel assigned to a unit
  has its own copy of the unit's effects, driven by the unit's values. The equaliser, the
  filter, the fader and the crossfader of each channel act on its own effected signal. Memory
  grows with the assignments.
- **Every effect built in advance in every slot.** No swap at all, but eight effects in three
  slots of four units on four channels is 384 instances, most of them delays and reverbs.
- **Effects as plug-ins behind a host interface now** (CLAP). No plug-in host exists and no
  plug-in is planned for M4; an abstraction without a second user is filler.

## Choice

- **Instances per channel and unit.** The processor keeps, for each channel, a boxed
  `ChannelEffects` with one entry per unit: the instances of the unit's slots (group mode) or
  of its single effect, and their ramps. `Processor` holds only the boxes, so its inline size
  grows by a few pointers. A test keeps `size_of::<Processor>()` under a bound, so the next
  addition that grows it inline fails there rather than in the renderer.
- **Instances are built on the control thread.** Selecting an effect, assigning a channel to a
  unit or switching the mode builds the instances the processor needs, with their buffers, and
  sends them through the command queue. The processor puts them in place and sends what they
  replace back on `Retired`. The callback never allocates or frees. The swaps in flight count
  against `COMMAND_CAPACITY`, as track swaps do.
- **Values are separate from instances.** The values of a unit (dry/wet, amounts, knobs, keys,
  on keys) are plain numbers sent as commands and smoothed in the callback. One command
  updates the unit's value, and every channel instance of that unit reads it. A knob turn
  costs no swap.
- **Order.** Insert units sit after the equaliser and before the filter; post-fader units sit
  after the fader and the pan; both in the order 1, 2, 3, 4. The mixer effect sits after the
  filter. Pre-listening taps the channel after the filter, as now.
- **Tempo.** Effects read the tempo and the beat position of the master clock from the
  processor every block; times in beats become samples there. No effect keeps its own clock.
- **Memory bound.** The delay buffer holds four seconds at the stream rate, enough for four
  beats down to 60 beats per minute; at slower tempos the longest times halve until they fit.
  The reverb, flanger, phaser and slicer buffers are shorter (the slicer holds one bar at the
  same bound). At 48 kHz a delay instance takes 1.5 MB; the worst case, every channel assigned
  to both units with three delays each, is 24 instances, 37 MB. At 384 kHz, the highest rate
  Platter accepts, the same worst case is eight times larger; the ADR accepts it, since that
  rate is chosen on purpose and the memory is taken only by what is assigned.
- **Restart.** The control side keeps the whole effect state (modes, effects, values,
  snapshots, assignments, routings) and replays it after a restart: the values through
  `as_commands`, the instances by building them again for the new rate. Tails are lost, as
  every other sound is. A test restarts an engine at another rate with units assigned on all
  four channels and checks what comes back. The burst of a full replay is counted against
  `COMMAND_CAPACITY` (256): four units of about twenty values, sixteen assignments and four
  mixer effects come to about a hundred commands, beside the mixer's dozen.
- **Plug-ins later.** Each effect is a block processor with values between 0 and 1, a reset
  and the tempo and beat of the clock as its context. That is the shape a plug-in host would
  need; nothing in this design ties an effect to the processor beyond it. No trait or host
  layer is written now.

## How it will be measured

- `crates/dsp`: each effect with synthesised signals: transparency at dry/wet 0 (bit for bit),
  the delay time at 120 beats per minute to the sample, a tail that rings after switching off,
  finite output from non-finite input.
- `crates/engine`: the order of insert and post-fader units against the fader and the filter,
  separate echoes on two channels, the restart test above, and the test allocator around the
  callback while effects are swapped and turned.
- The time of the callback with every effect of both units on all four channels, in a release
  build, against the length of a 256-frame block at 48 kHz.
