# 0039. Key lock on worker threads at the priority of audio

Status: accepted (M4).

## Context

Key lock plays each key-locked deck through a time stretch (`docs/decisions/0021-key-lock-stretch.md`),
and a stem deck through four, one per stem. Until now every stretch ran on the audio thread. On
21 November 2026, Windows x64, release build, blocks of 256 frames at 48 kHz, tempo 1.05, each
stretch took 4 to 5 % of the time of that thread: four stem decks with key lock took 70 to 85 % on
average and overran the period of the device in 500 to 1400 blocks of 3750, so they would drop out.
The cost is steady, not in bursts: spreading the work of a stretch over the blocks changed nothing
measurable. The documented application has a preference of multi-core processing that its manual
recommends for key lock on all decks.

A worker thread at the normal priority is not enough. With every processor busy, a worker that
asked to sleep 1 ms woke up to 32 ms later (99 % within 29 ms); raised through MMCSS to the
priority of audio, within 2.2 ms (5 ms with twice as many busy threads as processors). Raising the
priority of a thread needs calls to the system that Platter cannot make without `unsafe`.

## Options

- **Keep the stretches on the audio thread** and document the limit: one stem deck with key lock,
  or four track decks.
- **Worker threads at the normal priority**, with a reserve of more than 60 ms: a change of tempo
  heard that late, and still a risk of falling behind under load.
- **Worker threads at the priority of audio** through the crate `audio_thread_priority` (MPL-2.0,
  by Mozilla, the one cpal uses for its own callback when its feature `realtime` is on): MMCSS on
  Windows, the time-constraint policy on macOS, SCHED_FIFO on Linux. The crate `thread-priority`
  (MIT) was also read: on Windows it raises the priority inside the normal class, without MMCSS,
  and on macOS it does not use the time-constraint policy.
- **Own calls to each system**, in a crate of their own with an exception to the rule against
  `unsafe` written in `AGENTS.md` first. Possible later; not now.

## Choice

- `platter-engine`, module `stretch_pool`, the only one that names `audio_thread_priority`: one
  worker for every four processors, from one to four, each raised to the priority of audio when
  the system allows it.
- A `Remote` stands for each stretch on the audio thread and implements the trait `Stretch`, so the
  key lock of a deck does not change. It takes the frames the worker rendered from a lock-free
  queue (`rtrb`) and sends seeks and changes of tempo and pitch through another. Nothing on the
  audio thread allocates, locks or waits.
- The worker renders 16 ms ahead: twice the latest wake-up measured, plus a block and a pass. A
  change of tempo or pitch is stamped with the frame of output it applies at, the reserve and 1 ms
  after the frame being played, and asked only when the position in the track crosses a multiple
  of 64 frames: the stretch schedules its changes once a chunk of 64 frames anyway, and the
  position is the same for every stem of a deck. The four stretches of the stems of a deck thus
  change on the same frame, whichever worker runs each. A deck glides its tempo, so asked on every
  frame the changes filled the queue before a worker woke, and each stem lost different ones.
- A stretch keeps the changes it asked and that are not heard yet. When it starts again alone,
  after it fell behind, it starts at the tempo heard now and asks them again on the same frames,
  so it does not drift from the other stems of its deck through a glide of tempo.
- Each `Remote` keeps its pool alive: turning the preference off only stops handing new stretches
  to the workers, and a track that plays keeps its key lock.
- Seeks carry a count; what was rendered before the last seek is dropped, and of several waiting
  seeks a worker does only the last. A full queue never blocks: a seek waits for room, a change
  is asked again on the next boundary.
- A worker tops up every stretch that plays before it does a share of the work of one that seeks,
  so the four stems of a deck that seek at once do not starve the others.
- A preference of the Audio page turns it on or off, on by default. The engine starts without it.
  The tests that run faster than real time use a pool without threads that they run between two
  blocks, so they do not depend on the clock; the threads themselves are checked by a unit test
  and by the measurement below, which is not part of the gate.
- A worker the system does not raise to the priority of audio runs at the normal priority, and
  the application writes it in its log.
- The stretch and its FFT are optimised in the builds of the tests (`Cargo.toml`, the profile
  `dev`): unoptimised, a worker took longer than the 32 ms a deck waits after a seek.

## How it was measured

- Wake-up of a thread asked to sleep 1 ms, 10 s each, Windows x64, 20 threads: idle, 1.6 ms at the
  median and 5.8 ms at worst at the normal priority, 3.2 ms at worst raised; with 20 busy threads,
  32 ms at worst at the normal priority and 2.2 ms raised; with 40 busy threads, 5.1 ms raised.
- `cargo test -p platter-engine --release --test stems cost -- --ignored --nocapture`, at the pace
  of a device: four stem decks with key lock on worker threads leave 10 to 11 % of the audio thread
  on average and overran no block of 3750; in four runs of 20 s, with other programs taking about
  four processors, the workers fell behind in two, once each, for 8 and 9 frames.
- `crates/engine/tests/keylock.rs`, a deck at tempo 1.08 on worker threads: the pitch of its 440 Hz
  sine within 5 cents, the tempo within 10 ms over 2 s, no frame missing.
  `crates/engine/tests/stems.rs`: four stems of a quarter of one sine each, through thirty changes
  of tempo, add up to the full sine within 0.01 of its level.
- Not yet: macOS and Linux, a real device, and listening.
