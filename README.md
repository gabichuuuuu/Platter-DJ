# Platter

Platter is an independent, open-source DJ application written in Rust. The goal is a complete
desktop program for DJs, with the functions expected of professional DJ software, written with
its own code, graphics and sounds.

## Status

**Nothing plays through a sound card yet.** The repository is at milestone M1: the engine
exists and can be driven from the command line, without an interface and without live audio.

What exists today:

- The engine: two decks, a mixer (gain, three-band equaliser with full kill, one-knob filter,
  fader, crossfader with three curves) and a master section with a safety limiter.
- `platter-cli info <file>`: format, duration, channels and sample rate of an audio file.
- `platter-cli render --script <mix.toml> --out <mix.wav>`: runs a script of timed commands
  through the engine and writes the mix as a WAV file, without a sound card.
- `cargo xtask ci`, the gate that runs formatting, lints, tests, the layer rules, the licence
  and advisory checks, the clean-room check, the privacy check and the feature inventory check.

What does not work yet:

- No live audio and no desktop application; both are milestone M2.
- Decoding of WAV, AIFF and FLAC is tested with files synthesised in the tests. Decoding of
  MP3, M4A (AAC) and Ogg Vorbis is wired in but has no test with a valid file.
- No key lock: changing the tempo changes the pitch.
- Nobody has listened to the output. The tests measure levels, frequencies, distortion and
  the absence of clicks; they do not judge sound.

`ROADMAP.md` lists what each milestone delivers and how far the inventory is.

## Building

Requirements: Rust through `rustup` (the toolchain in `rust-toolchain.toml` is installed on first
use) and `cargo-deny` 0.20.2. Details are in `docs/development.md`.

```
cargo xtask ci
cargo run -p platter-cli -- --help
```

## Documentation

| File | Content |
| --- | --- |
| `ROADMAP.md` | Milestones and progress |
| `AGENTS.md` | Rules every change follows |
| `CONTRIBUTING.md` | How to help and how contributions work |
| `docs/architecture.md` | Crates, layers, threads and data flow |
| `docs/development.md` | Setup, the gate and the daily workflow |
| `docs/parity.toml` | Feature inventory with milestone and status |
| `docs/gaps.md` | What is out of scope |
| `docs/dependencies.md` | External dependencies and why each one is there |
| `docs/legal-notes.md` | Licensing, clean-room sources and open legal points |
| `docs/behavior/` | What each function does, with its source in the public manual |
| `docs/decisions/` | Decision records |
| `docs/design/` | Design system and reference screens |

## Licence

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT license
(`LICENSE-MIT`), at your option. Third-party resources are listed in `ATTRIBUTION.md`.

Platter is not affiliated with the maker of any other DJ application.
