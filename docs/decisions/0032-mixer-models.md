# 0032. Models of the channel equaliser and filter

Status: accepted (M4).

## Context

The public manual offers several equaliser types and filter types on the Mixer page of the
preferences (`docs/behavior/mixer.md`). It describes each type only by its sound and by the
mixers it is modelled on, with two exceptions: one equaliser is a full-kill isolator with
slopes of 24 dB per octave, one has a fourth band, and of the filters one is a ladder with a
steep response and a lot of resonance, one has a mellow slope and little resonance, and one is
steep without much resonance. The inventory (`docs/parity.toml`) asks for four equaliser
models and three filter models in all.

Platter had one of each: the isolator of `docs/decisions/0005-equaliser-topology.md` and the
state-variable filter of `docs/decisions/0006-channel-filter.md`. Every equaliser model must
keep their contract: flat at the centre, a band at zero removed, all bands at zero exact
silence. Every filter model must keep its own: bit for bit at the centre, the blend zone
around it, and a sweep of the whole travel that stays finite and does not click.

## Options

- **Shelving and peaking equalisers**, as on many hardware mixers. A shelf cuts by a finite
  amount, so the kill buttons and the knob at zero would no longer remove a band.
- **Band splits of different steepness.** The same three bands at 250 Hz and 2.5 kHz, with
  crossovers of first, second, fourth or eighth order. Each order adds up to a flat magnitude,
  so every model keeps the contract, and the steepness is what a DJ hears: how much of the
  neighbouring band a cut takes with it.
- **A fourth band.** It needs a fourth knob per channel, which the design system does not
  draw. Left out.
- **Filters:** state-variable sections of second order (the first model) and of fourth order,
  and a four-stage ladder with feedback, all in their trapezoidal form so the corner can move
  every sample.

## Choice

Equaliser models, all band splits at 250 Hz and 2.5 kHz:

| Model | Crossover | Edge | Why it adds up flat |
| --- | --- | --- | --- |
| `smooth` | First order | 6 dB per octave | A first-order low-pass and high-pass sum to the input exactly |
| `soft` | Second-order Linkwitz-Riley | 12 dB per octave | The high-pass edges are inverted, so each pair sums to an all-pass |
| `isolator` | Fourth-order Linkwitz-Riley | 24 dB per octave | The first model, unchanged |
| `steep` | Eighth-order Linkwitz-Riley (two fourth-order Butterworth filters in series) | 48 dB per octave | Each pair sums to an all-pass |

Filter models, sharing the knob, the ranges and the blend zone of the first one:

| Model | Design | Slope | Corner |
| --- | --- | --- | --- |
| `gentle` | Second-order state-variable filter, Q 0.9 | 12 dB per octave | About +0.7 dB |
| `steep` | Two state-variable filters with the Q of a fourth-order Butterworth filter | 24 dB per octave | No emphasis |
| `ladder` | Four one-pole stages, feedback 2 | About 22 dB per octave near the corner | About +8 dB |

- The models are one setting for every channel, as on the Mixer page of the manual:
  `mixer.eq.model.set` and `mixer.filter.model.set`. `isolator` and `gentle` stay the
  defaults, so nothing changes for a user who does not choose.
- A change fades over 10 ms: the outgoing and the incoming model run side by side and are
  blended, and the incoming one starts from a cleared state. A change asked for during a fade
  waits for it, so no more than two models run at once. Every model is built before the audio
  starts; a change allocates nothing.
- The ladder solves its feedback loop per sample (zero-delay feedback), and its input is
  multiplied by one plus the feedback so the pass band stays at unity.
- The names are Platter's own and describe the response; none names a mixer or a maker.

## How it was measured

Tests in `crates/dsp/src/eq.rs` and `crates/dsp/src/filter.rs`, with sines:

- Every equaliser model at unity, at 44.1, 48 and 96 kHz: within 0.05 dB at 30 Hz, 250 Hz,
  790 Hz, 2.5 kHz and 15 kHz.
- Low band removed, two octaves below its edge (62.5 Hz) at 48 kHz: -12.3, -24.6, -48.2 and
  -96.3 dB for the four models. Mid band removed, halfway between the edges (790 Hz): -20.9,
  -15.3, -34.3 and -74.3 dB. The smooth model cuts deeper there than the soft one because its
  two neighbours overlap with opposite phase.
- All bands at zero: exact silence in every model.
- Filter half way to the left (corner near 1.1 kHz), at 44.1, 48 and 96 kHz: pass band within
  0.5 dB at 100 Hz; the octave from 2.2 to 4.4 kHz falls by about 12.5 to 13 dB (gentle), 24
  to 25 dB (steep) and 21.5 to 22.5 dB (ladder); the highest level around the corner is +0.68,
  -0.12 and about +7.8 dB.
- A sweep of the whole travel over a 200 Hz sine at 0.5: the gentle and steep models stay
  below 0.55, the ladder peaks at 1.24 where its corner crosses the tone. No step is larger
  than four times the largest step of the input.
- A change of model every 100 ms over a playing sine: no step larger than 1.5 times (the
  equaliser) or 2 times (the filter) the largest step of the input.
- `crates/engine/tests/realtime_memory.rs` changes both models while the decks play, with no
  allocation in the audio callback.

Not measured: how each model sounds on music. Nobody has listened to them yet.

## Consequences

- The steep equaliser runs 24 second-order sections per side of a channel instead of 12;
  during a fade two models run. The load of the callback with four decks is still to be
  measured on a device.
- The ladder can add about 8 dB at its corner; the safety limiter of the master holds what
  passes full scale.
- The public manual lists six equaliser types and the inventory counts four. A fourth band
  waits for a design of the mixer channel with a fourth knob.
