# Architecture

This document describes the design the milestones follow. The crates of M0, M1 and M2 exist
today; a crate is created when it has code, in the milestone the table names.

## Crates

| Layer | Crate | Content | Milestone |
| --- | --- | --- | --- |
| L0 | `platter-core` | Shared types: sample rate, position in samples, tempo, gain, deck and track identifiers, common errors | M1 |
| L1 | `platter-dsp` | Processing blocks without global state: interpolation, EQ, filter, parameter smoothing, meters, limiter | M1 |
| L1 | `platter-decode` | From a file to `f32` PCM in memory, with basic metadata | M1 |
| L1 | `platter-analysis` | Tempo, beatgrid, key, waveform peaks | M3 (waveform peaks live in the engine until then) |
| L1 | `platter-library` | Collection, playlists, history | M3 |
| L1 | `platter-control` | Mappings: from MIDI messages to commands, with modifiers | M3 (the keyboard of M2 lives in the interface crate) |
| L1 | `platter-stems` | Stem separation | M4 |
| L2 | `platter-engine` | Decks, mixer, command registry, real-time processor, background track loading, three-band waveform peaks, render without a device | M1 |
| L3 | `platter-audio-io` | Devices and the output stream with cpal; a null backend for tests | M2 |
| L3 | `platter-device-io` | MIDI and HID ports | M3 |
| L3 | `platter-ui-egui` | Panels, waveforms, shortcuts, settings | M2 |
| L3 | `platter-mcp` | MCP server and JSON control channel | M4 |
| App | `platter` (`apps/platter`) | Desktop application | M2 |
| App | `platter-cli` (`apps/platter-cli`) | `render`, `info` | M1 |
| Test | `platter-testkit` | Signal generators, comparators, test allocator | M1 |
| Tool | `xtask` | `ci`, `layers`, `cleanroom`, `privacy`, `parity` | M0 |

## Layer rules

`cargo xtask layers` builds a model of the workspace from `cargo metadata` and fails when a rule
is broken. The layer of each crate is a table in `xtask/src/layers.rs`; a workspace crate missing
from it fails the check.

- A crate depends only on lower layers. Crates of the same layer do not depend on each other;
  in particular, L1 crates are independent.
- Nothing below L3 pulls in `egui`, `eframe`, `winit`, `wgpu`, `cpal`, `midir` or `hidapi`,
  directly or through another dependency.
- `platter-testkit` appears only as a dev-dependency, and builds on L0 only.
- No crate depends on `xtask`, and `xtask` depends on no workspace crate.
- The real-time module of `platter-engine` (`crates/engine/src/rt/`) uses only `platter-core`
  and `platter-dsp` among the workspace crates, and nothing of the engine outside itself. The
  check reads the sources of the module, which must stay a flat folder.
- Every direct external dependency has a row in `docs/dependencies.md`, and every row there
  matches a dependency in use.

The reasons for the shape of the check are in `docs/decisions/0002-layer-check-model.md`.

## Threads and data flow

```
interface / CLI -- command --> control side of the engine -- RtCommand (rtrb) --> processor
      ^                              |  ^                                            |
      |                              |  +---- buffers to free (rtrb) <---------------+
      +---- state snapshot <---------+------- positions, levels, dropouts (atomics / rtrb) <--+
                                     |
                              worker threads: decoding, waveform peaks
```

- **Processor.** `process(&mut self, output: &mut [f32], channels)`. It does not know cpal. The
  callback of `platter-audio-io` and the deviceless render of the CLI call it with the same
  code. Everything it does follows the real-time rules in `AGENTS.md`.
- **Control side.** It validates commands, starts track loading, translates to `RtCommand`
  (plain data, no allocation) and publishes the snapshot the interface reads.
- **Worker threads.** Decoding, analysis and waveform peaks never run on the audio thread or on
  the interface thread. There is no async runtime: background work uses standard threads and
  channels.
- **Mix recorder.** The processor hands each frame of the master to a queue of its own; a
  worker thread of each recording writes it into files, so the disk never holds up the audio
  (`docs/decisions/0035-mix-recorder.md`).
- **Loaded track.** Complete stereo `f32` PCM in memory, shared through an `Arc`. This is the
  simple option for M1 and M2; it stays behind a project type so it can become a block cache.
- **Sample rate.** The engine runs at the rate of the device. A track is read at the ratio
  `tempo * (track_rate / engine_rate)` with interpolation; it is not resampled on load. The
  interpolation is chosen by measurement (distortion and aliasing of a sine) and recorded in an
  ADR. With key lock on, a stretch behind the trait `Stretch` of `platter-dsp` plays the
  track instead, at the tempo of the deck and the pitch of the track
  (`docs/decisions/0021-key-lock-stretch.md`).
- **Beatgrid.** `BeatGrid` in `platter-core`: a tempo, up to 128 markers and a lock, in a
  fixed-size value. In M3 the markers share the one tempo; M4 gives each marker its own (a
  flexible grid). The processor holds the grid of each deck and reports it to the control side
  after each change (`docs/decisions/0014-beatgrid-in-the-processor.md`).
- **Loops and jumps.** The processor holds the loop of each deck too. The return of a loop to
  its start and a beatjump move the deck at once, to the frame, and crossfade from what it was
  playing; a seek fades out and in instead
  (`docs/decisions/0015-jumps-that-keep-the-time.md`).
- **Stored cues.** `Cues` in `platter-core`: eight slots and the selected one, in a fixed-size
  value with the names in place. The processor holds the cues of each deck beside its grid
  and its loop, so a store and the edits after it act in the order they were sent, and
  reports them the same way (`docs/behavior/cues.md`).
- **Sync.** The processor holds the master clock, a tempo and a place in the beat, and
  chooses the tempo master each block. A synced deck takes its speed from the clock each
  frame and, in the beat mode, keeps its beats on those of the clock. The tempo master, the
  tempo of the clock, and the sync, the speed and the phase of each deck go to the control
  side through the atomics (`docs/decisions/0016-master-clock-in-the-processor.md`).

## Engine

`Engine::new` returns the control side (`Engine`) and the real-time side (`Processor`). The
concrete shapes of commands, queues, track memory and the snapshot, and how each was checked,
are in `docs/decisions/0009-engine-control-and-snapshot.md`. The snapshot carries where every
control stands, so the interface draws its knobs and faders from it and keeps no copy. The
worker thread that decodes a track also computes its waveform peaks in three bands
(`docs/behavior/waveform.md`).

## Audio output

`platter-audio-io` runs a source of audio, which in the application is the processor, from
the callback of a device opened through `cpal`, or from a clock when the system has no device.
One driver serves both: it asks the source for the master and, when the routing gives them
outputs of their own, the headphones, and measures load and dropouts. A stream hands its
source back when it closes, so the device can change while the engine keeps its tracks; only a
change of sample rate builds the engine again. Details and what was measured:
`docs/decisions/0010-audio-output.md`.

## Commands

Every action visible to the user is a command with a stable identifier such as
`deck.play_pause`, a label, typed parameters and tests. Keyboard, interface and CLI emit the same
commands; MIDI and MCP will too. The interface reads a state snapshot and acts through commands;
it holds no engine state. The list of commands is the table `SPECS` in
`crates/engine/src/command.rs`; their behaviour is described in `docs/behavior/`.

## Mixer (M1 and M2)

Per channel: input gain, three-band EQ with full kill and a kill button per band, a one-knob
filter (low-pass one way, high-pass the other), pan, fader and a peak meter. A crossfader with a
selectable curve. A master section with gain, a safety limiter and a meter. Cue monitoring uses
outputs 3 and 4 when the device has four channels or more; otherwise it is omitted and the
interface says so.

## Interface

`egui` with `eframe` and `wgpu`, as a thin layer over the engine. The layout follows the zones a
DJ expects in this order: status header, global section, waveforms, decks, mixer, browser. The
look comes from `docs/design/`: `tokens.toml` is the single source of colours, sizes and text
styles.

Each frame the application (`apps/platter`) builds a view from the snapshot of the engine, the
state of the audio output, the settings and the file list. `platter-ui-egui` draws it and
answers with actions: commands of the engine and commands of the application. The application
carries each one out in one place. The widgets are the project's own, drawn with the painter
from the tokens; the icons and the mark are read from their shape data; every text comes from
a translation file. Reading folders and file headers for the file selector happens on worker
threads. The shape of all this and how it is tested without a display:
`docs/decisions/0013-interface.md`, with `0011-icons.md` and `0012-translations.md`.
