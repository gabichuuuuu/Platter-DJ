# Platter

Platter is an independent, open-source DJ application written in Rust. The goal is a complete
desktop program for DJs, with the functions expected of professional DJ software, written with
its own code, graphics and sounds.

## Status

**Nothing plays yet.** The repository is at milestone M0: the foundations.

What exists today:

- The cargo workspace, the pinned toolchain and the lint policy.
- `cargo xtask ci`, the gate that runs formatting, lints, tests, the layer rules, the licence
  and advisory checks, the clean-room check, the privacy check and the feature inventory check.
- The project rules (`AGENTS.md`), the architecture the next milestones will follow
  (`docs/architecture.md`), the design system (`docs/design/`) and the feature inventory
  (`docs/parity.toml`).

What does not exist yet: the audio engine, the decoders, the command line tool and the desktop
application. `ROADMAP.md` lists what each milestone delivers and how far the inventory is.

## Building

Requirements: Rust through `rustup` (the toolchain in `rust-toolchain.toml` is installed on first
use) and `cargo-deny` 0.20.2. Details are in `docs/development.md`.

```
cargo xtask ci
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
| `docs/decisions/` | Decision records |
| `docs/design/` | Design system and reference screens |

## Licence

Licensed under either of the Apache License, Version 2.0 (`LICENSE-APACHE`) or the MIT license
(`LICENSE-MIT`), at your option. Third-party resources are listed in `ATTRIBUTION.md`.

Platter is not affiliated with the maker of any other DJ application.
