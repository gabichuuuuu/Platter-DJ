<p align="center">
  <img src="docs/assets/platter-mark.svg" width="96" height="96" alt="The Platter mark">
</p>

<h1 align="center">Platter</h1>

<p align="center">An open-source DJ application written in Rust.</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-milestone%20M1-272727?style=flat-square&labelColor=111111" alt="Status: milestone M1">
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
> **Nothing plays through a sound card yet.** The repository is at milestone M1: the engine
> exists and can be driven from the command line, without an interface and without live audio.

### What exists today


- The engine: two decks, a mixer (gain, three-band equaliser with full kill, one-knob filter,
  fader, crossfader with three curves) and a master section with a safety limiter.
- `platter-cli info <file>`: format, duration, channels and sample rate of an audio file.
- `platter-cli render --script <mix.toml> --out <mix.wav>`: runs a script of timed commands
  through the engine and writes the mix as a WAV file, without a sound card.
- `cargo xtask ci`, the gate that runs formatting, lints, tests, the layer rules, the licence
  and advisory checks, the clean-room check, the privacy check and the feature inventory check.

### What does not work yet

- No live audio and no desktop application; both are milestone M2.
- Decoding of WAV, AIFF and FLAC is tested with files synthesised in the tests. Decoding of
  MP3, M4A (AAC) and Ogg Vorbis is wired in but has no test with a valid file.
- No key lock: changing the tempo changes the pitch.
- Nobody has listened to the output. The tests measure levels, frequencies, distortion and
  the absence of clicks; they do not judge sound.

[`ROADMAP.md`](ROADMAP.md) lists what each milestone delivers and how far the inventory is.

## Building

Requirements: Rust through `rustup` (the toolchain in `rust-toolchain.toml` is installed on first
use) and `cargo-deny` 0.20.2. Details are in [`docs/development.md`](docs/development.md).

```sh
cargo xtask ci
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

Licensed under either of the Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)) or
the MIT license ([`LICENSE-MIT`](LICENSE-MIT)), at your option. Third-party resources are listed
in [`ATTRIBUTION.md`](ATTRIBUTION.md).

Platter is not affiliated with the maker of any other DJ application.
