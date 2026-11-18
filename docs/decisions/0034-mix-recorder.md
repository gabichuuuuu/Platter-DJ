# 0034. The mix recorder: a queue out of the processor and a worker per recording

Status: accepted (M4).

## Context

The mix recorder writes the master into files while the DJ plays
(`docs/behavior/recording.md`). The audio callback may not allocate, lock, wait or touch the
disk, and a disk can stall for a moment. A cut must close one file and open the next without
losing or doubling a frame, and a stop must finish the header of the WAV file. The engine is
built again on a change of sample rate, and the application may close while a recording runs.

## Options

- **Where the samples leave the processor.** A queue allocated with the processor and kept for
  its whole life, or one built for each recording and sent in, which would have to come back
  through the retired queue to be freed. The first costs memory while nothing is recorded; the
  second changes the shape of `Retired`, which the effect chain also builds.
- **What the queue carries.** Interleaved samples with the start and the end told through
  atomics, or items that are a frame or a mark. Atomics cannot say where in the stream a
  recording starts; marks travel in order with the frames.
- **Where a cut happens.** As a mark from the processor, or in the worker, between two frames it
  takes from the queue. The worker sees every frame once, so a cut there loses nothing either,
  and the audio thread does not take part.
- **The format.** 16 or 24-bit PCM needs a choice of dither; 32-bit float keeps the output as
  it is, which `WavWriter` already writes for the render and which the tests can compare bit for
  bit.

## Choice

- `rt::channel` allocates a queue of `RECORDER_QUEUE_SECONDS` (4) seconds of frames at the rate
  of the engine, 2.3 MB at 48 kHz. The processor keeps its producer in `rt::recorder::Tap`; the
  engine keeps the consumer and lends it to the worker of each recording, which gives it back
  when it returns. A recording starts only while the engine holds it.
- The queue carries `Recorded::Start(n)`, `Recorded::Frame` and `Recorded::End(n)`, where `n`
  numbers the recordings of the engine. The tap sends the master after the limiter, before the
  blend of external mode, frame by frame. A frame that finds the queue full, or a mark still
  waiting for room, is counted as dropped. A mark that finds no room waits for the next frame;
  an end waits before a start, and no frame goes out while either waits.
- The worker takes the queue in batches of 4096 items and writes through a `BufWriter`, since
  `WavWriter` writes four bytes at a time. It skips whatever belongs to another recording, cuts
  on an order from the control side after taking everything queued until then, splits at
  `SPLIT_BYTES` (2048 MiB) the same way, and returns on its end mark, two seconds after a stop
  that got no end mark (the processor does not run without an output), or at once when the
  engine is dropped. Files are opened with `create_new`, so nothing is written over.
- A worker that cannot open or write its file reports the reason; the engine stops the
  recording, and the worker goes on emptying the queue until the end, so the queue never stays
  full.

## How it was checked

- `crates/engine/tests/recorder.rs` compares the files of a recording with a cut against what
  `process` returned, sample for sample, and checks a folder that cannot be made, the refusals,
  a dropped engine and the stop at closing with the processor on another thread.
- `crates/engine/src/recorder.rs` splits at a small size and skips frames of another recording;
  `crates/engine/src/rt/recorder.rs` fills a queue of three items.
- `crates/engine/tests/realtime_memory.rs` records through its whole command sequence, with a
  cut, and then fills the queue while the worker is held: `process` neither allocates nor frees.
- Not measured: how long a real disk stalls while a set is recorded, and whether four seconds
  of queue is enough on the machines DJs use.
