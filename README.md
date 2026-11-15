<p align="center">
  <img src="docs/assets/platter-mark.svg" width="96" height="96" alt="The Platter mark">
</p>

<h1 align="center">Platter DJ</h1>

<p align="center">An open-source DJ application written in Rust.</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-M2%20done-272727?style=flat-square&labelColor=111111" alt="Status: M2 done">
  <img src="https://img.shields.io/badge/rust-edition%202024-272727?style=flat-square&labelColor=111111" alt="Rust, edition 2024">
  <img src="https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-272727?style=flat-square&labelColor=111111" alt="Platforms: Linux, macOS and Windows">
  <img src="https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-272727?style=flat-square&labelColor=111111" alt="Licence: MIT or Apache-2.0">
</p>

<p align="center">
  <a href="#status">Status</a> ·
  <a href="#building">Building</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

---

Platter is an independent, open-source DJ application written in Rust. The goal is a complete
desktop program for DJs, with the functions expected of professional DJ software, written with
its own code, graphics and sounds.

## Status

> [!IMPORTANT]
> **Milestone M2 is complete; M3 has started with the beatgrid, the loops and the cues.** The
> desktop application opens, loads tracks and plays them through a sound card, its tests
> pass, and the maintainer has listened to it. It is not yet a tool to play a set with: sync
> has no controls in the window yet and there is no library.

### What exists today

- The desktop application, `platter`: two decks with scrolling and overview waveforms coloured
  by frequency, play, CUE, CUP, a tempo fader with three ranges and tempo bend; a mixer with
  gain, three-band equaliser with kill keys, filter, pan, faders, a crossfader with three
  curves, meters and a master with a safety limiter; pre-listening on a second output pair;
  loops of a preset size or between an in and an out point, beatjumps and the moves of a
  loop; eight hotcues per deck that store cues and loops, with a CUE page to edit them; a
  file selector; audio and general preferences; keyboard shortcuts in a file the user
  edits; English and Spanish.
- `platter-cli info <file>` and `platter-cli render --script <mix.toml> --out <mix.wav>`: file
  information, and a scripted mix rendered to a WAV file without a sound card.
- `cargo xtask ci`, the gate that runs formatting, lints, tests, the layer rules, the licence
  and advisory checks, the clean-room check and the feature inventory check.

### What does not work yet

- The manual check of M2 rests on the window, measured renders and the listening of the
  maintainer; the hour of playing with its dropout count has not been recorded. The tests measure levels, frequencies,
  distortion and the absence of clicks; they do not judge sound.
- On Windows the audio runs in the shared mode of WASAPI at the period of the device, about
  10 ms; the buffer size setting cannot go below it. Going lower needs the exclusive mode of
  WASAPI, which is left for a later version (`docs/decisions/0010-audio-output.md`).
- The clock of the status header is not shown, and screen readers get no information.
- The file selector does not list the drives of a Windows machine by itself; a drive is named
  in the settings file.
- Decoding of WAV, AIFF and FLAC is tested with files synthesised in the tests. MP3, M4A (AAC)
  and Ogg Vorbis were checked by hand against a WAV of the same signal; their tests cover
  damaged input only, as the project has no encoder for them.
- The beatgrid is set by hand on the GRID page of each deck: nothing detects the tempo yet,
  the tempo field cannot be typed into, and the grid is lost when the track is loaded again.
- Loops, cues and the moves of a loop start at the play position as it is unless Snap and
  Quantize are on, and those have no controls in the window yet. Nobody has listened to
  loops or jumps yet.
- Stored cues are lost when the track is loaded again, the fade and load types are labels
  only, and the window cannot rename a cue yet. Nobody has listened to the hotcues yet.
- Sync, the tempo master and the master clock exist in the engine only: the window has no
  controls for them, its tempo readout does not show the speed of a synced deck, and nobody
  has listened to them. A deck needs a tempo tapped or stepped into its grid before it can
  sync.
- No key lock, no collection, no MIDI: those are milestone M3.
  Changing the tempo changes the pitch.

[`ROADMAP.md`](ROADMAP.md) lists what each milestone delivers and how far the inventory is.

## Building

No version has been released yet. Releases, with binaries for Windows, macOS and Linux built in
CI, will appear in the GitHub releases of this repository; until then Platter is built from
source.

Requirements: Rust through `rustup` (the toolchain in `rust-toolchain.toml` is installed on first
use) and `cargo-deny` 0.20.2. Details are in [`docs/development.md`](docs/development.md).

```sh
cargo xtask ci
cargo run --release -p platter
cargo run -p platter-cli -- --help
```

## Documentation

| File | Content |
| --- | --- |
| [`ROADMAP.md`](ROADMAP.md) | Milestones and progress |
| [`AGENTS.md`](AGENTS.md) | Rules every change follows |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | How to help and how contributions work |
| [`docs/architecture.md`](docs/architecture.md) | Crates, layers, threads and data flow |
| [`docs/development.md`](docs/development.md) | Setup, the gate and the daily workflow |
| [`docs/parity.toml`](docs/parity.toml) | Feature inventory with milestone and status |
| [`docs/gaps.md`](docs/gaps.md) | What is out of scope |
| [`docs/dependencies.md`](docs/dependencies.md) | External dependencies and why each one is there |
| [`docs/legal-notes.md`](docs/legal-notes.md) | Licensing, clean-room sources and open legal points |
| [`docs/behavior/`](docs/behavior/) | What each function does, with its source in the public manual |
| [`docs/decisions/`](docs/decisions/) | Decision records |
| [`docs/design/`](docs/design/) | Design system and reference screens |

## Licence

Copyright 2026 Gabriel Alejandro Spoleto Couto and the Platter contributors.

Licensed under either of the Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)) or
the MIT license ([`LICENSE-MIT`](LICENSE-MIT)), at your option. Third-party resources are listed
in [`ATTRIBUTION.md`](ATTRIBUTION.md).

Platter is not affiliated with the maker of any other DJ application.
