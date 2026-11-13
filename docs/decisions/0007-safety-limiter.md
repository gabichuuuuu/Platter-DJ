# 0007. Safety limiter of the master output

Status: accepted (M1).

## Context

Two channels with gain and equaliser boosts can add up far beyond full scale. Whatever the
mix does, the samples handed to the audio device must stay inside full scale and must be
finite numbers.

## Options

- **Hard clipping.** Guarantees the range and distorts everything above it.
- **Limiter without look-ahead.** Gain drops at once when a frame would exceed the ceiling
  and recovers slowly. No delay. The first frame of an overshoot is scaled exactly to the
  ceiling, which bends the waveform at that instant.
- **Limiter with look-ahead.** The gain starts falling before the peak arrives, so nothing is
  bent, at the price of a delay of a few milliseconds on the whole output.

## Choice

A limiter without look-ahead: stereo-linked gain, instantaneous attack, 200 ms release
towards unity, ceiling at full scale, and a final clamp that covers rounding. A sample that is
not a finite number is replaced by silence before it is measured.

The output delay of a look-ahead matters for a DJ who mixes by ear, and M2 will measure the
latency of the whole path before anything is added to it. This limiter is a guard for
mistakes, not a loudness tool: the maximiser of M4 is the place for look-ahead.

## How it was measured

Tests in `crates/dsp/src/limiter.rs` and `crates/engine/tests/mixer.rs`:

- A signal below the ceiling passes sample for sample.
- A sine at four times full scale comes out with its peaks between 0.95 and 1.0 once settled.
- Two decks at +12 dB of channel gain and +12 dB of main volume: every output sample inside
  full scale, and the published limiter gain below 0.1.
- Inputs of infinity, NaN and 1e30 give finite output inside full scale.

## Consequences

With the limiter working hard, low frequencies modulate the gain and distort audibly. The
snapshot publishes the limiter gain so that the interface of M2 can show when it acts.
