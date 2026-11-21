# 0040. Audio input: a second stream, a queue and a steady delay

Status: accepted (M5).

## Context

M5 adds the input routing: decks that play an input of the audio device, an auxiliary input
for a microphone and, later, timecode, the send of the effect units and the external source of
the recorders (`docs/behavior/live-input.md`). Until now Platter only opened an output stream.

`cpal` 0.18.2 opens input and output as two streams, each with its own callback on its own
thread (`DeviceTrait::build_input_stream` and `build_output_stream`). On Windows the inputs and
the outputs of one interface are two devices of the system, and nothing guarantees that the two
callbacks run at the same pace: two devices may run on two clocks that drift apart by a few
parts in ten thousand, and even one interface delivers its input in bursts of its own size.
The real-time rules hold in both callbacks (`AGENTS.md`, *Real time*).

## Options

- **Where the input is mixed.** In the input callback, which would have to reach the
  processor, owned by the output callback; or in the output callback, with the input carried
  across by a lock-free queue. Only the second keeps one owner for the processor.
- **What crosses the queue.** Samples with the channel count of the device, which makes the
  reader depend on a count that changes with the device; or frames of a fixed width of eight
  channels, the most Platter opens, with the missing channels silent. Fixed frames make each
  push and pop one copy of a small value.
- **Drift.** Ignore it, and let the queue empty or fill up, which gives a gap and a jump every
  few minutes between two clocks; drop or repeat a frame when the queue leaves a window, which
  is a small click each time; or read the input a little slower or faster, through the cubic
  interpolation Platter already uses for tempo (`platter-dsp`, `interpolate`), so the queue
  holds a steady amount.
- **Rate.** Resample an input that runs at another rate, or open the input at the rate of the
  output and refuse a device that does not accept it.

## Choice

- A second stream in `platter-audio-io`, beside the output and like it: `cpal` stays in the
  `device` module, the callback gets the samples as `f32` through a function the application
  gives (a *sink*, the mirror of the *source* of the output) and the function comes back when
  the stream closes, as the source of the output does.
- The engine builds the queue once, with the processor: frames of eight channels, room for a
  quarter of a second. The writing end goes to the sink of each input stream in turn and the
  reading end stays in the processor, so opening and closing inputs allocates nothing on either
  audio thread.
- The processor reads the input once per output frame through a reader that keeps the queue
  near a target: the frames of one input buffer and one output buffer, the least that lets the
  two callbacks take turns, and one more output buffer for a callback that comes late; never
  less than two output buffers and that margin. A first version without the margin ran dry
  each time a slower writer skipped a callback. The fill level, smoothed over about a second, sets a reading speed
  between 0.995 and 1.005 of the nominal one, by a proportional and an integral term, and the
  cubic interpolation reads between the frames. The correction is far below what an ear tells
  as a change of pitch.
- When the queue runs dry the reader fades the input out, counts the gap and waits for the
  target to fill again before it fades the input back in, so a stopped device gives silence
  and a restart does not click.
- The input opens at the rate of the output. A device that does not accept it is refused with a
  message; resampling between rates can come later, behind this reader.

## How it was measured

With synthesised signals only, in the tests of `platter-engine`:

- a writer that runs 0.2 % faster and one that runs 0.2 % slower than the reader, in callbacks
  of 256 frames for 60 seconds: no gap is counted, in the last second the fill level stays on
  average within a block of the target, and the reading speed is within 0.05 % of that of the
  writer;
- a writer that stops: the input fades to silence, one gap is counted, and it returns when the
  writer resumes;
- a sine of 1 kHz through the reader at the nominal rate keeps its level within 0.1 dB;
- the processor with an input routed to a deck and to the auxiliary input does not allocate in
  `process` (`realtime_memory`).

Not measured: a real device, two devices on separate clocks, the delay from input to output
on hardware.
