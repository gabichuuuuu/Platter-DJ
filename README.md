<p align="center">
  <img src="docs/assets/platter-mark.svg" width="88" height="88" alt="The Platter mark">
</p>

<h1 align="center">Platter DJ</h1>

<p align="center">
  An open-source DJ application for professional use, written in Rust,<br>
  with its own code, graphics and sounds.
</p>

<p align="center">
  <a href="#why-platter">Why Platter</a> ·
  <a href="#status">Status</a> ·
  <a href="#download">Download</a> ·
  <a href="#building">Building</a> ·
  <a href="#what-does-not-work-yet">Limitations</a> ·
  <a href="#documentation">Documentation</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <a href="https://github.com/gabichuuuuu/Platter-DJ/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/gabichuuuuu/Platter-DJ/ci.yml?branch=main&style=flat-square&label=ci&labelColor=111111" alt="CI status on main"></a>
  <img src="https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-272727?style=flat-square&labelColor=111111" alt="Platforms: Linux, macOS and Windows">
  <img src="https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-272727?style=flat-square&labelColor=111111" alt="Licence: MIT or Apache-2.0">
</p>

<br>

Platter is an independent, open-source DJ application written in Rust. The goal is a complete
desktop program for DJs, with the functions expected of professional DJ software, written with
its own code, graphics and sounds.

## Why Platter

The professional DJ applications in wide use are closed programs sold by their vendors. Platter
differs in these ways; each one is enforced by the code or was measured, and none is a promise.

- **Open.** The whole program is under `MIT OR Apache-2.0`, with no paid tier. Controller
  mappings and keyboard shortcuts are TOML files the user edits, and every action is a command
  with a stable identifier, so the keyboard, the window, the command line and the MCP server
  drive the same functions.
- **Private.** No telemetry, no account and no automatic reports; the application opens no
  network connection without an explicit action of the user. Stem separation runs on the
  computer, with no service and no native library.
- **Built not to crash.** The workspace lints deny `unwrap`, `expect`, `panic!` and `unsafe` in
  shipped code, and files, MIDI messages and command parameters are treated as untrusted input.
  `cargo xtask ci` checks this on every change.
- **Steady audio.** The audio thread does not allocate, lock or wait, and a test fails if it
  allocates or frees memory (`crates/engine/tests/realtime_memory.rs`). Rust has no garbage
  collector, so nothing pauses the audio to reclaim memory. Decoding, analysis and separation
  run on worker threads.
- **Measured** (Windows x64, release build): 30 s of audio separated into four stems in 3.0 s
  on 20 threads; the application uses half the processors for it.

Latency, CPU per deck, memory with four decks and start-up time have not been measured yet,
against Platter or any other program, so no claim of speed against other software is made. Rust
compiles to native code like the C++ most DJ software is written in; a difference will come
from design and has to be shown by measurement.

## Status

> [!IMPORTANT]
> **Milestone M2 is complete; M3 has the beatgrid, the loops, the cues, sync, key lock, the
> library and the browser.** The desktop application opens, loads tracks and plays them
> through a sound card, its tests pass, and the maintainer has listened to M2. It is not yet a
> tool to play a set with: the detection of tempo and key is not reliable enough yet, and
> much of M3 has not been listened to (see below).

### What exists today

| Area | Today |
| --- | --- |
| Decks | Two or four decks with scrolling and overview waveforms coloured by frequency; play, CUE and CUP; a tempo fader with three ranges and tempo bend |
| Mixer | Gain, three-band equaliser with kill keys, filter, pan, faders, a crossfader with three curves, meters and a master with a safety limiter; pre-listening on a second output pair |
| Loops and cues | Loops of a preset size or between an in and an out point, beatjumps and the moves of a loop; eight hotcues per deck that store cues and loops, with a CUE page to edit them |
| Sync | SYNC, MASTER and a phase meter per deck, and a master clock with Snap and Quantize |
| Library | A collection with playlists, folders, favourites, a preparation list and the history of each session; tags of one or several tracks edited and written into the files; covers; tempo, grid, key and loudness analysed in the background; collection files in the XML format of rekordbox and PLS and XSPF playlists imported |
| Stems | A stem deck plays the four stems linked to a track (drums, bass, other, vocals), each with volume, mute, filter and effect send; the stems are attached from a folder of separated files or separated by Platter with the 4-stem Spleeter model the user downloads, and the four-deck layout shows a lane per stem |
| Remote control | `platter --mcp` serves the Model Context Protocol on the standard input and output, so an assistant that starts Platter can find and run any of its commands and read the state of the decks and the mixer; no port is opened (`docs/behavior/mcp.md`) |
| Application | A browser with the collection and a file explorer; preferences for audio, general settings, decks, mixer, library and MIDI controllers; keyboard shortcuts in a file the user edits; English and Spanish |
| Command line | `platter-cli info <file>` prints file information; `platter-cli render --script <mix.toml> --out <mix.wav>` renders a scripted mix to a WAV file without a sound card |
| Gate | `cargo xtask ci` runs formatting, lints, tests, the layer rules, the licence and advisory checks, the clean-room check and the feature inventory check |

[`ROADMAP.md`](ROADMAP.md) lists what each milestone delivers and how far the inventory is.

## Download

Each [release](https://github.com/gabichuuuuu/Platter-DJ/releases) has an archive per system,
built in CI, with `platter` (the application) and `platter-cli`:

| System | Archive |
| --- | --- |
| Windows x64 | `platter-vX.Y.Z-x86_64-pc-windows-msvc.zip` |
| macOS on Apple silicon | `platter-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| Linux x64 | `platter-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |

Only Windows has been used for real. macOS and Linux build and pass the tests in CI, but nobody
has run Platter on them yet. The binaries are not signed, so Windows and macOS warn before
opening them.

## Building

Requirements: Rust through `rustup` (the toolchain in `rust-toolchain.toml` is installed on first
use) and `cargo-deny` 0.20.2. Details are in [`docs/development.md`](docs/development.md).

```sh
cargo xtask ci                      # the gate, the same locally and in CI
cargo run --release -p platter      # the desktop application
cargo run -p platter-cli -- --help  # the command line
```

## What does not work yet

Everything known to be missing, unfinished or not yet listened to.

<details>
<summary>The full list</summary>

- The manual check of M2 rests on the window, measured renders and the listening of the
  maintainer; the hour of playing with its dropout count has not been recorded. The tests measure levels, frequencies,
  distortion and the absence of clicks; they do not judge sound.
- On Windows the audio runs in the shared mode of WASAPI at the period of the device, about
  10 ms; the buffer size setting cannot go below it. Going lower needs the exclusive mode of
  WASAPI, which is left for a later version (`docs/decisions/0010-audio-output.md`).
- The clock of the status header is not shown, and screen readers get no information.
- The separation into stems has been checked on invented mixtures, not listened to on real music.
  Its model, the 4-stem Spleeter model, is downloaded by the user: the licence of its weights is
  not confirmed by their publisher (`docs/legal-notes.md`).
- The two-deck layout shows no stem lanes: a stem deck plays there, and its stems are driven
  through commands, the keyboard and MIDI.
- The MCP server has been run with a script of messages, not with an MCP client yet.
- The file selector does not list the drives of a Windows machine by itself; a drive is named
  in the settings file.
- Decoding of WAV, AIFF and FLAC is tested with files synthesised in the tests. MP3, M4A (AAC)
  and Ogg Vorbis were checked by hand against a WAV of the same signal; their tests cover
  damaged input only, as the project has no encoder for them.
- The tracks of the collection are analysed in the background, and a loaded track gets the
  tempo and the first beat found. Measured on 2,248 annotated loops of a public dataset, a
  tempo is given for 43 % of them and is right for 73 % of those, and the key is exact for
  36 %: the detection is not reliable enough yet, and it has not been measured on full tracks
  (`docs/decisions/0020-own-analysis.md`). The key found cannot be edited. The grid and the
  stored cues of a track are kept in the collection and come back when it is loaded again.
- Nobody has listened to loops or jumps yet.
- A deck crossing a fade-out cue starts the other deck of its pair from its fade-in cue and
  fades the crossfader to it, when the *Fade cues* key of the Mixer page is on. There is no
  automatic playback of a playlist, and nobody has listened to a transition
  (`docs/behavior/auto-mix.md`). Nobody has listened to the hotcues yet.
- Nobody has listened to sync yet. A deck syncs once its grid has a tempo, found by the
  analysis or tapped, typed or stepped in by hand.
- The metronome of the master clock clicks on the headphones only, so a device with two
  outputs does not play it, and nobody has listened to it.
- Key lock softens the attack of kicks, the first milliseconds after a jump play with the
  pitch following the speed, a track below 32 kHz plays without it, and nobody has listened to
  it; only synthesised signals were measured (`docs/decisions/0021-key-lock-stretch.md`). The
  same holds for transposition by semitones, locking the current key and the resulting key:
  the bridge after a jump plays 32 ms at the untransposed pitch, and nobody has listened to a
  transposed deck (`docs/decisions/0031-transposition.md`).
- The beat tick plays in the headphones, so a device with two channels does not play it, and
  nobody has listened to it against a real track (`docs/behavior/beatgrid.md`).
- Autogain is on and sets the gain of a channel when a track is analysed. It is switched off
  on the Mixer page of the preferences; the mixer has no key to restore it yet, the value is
  measured again on each load, and nobody has listened to its level, -10 LUFS
  (`docs/decisions/0029-autogain-level.md`).
- The length of the automatic fade of the crossfader is set on the Mixer page of the
  preferences. Nobody has listened to a fade (`docs/behavior/mixer.md`).
- External mixer mode, each deck on its own pair of outputs, is chosen on the Mixer page of
  the preferences. Deck A takes the master outputs of the routing and deck B the headphone
  ones, and it has not run on a device with four outputs feeding a hardware mixer
  (`docs/behavior/external-mixer.md`).
- Nobody has listened to flux or reverse yet (`docs/behavior/flux.md`).
- The maximiser of the master is switched by the Limiter key, which only the four-deck mixer
  draws, and set by commands and the settings file: it has no controls in the preferences and
  no gain reduction meter. Its ceiling is on sample peaks, and nobody has listened to it
  (`docs/behavior/master.md`).
- The mix recorder, over the mixer, writes the master into WAV files of 32-bit float samples
  in the folder `Platter DJ Recordings` of the music folder, named by the time they start in
  UTC, and Cut starts a new file. *Recordings*, last in the tree of the browser, shows that
  folder. It has no gain or level meter of its own, no preferences
  page and no external input, and nobody has listened to a recorded set
  (`docs/behavior/recording.md`).
- The browser shows the collection and imports a folder into it, with the tags of each file
  header and the cover each track carries, with a colour tag and a rating the menu of a track
  sets, a dialog that edits title, artist, album, genre, comment and rating of one track or
  of several at once and writes the text fields into the files unless the preferences say
  otherwise, and menu entries that import or delete the cover of the selected tracks. It
  searches the collection as the user types and sorts it by a column. It lists the playlists of the
  library and shows their tracks; their menus create, rename, empty and delete them, make
  folders, remove repeated tracks, take a track out and export or import playlists as M3U files
  in the folder the explorer shows, and a track dragged onto a playlist is added to it. One playlist can be the preparation list, which the menu of a track appends
  to or fills as next, and whose tracks are marked in the list; favourites above the tree reach
  the collection, the preparation list, the history and five playlists. A track a deck has
  played for the minimum play time of the Library page of the preferences, 30 seconds by
  default, is marked as played and enters the history of the session, which the
  tree shows with the histories of earlier sessions, dated in UTC. A track can be deleted from
  the collection after a question, and the Library page of the preferences opens the
  maintenance of the collection, which checks for missing files, relocates them by name in a
  folder and removes them. The preview player plays a track in the headphones without loading
  it, and is silent in external mixer mode. Nobody has imported a real music collection
  (`docs/behavior/browser.md`).
- MIDI controllers are added on the Controllers page of the preferences, each with a mapping
  file and a learn mode (`docs/behavior/controllers.md`); the status header shows how many are
  open and lights on their messages. There are no lights on the controllers yet. No real
  controller has been tried: the path from a MIDI input to the engine is tested with invented
  messages only.

</details>

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
| [`docs/behavior/`](docs/behavior/) | What each function does, with its public source |
| [`docs/decisions/`](docs/decisions/) | Decision records |
| [`docs/design/`](docs/design/) | Design system and reference screens |

## Licence

Copyright 2026 Gabriel Alejandro Spoleto Couto and the Platter contributors.

Licensed under either of the Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)) or
the MIT license ([`LICENSE-MIT`](LICENSE-MIT)), at your option. Third-party resources are listed
in [`ATTRIBUTION.md`](ATTRIBUTION.md).

Platter is not affiliated with the maker of any other DJ application.
