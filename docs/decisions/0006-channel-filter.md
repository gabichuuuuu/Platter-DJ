# 0006. One-knob channel filter

Status: accepted (M1).

## Context

Each channel has a filter on one knob: a low-pass when turned one way, a high-pass the other,
and no effect at the centre. The knob is moved while music plays, so the corner frequency
changes every sample.

## Options

- **Second-order sections in direct form** with new coefficients per sample. They can produce
  bursts when the coefficients move fast.
- **State-variable filter in its trapezoidal form.** It stays stable and well behaved under
  per-sample modulation, and gives low-pass and high-pass from the same state.

## Choice

A second-order state-variable filter in the trapezoidal form, with a quality factor of 0.9: a
slight emphasis at the corner, far from self-oscillation.

Knob mapping, for a position between -1 and 1:

- Below zero, a low-pass whose corner falls exponentially from 20 kHz to 60 Hz.
- Above zero, a high-pass whose corner rises exponentially from 20 Hz to 10 kHz.
- At exactly zero the signal passes bit for bit. Within 0.05 of the centre the filtered signal
  is blended in gradually, so entering the filter does not click.

The corner is kept below 45 % of the sample rate. The engine smooths the knob position over
10 ms before it reaches the filter.

## How it was measured

Tests in `crates/dsp/src/filter.rs`, with sines at 48 kHz:

- Centre: output equal to the input, sample for sample.
- Half way to the left: within 1 dB at 100 Hz, more than 30 dB down at 10 kHz. Mirrored to the
  right.
- A sweep of the whole travel in two seconds over a 200 Hz sine: output finite, below full
  scale, and no step larger than four times the largest step of the input.

## Consequences

Further filter models (M4) will sit beside this one. The ranges and the resonance are constants
of the module; a preference for them, if ever wanted, is a later decision.
