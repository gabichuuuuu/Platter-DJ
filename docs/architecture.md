# Architecture

This document describes the design the milestones follow. Only `xtask` exists today; a crate is
created when it has code, in the milestone the table names.

## Crates

| Layer | Crate | Content | Milestone |
| --- | --- | --- | --- |
| L0 | `platter-core` | Shared types: sample rate, position in samples, tempo, gain, deck and track identifiers, common errors | M1 |
| L1 | `platter-dsp` | Processing blocks without global state: interpolation, EQ, filter, parameter smoothing, meters, limiter | M1 |
| L1 | `platter-decode` | From a file to `f32` PCM in memory, with basic metadata | M1 |
| L1 | `platter-analysis` | Tempo, beatgrid, key, waveform peaks | M3 (waveform peaks live in the engine until then) |
| L1 | `platter-library` | Collection, playlists, history | M3 |
| L1 | `platter-control` | Mappings: from keyboard and MIDI events to commands | M3 (the keyboard of M2 lives in the interface crate) |
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
- The real-time module of `platter-engine` uses only `platter-core` and `platter-dsp`. This rule
  concerns a module, not a crate, so its check arrives with the engine in M1.
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
- **Loaded track.** Complete stereo `f32` PCM in memory, shared through an `Arc`. This is the
  simple option for M1 and M2; it stays behind a project type so it can become a block cache.
- **Sample rate.** The engine runs at the rate of the device. A track is read at the ratio
  `tempo * (track_rate / engine_rate)` with interpolation; it is not resampled on load. The
  interpolation is chosen by measurement (distortion and aliasing of a sine) and recorded in an
  ADR. There is no key lock until M3: pitch follows tempo.
- **Beatgrid.** From M3 a beatgrid is a list of markers, each with its own tempo (a flexible
  grid). The type is created when it is first used.

## Commands

Every action visible to the user is a command with a stable identifier such as
`deck.play_pause`, a label, typed parameters and tests. Keyboard, interface and CLI emit the same
commands; MIDI and MCP will too. The interface reads a state snapshot and acts through commands;
it holds no engine state.

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
