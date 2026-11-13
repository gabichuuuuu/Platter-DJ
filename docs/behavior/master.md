# Master section

## What the public manual says

Sections of the reference manual:

- *Overview*, section *Master control panel*
- *Preferences*, section *Mixer page*

A main knob sets the volume of the main output. A limiter protects that output from clipping;
it can be switched off, and the level meter of the main output shows when it is working. A
preference chooses the limiter type, one of which is a third-party maximiser.

## What Platter does in M1

- `master.gain.set` sets the main volume in decibels, from silence (-90 dB or below) to
  +12 dB. The change follows a 10 ms ramp.
- A safety limiter is always in the path, after the main volume. It keeps every output sample
  inside full scale: when a frame would exceed it, the gain of both channels drops at once to
  the value that puts the peak exactly at full scale, and then recovers over about 200 ms.
  Design and trade-offs: `docs/decisions/0007-safety-limiter.md`.
- The state snapshot carries the peak level of the main output and the gain the limiter is
  applying, for the meter of M2.

## Differences

- The limiter cannot be switched off. It is a guard against clipping, not an effect, and with
  it off a hot mix would clip at the converter.
- There is one limiter type. Platter's own maximiser arrives in M4; the third-party one of the
  reference product is out of scope (`docs/gaps.md`).

## Edge cases

| Input | Result |
| --- | --- |
| Main volume above +12 dB, or not a number | The command is rejected; nothing changes |
| Main volume of negative infinity | Silence |
| A mix far above full scale | Output peak at full scale; the limiter gain reports the reduction |
| A sample that is not a finite number reaches the limiter | It is replaced by silence |

## Tests

`crates/dsp/src/limiter.rs` and `crates/engine/tests/mixer.rs` (`main_volume_is_in_decibels`,
`the_limiter_keeps_an_overloaded_mix_inside_full_scale`).
