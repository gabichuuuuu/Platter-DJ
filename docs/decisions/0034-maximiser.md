# 0034. Maximiser of the master

Status: accepted (M4).

## Context

The inventory asks for an own, configurable maximiser (`docs/parity.toml`, `maximiser`), and
the M4 screen draws a Limiter key in the master column of the four-deck mixer, with the
command `master.limiter.toggle` of the design system. In the public manual that key switches
the limiter of the main output, and a preference chooses its type, one of them a
third-party maximiser with a single gain control (`docs/behavior/master.md`).

Platter already has a safety limiter, last in the path and always on, without look-ahead
(`docs/decisions/0007-safety-limiter.md`). That record left look-ahead to the maximiser,
because the delay it adds matters to a DJ who mixes by ear.

## Options

- **What the key switches.** (a) The safety limiter: off would let a hot mix clip at the
  converter, which `docs/behavior/master.md` rules out. (b) A maximiser in front of the
  safety limiter, which stays. Chosen: (b).
- **Detection.** Sample peaks, which can be guaranteed exactly; or true peaks, measured on a
  signal oversampled four times (Recommendation ITU-R BS.1770, "Algorithms to measure audio
  programme loudness and true-peak audio level", <https://www.itu.int/rec/R-REC-BS.1770>),
  which costs a polyphase filter per channel and still bounds the rebuilt peaks only
  approximately, since the changing gain moves them. Chosen: sample peaks, with a ceiling a
  little under full scale by default.
- **Gain computer.** An attack and release follower in front of a delay, which overshoots
  by however much its attack is too slow; or the minimum of the needed gain over a window,
  smoothed by an average no longer than the window (G. Luff, "Designing a straightforward
  limiter", 2022, <https://signalsmith-audio.co.uk/writing/2022/limiter/>), which cannot
  overshoot. Chosen: the second. The minimum is kept with a monotonic queue in the manner of
  D. Lemire, "Streaming maximum-minimum filter using no more than three comparisons per
  element" (2006, <https://arxiv.org/abs/cs/0610046>), so each frame costs a bounded number of
  comparisons.
- **Delay when off.** A constant delay whether on or off, so the key switches between two
  signals in time; or no delay when off, so the master is what it was before. Chosen: no delay
  when off, and the key off at start, so nothing changes for a user who does not press it.
  The price is the fade between the two, below.
- **The headphones.** With the maximiser on and the master late, a deck pre-listened and heard
  through the master reaches the headphones twice, 1.5 ms apart, and combs. Chosen: the
  pre-listened decks and the ticks wait as long as the master while the maximiser is on.

## Choice

- In the master path: main volume, maximiser, test tone, safety limiter. The drive, from 0 to
  +12 dB, comes first; the ceiling, from -12 to 0 dB of full scale, and the release, from 10 ms
  to 1 s, follow. Defaults: off, 0 dB, -1 dB, 100 ms.
- The window is 1.5 ms (72 frames at 48 kHz, 576 at 384 kHz). For each frame the needed gain
  is the ceiling over the louder side; the minimum over the window is held; the release brings
  the gain back towards one, closing 90 % of the way in the release time, never above the held
  minimum; an average over the window smooths it; the audio comes out one window less a frame
  later, so every gain in the average belongs to a window that holds the frame it scales. Each
  delayed frame carries the ceiling it was measured against, so a ceiling lowered while the
  music plays holds for every frame from the next on.
- The release works on the distance of the gain below one, not on the gain: near one, the
  gain itself stops moving when its steps fall under half of its last digit (in single
  precision, at about 3e-5 below one for a release of 50 ms at 48 kHz). The average is summed
  in integers of 2^-24, rounded down, so it does not drift and returns to exactly one.
- The key fades over 10 ms, the ramp of every mixer parameter; the drive and the ceiling
  ramp too, and a new release applies at once. The limited path runs while the key is off, so
  it is ready when it is switched in.
- The large state (the delay line, the queue of minima and the average, about 18 KB, and 7 KB
  for the delay of the headphones) is in a box built before the audio starts; the processor
  stays under its 64 KB.

## How it was measured

Synthesised signals at 48 kHz, tests in `crates/dsp/src/maximiser.rs` and
`crates/engine/tests/maximiser.rs`:

- No frame of the limited path passes its ceiling by more than one part in a million before
  the final clamp, for a full-scale square with +12 dB of drive, a single sample at 8, a burst
  shorter than the window, a step out of silence and sines from 50 Hz to 5 kHz at +6 dB over
  full scale. In the engine, two decks at +12 dB of gain and main volume come out at or under
  -1 dB, within 0.5 dB of it, with the safety limiter at exactly one.
- Under the ceiling with no drive, the output is the input 71 frames later, bit for bit, in
  the processor and in the headphones, where the deck pre-listened and through the master keeps
  its level. Off, the input passes untouched and without delay.
- Steady sines at +6 dB over full scale: peaks within 0.5 dB under the ceiling and a gain
  reduction of more than 6 dB (tested). In a one-off measurement with the same signals, kept
  in no test: a reduction of 7.0 dB, and a residual around the fitted sine, a measure of the
  distortion the moving gain adds, 28 dB under the tone at 50 Hz, 37 dB at 100 Hz and more than
  150 dB from 1 kHz up, with a release of 100 ms. Low tones distort when limited hard: the
  gain moves within their cycle.
- The release reaches 90 % of the way back in the release time plus 2.25 ms (the hold of the
  window and the lag of its average), within 0.5 ms, for 50 ms, 100 ms and 500 ms.
- After 20 s of hard limiting, the gain returns to exactly one and the output is the driven
  input, bit for bit.
- Switching on and off while a sine plays, no frame moves more than the sine does plus a 480th
  of the gap between the two signals (`max_step`). In the one-off measurement, a 1 kHz tone,
  near opposite phase across 1.5 ms, dipped by 16 dB at the middle of the fade.
- `crates/engine/tests/realtime_memory.rs` switches the maximiser in over a hot mix and
  changes all its settings: `process` neither allocates nor frees.

## Consequences

- With the key on, the master and the headphones are 1.5 ms late, the latency ADR 0007 avoided
  for the safety limiter. The preview player is not delayed.
- Peaks between samples can pass the ceiling; the default of -1 dB leaves room for most music.
- The maximiser has no controls in the preferences and no gain reduction meter; both wait for
  the design system.
- Not measured: listening, and the inter-sample peaks of real music.
