# Mixer

The inventory places the mixer controls in M2, with the interface. M1 delivers the engine
commands behind them; this note covers those commands and is completed in M2.

## What the public manual says

Sections of the reference manual:

- *Overview*, section *Mixer channel*
- *Overview*, section *Crossfade controls*
- *Preferences*, section *Mixer page*

Each channel has a gain knob that sets the level before the fader, three equaliser knobs for
high, mid and low, a kill button per band that removes the band completely, and a channel
fader. The crossfader blends the two sides, and a preference shapes its curve continuously
from smooth to sharp. The preferences also offer several equaliser types and filter types.
The section of the manual on the mixer channel does not describe the one-knob filter; the
behaviour below (low-pass one way, high-pass the other) is Platter's own definition.

## What Platter does in M1

Signal path of a channel: gain, equaliser, filter, fader, meter. Then the crossfader, the sum
of both sides and the master section (`docs/behavior/master.md`).

| Command | Effect |
| --- | --- |
| `mixer.gain.set` | Channel gain in decibels, silence to +12 dB |
| `mixer.eq.set` | Position of a band knob, 0 to 1: 0 removes the band, 0.5 leaves it untouched, 1 boosts it by 12 dB |
| `mixer.filter.set` | Position of the filter knob, -1 to 1: low-pass below zero, high-pass above, no effect at zero |
| `mixer.fader.set` | Fader position, 0 to 1, with a squared taper; 0 is silence |
| `mixer.crossfader.set` | Position, 0 (deck A alone) to 1 (deck B alone) |
| `mixer.crossfader.curve.set` | `smooth` (equal power), `linear` or `sharp` |

- Every parameter follows a 10 ms ramp; none of them steps.
- The equaliser splits the signal at 250 Hz and 2.5 kHz
  (`docs/decisions/0005-equaliser-topology.md`).
- The filter sweeps its corner exponentially: the low-pass from 20 kHz down to 60 Hz, the
  high-pass from 20 Hz up to 10 kHz (`docs/decisions/0006-channel-filter.md`).

## Differences

- A band is removed by turning its knob to zero. The kill buttons and the pan control are M2.
- The crossfader has three named curves instead of a continuous control.
- One equaliser model and one filter model; the others are M4.

## Edge cases

| Input | Result |
| --- | --- |
| A position or a gain outside its range, or not a number | The command is rejected; nothing changes |
| All three bands at zero | Exact silence from the channel |
| Filter at exactly zero | The signal passes bit for bit |
| Crossfader at an end | The far side is exactly silent, with every curve |

## Tests

`crates/dsp/src/eq.rs`, `crates/dsp/src/filter.rs`, `crates/dsp/src/crossfade.rs` and
`crates/engine/tests/mixer.rs`.
