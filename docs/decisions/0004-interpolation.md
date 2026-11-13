# 0004. Interpolation for reading a track

Status: accepted (M1).

## Context

The engine runs at the sample rate of the device and reads each track at the ratio
`tempo * (track_rate / engine_rate)`, so almost every output sample falls between two stored
samples. The read runs per sample inside the audio callback, for every deck.

## Options

- **Linear**, two samples. The cheapest; it dulls the top of the spectrum and leaves images.
- **Cubic Hermite with Catmull-Rom tangents**, four samples. A handful of multiplications.
- **Windowed sinc**, tens of samples per output sample and a coefficient table. The cleanest,
  and the only one that can also filter before reading faster than the original speed.

## How it was measured

A sine at 48 kHz is read at a fixed ratio; a sine of the expected frequency is fitted to the
output by least squares, and the ratio between the fitted sine and everything else is the
figure (signal to distortion, images and noise). Test:
`crates/dsp/src/interpolate.rs`, `the_cubic_is_cleaner_than_the_line`.

| Sine | Linear | Cubic |
| --- | ---: | ---: |
| 100 Hz | 103.9 dB | 145.6 dB |
| 1 kHz | 63.9 dB | 91.7 dB |
| 5 kHz | 35.5 dB | 48.9 dB |

The figures are the same for a ratio of 1.06 (tempo +6 %) and of 0.919 (a 44.1 kHz track on a
48 kHz engine).

## Choice

Cubic Hermite. It gains 13 to 42 dB over linear for a cost that does not show in the callback.

## Known limits

- Above a few kilohertz the residual is still well above the noise floor of 16-bit audio.
- Nothing filters the track before it is read faster than its original speed, so content
  near the top of the spectrum aliases when the ratio is above one.

Both call for a windowed sinc reader. It is left for the milestone that adds key lock (M3),
which rebuilds the read path anyway; the test above then measures the replacement.
