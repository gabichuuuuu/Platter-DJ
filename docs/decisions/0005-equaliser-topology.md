# 0005. Topology of the three-band equaliser

Status: accepted (M1).

## Context

The channel equaliser has three bands and each must be removable completely ("kill"), as the
inventory asks. With the three knobs at the centre the equaliser must not colour the sound.

## Options

- **Shelving and peaking filters in series.** Cheap and exactly transparent at the centre, but
  a shelf only attenuates by a finite amount: it cannot remove a band.
- **Band split with a crossover, a gain per band, and a sum.** A gain of zero removes the band.
  The split must add back to a flat response.

## Choice

A band split with fourth-order Linkwitz-Riley crossovers at 250 Hz and 2.5 kHz. Each slope is
two Butterworth sections in series; a low-pass and a high-pass of that kind add up to an
all-pass, flat in magnitude. The low band, which skips the second crossover, passes through an
all-pass with the phase of that crossover, so the three bands stay aligned.

Knob law: position 0 gives a gain of zero, 0.5 gives unity, 1 gives +12 dB. Below the centre
the gain is the square of the travel, so the band closes smoothly down to nothing.

Cost: twelve second-order sections per channel of audio. The load of the callback has not
been measured yet; M2 measures it with a real device, and four decks arrive in M4.

## How it was measured

Tests in `crates/dsp/src/eq.rs`, with sines through the equaliser at 48 kHz:

- All gains at unity: within 0.05 dB from 30 Hz to 15 kHz.
- Low band at zero: more than 60 dB down at 40 Hz, within 0.05 dB at 8 kHz. The same,
  mirrored, for the high band.
- Mid band at zero: more than 30 dB down halfway between the crossovers, where the two
  neighbours add up.
- All bands at zero: exact silence.

## Consequences

- The equaliser is not phase-linear: it shifts phase around the crossovers even at the centre.
  That is inaudible on a single channel and is the usual behaviour of mixer equalisers.
- The filters ring down for about 200 ms after their input stops, at levels below -100 dB.
- Further equaliser models (M4) will sit beside this one behind the same interface.
