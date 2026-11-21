# 0041. MIDI clock: ticks from the audio thread, sent and read on a thread of their own

Status: accepted (M5).

## Context

M5 sends the master clock as MIDI clock and lets it follow a clock received
(`docs/behavior/midi-clock.md`). A Timing Clock message marks a 24th of a beat: at 120 BPM one
every 20.8 ms, at 180 BPM one every 13.9 ms. A device that follows hears the jitter of the
messages as a wobble of its tempo, so they should leave within a millisecond of their time.

The beats of the master clock exist only on the audio thread, one block at a time
(`crates/engine/src/rt/clock.rs`): the processor knows on which frame of a block a beat falls,
not when that frame is heard. Sending a message is a system call, which the audio thread may not
make (`AGENTS.md`, *Real time*). `midir` 0.11.1, the approved MIDI dependency
(`docs/dependencies.md`), opens outputs (`MidiOutput::connect`, `MidiOutputConnection::send`)
as well as inputs, and gives each message received a timestamp in microseconds from a point that
does not move while the connection is open (`MidiInput::connect`).

## Options

- **Where the ticks are found.** On a thread that runs its own copy of the clock from the tempo
  in the snapshot, which drifts from the audio and misses the beats of a deck that is the tempo
  master; or on the audio thread, where the clock is, as frame numbers.
- **When a frame is heard.** The frame counter and the time of each callback, read by the driver
  of the output as it already reads the time for the load of the callback
  (`crates/audio-io/src/driver.rs`), plus the latency of the output; or the timestamps of `cpal`,
  which differ in meaning between its backends.
- **Jitter of the callbacks.** A callback starts late by up to a fraction of a buffer. Taking each
  callback time as it is carries that lateness into the ticks; a second-order delay-locked loop
  over the pairs of frame and time (F. Adriaensen, *Using a DLL to filter time*,
  <https://kokkinizita.linuxaudio.org/papers/usingdll.pdf>, sections 2 to 4) gives a steady line
  of time against frames. For a loop run `F` times a second with a bandwidth `B`, it takes
  `ω = 2πB/F`, `b = √2·ω` and `c = ω²`; each period, with the error `e` of the time read
  against the time predicted, the predicted time moves by `b·e` plus the period estimate, and
  the period estimate by `c·e`.
- **Reading a clock.** The time between two messages alone, which carries the jitter of the
  sender and of the system; or the same loop over the timestamps of the messages, which gives the
  period and the time of the next beat.

## Choice

- The processor finds each 24th of a beat the master clock crosses, and the Start the user asked
  for on the next beat, and pushes them as frame numbers into a lock-free queue, as it pushes the
  frames of the recorder. It also publishes, in an atomic, the frame each block begins at.
  Nothing else changes on the audio thread, which makes no system call for any of it.
- A sender thread of the application looks at that frame count about once a millisecond and
  stamps each new value with its own clock. The pairs feed a delay-locked loop with a bandwidth
  of 1 Hz (`platter_engine::midi_clock::FrameClock`), which turns the frame of each message into
  a time; the thread adds the latency of the output and the offset, waits until then and sends
  the message through a `midir` output in its own module of `platter-device-io`. No type of
  `midir` leaves that module, as for the inputs. The loop starts again when the frames go back
  or an observation lies more than 50 ms off the line: a new engine, or a stall.
- The stamp of the sender is late by the time between the start of the callback and its look,
  up to a millisecond and a fraction of a buffer; the loop smooths the variation and keeps the
  mean, which the offset of the preferences takes away.
- A jump of the phase of the master clock by more than three 24ths in one frame is not sent as a
  burst: the count goes on from there.
- Following a received clock is the next step: an input connection of its own that does not
  ignore timing messages, the same loop over the timestamps `midir` gives, and the phase of the
  master clock aligned to the count since Start.
- `docs/dependencies.md` records that `midir` also opens outputs for this.

## How it was measured

With synthesised timings only, in the tests:

- the messages of the processor at 120, 128 and 174 BPM, with a bend of 4 % and with a deck at
  125 BPM as the tempo master: 24 to a beat, Start with the first one, the mean spacing within
  0.1 frame of the tempo and each tick within a frame of its 24th (`tests/midi_clock.rs`);
- the loop fed with blocks of 256 frames at 48 kHz from a device clock 0.01 % fast, each seen up
  to a quarter of a buffer late: after ten seconds, the time it gives a frame varies by less
  than 0.25 ms (`src/midi_clock.rs`);
- the processor sending the clock with nobody reading the queue does not allocate
  (`realtime_memory`).

Not measured: a real MIDI output, the delay of the system between `send` and the cable, how long
the thread oversleeps on each system, and a device following the clock.