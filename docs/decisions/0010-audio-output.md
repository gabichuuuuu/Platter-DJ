# 0010. Audio output: the driver, the deviceless output and what the device decides

Status: accepted (M2).

## Context

`platter-audio-io` connects the processor of the engine to a sound card through `cpal`. The
architecture fixes that the processor does not know the device library and that the same code
runs in the callback and in the deviceless render. It leaves open how the callback is built,
what happens without a device, and how load and dropouts are measured.

## Choices

**A source, not the engine.** The crate takes a function that fills a block of samples. The
application passes the processor; the tests pass a counter. The crate does not depend on
`platter-engine`.

**One driver for every output.** `OutputDriver` is what the callback runs. It asks the source
for two channels, or four when the routing gives the headphones a pair of their own, and
writes each on the output the routing names. A callback larger than its scratch buffer is
served in pieces of 1 024 frames, so no size a device asks for needs memory. The same driver
runs under a real device, under the clocked output and under `ManualStream`, which the tests
pump with blocks of any size.

**The clock read.** The real-time rules forbid system calls in the callback. The driver reads
the monotonic clock twice per callback (`std::time::Instant`) to measure the load. On the
three supported systems that read does not enter the kernel in the usual case and does not
block or allocate: it is the performance counter on Windows, the commpage time on macOS and
the vDSO clock on Linux. The load indicator the design asks for cannot be had without it, so
the read stays, here and nowhere else in the callback.

**Dropouts.** A dropout is a callback that took longer than the time of its own buffer, or a
dropout the system reports. A report that follows a late callback within two callbacks is the
same event and counts once. The shared mode of WASAPI does not report output dropouts, so on
Windows the count comes from the timing alone.

**Without a device.** `silent` runs the driver from a thread that sleeps one buffer at a time.
The engine keeps working: tracks load, positions advance, meters read; nothing is heard. The
alternative, an engine with no processor, would fill the command queue and leave the snapshot
frozen.

**The source comes back.** A stream callback owns the driver while it runs. When the stream
closes, or a configuration is refused, the callback is dropped and its `Drop` sends the driver
back through a channel; `Output::stop` returns the source. The application can change device,
buffer size or routing and keep the engine with its loaded tracks. Only a change of sample
rate needs a new engine.

**Configuration.** Among the configurations a device offers at the sample rate asked for, the
driver takes `f32` when there is one, then `i32`, then `i16`, with the most channels up to
eight. The buffer size asked for is tried first; if the device refuses it, the stream opens
with the size the device prefers, and the interface shows the size in use.

## What was measured

On Windows x64 with WASAPI in shared mode, `cargo run -p platter-audio-io --example probe`
opens the default device at 48 kHz, asks for 256 frames and gets 480: the stream reports
10.0 ms per buffer. The backend of `cpal` 0.18.2 states the reason in its source: in shared
mode the callback period is the device period whatever is asked for. A smaller period on
Windows needs exclusive mode or ASIO, and `AGENTS.md` keeps the `asio` feature off. The buffer
size setting therefore has no effect below the device period on Windows; it is passed on for
the systems where it has one.

With a source that writes silence the load reading stays under 1 % and no dropout is counted
over the half second the example runs.

## How it was checked

- `crates/audio-io/tests/null_backend.rs`: blocks from 0 to 4 096 frames give one continuous
  stream; the routing puts each channel on its output and drops a channel routed beyond the
  device; a slow source is counted as a dropout; the clocked output runs at the pace of a
  device and returns its source.
- `crates/audio-io/src/driver.rs`: sample conversion saturates; the load reading and the
  dropout count.
- The device backend is not covered by tests: CI has no sound card. The `probe` example is
  its manual check.
