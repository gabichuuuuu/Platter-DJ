# 0009. Shape of the engine: commands, real-time side and snapshot

Status: accepted (M1).

## Context

`docs/architecture.md` fixes the outline: a control side that validates commands, a processor
that runs in the audio callback, lock-free queues between them and a snapshot for the
interface. It leaves the concrete shapes open.

## Choices

**Commands.** `Command` is an enum with typed fields. A table of specifications (`SPECS`)
gives each command its stable identifier, a label and the names and kinds of its parameters.
`Command::from_args` builds a command from an identifier and untyped values, and rejects a
parameter the command does not declare. The CLI script uses it today; keyboard shortcuts,
MIDI mappings and MCP will use the same entry. Ranges are checked in one place, when the
control side dispatches the command; an invalid command changes nothing.

**Two values from one constructor.** `Engine::new` returns the control side and the
`Processor`. The processor is moved to whoever drives the audio. The two share a block of
atomics and two `rtrb` queues: commands one way, retired buffers the other. M3 adds a third,
for the beatgrids (`docs/decisions/0014-beatgrid-in-the-processor.md`).

**Track memory.** A loaded track is stereo `f32` PCM behind an `Arc`. It reaches the processor
inside a command and leaves through the retired queue, one item per swap, so the buffer is
always freed by the control side. The retired queue has room for every swap that can be in
flight, and the control side refuses a swap beyond that, so the processor never has to choose
between blocking and freeing. If that reasoning were ever wrong the processor leaks the
buffer instead of freeing it.

**Changes that wait for silence.** A track change and a seek wait for the 5 ms fade-out and
are applied at a gain of exactly zero. A play or a seek that follows a load applies to the
track that arrives.

**Per-sample processing.** The processor computes one frame at a time through the whole
chain, with every parameter on a linear ramp. It needs no scratch buffers and makes the ramps
exact. Block processing is a later optimisation if M2 measures a need.

**Snapshot.** `Engine::snapshot` builds a plain value from the atomics (position, cue,
playing, levels, limiter gain, written once per block) and from what the control side knows
(track details, loading). M1 carries what the tests and the CLI need; M2 adds the parameter
values the interface must draw.

**Loading.** One worker thread per load. A counter per deck marks stale results, so a slow
decode never replaces a newer choice. `wait_for_loads` exists for the deviceless render, which
must not depend on decoding speed.

**Waveform peaks.** The architecture lists three-band waveform peaks in the engine. They are
computed for the interface, which arrives in M2; M1 has no reader for them, so they are
written in M2.

## How it was checked

- `crates/engine/tests/realtime_memory.rs` runs the processor under the counting allocator
  through every command, a track change while playing and an eject: zero allocations and zero
  frees. Replacing the retired queue by a plain drop makes the test fail.
- `crates/engine/tests/command_sweep.rs` sends random command sequences, valid and invalid:
  no panic, every sample finite and inside full scale.
- `crates/engine/tests/render.rs` renders the same plan twice and compares the files byte for
  byte.
- `cargo xtask layers` checks that `crates/engine/src/rt/` names no workspace crate other than
  `platter-core` and `platter-dsp`, and nothing of the engine outside itself.
