# Roadmap

Milestones are delivered one at a time. A milestone closes when its gate passes; the gate always
includes `cargo xtask ci`. Versions stay at 0.x until M4 closes.

## Progress

The table is generated from `docs/parity.toml` by `cargo xtask parity`. Do not edit it by hand.

<!-- parity:start -->

| Milestone | Features | Done | In progress | Planned |
| --- | ---: | ---: | ---: | ---: |
| M1 | 9 | 9 | 0 | 0 |
| M2 | 30 | 30 | 0 | 0 |
| M3 | 76 | 9 | 4 | 63 |
| M4 | 31 | 0 | 0 | 31 |
| M5 | 15 | 0 | 0 | 15 |
| Later | 1 | 0 | 0 | 1 |
| Total | 162 | 48 | 4 | 110 |

First complete version (M0 to M3): 48 of 115 features done.

<!-- parity:end -->

## Milestones

### M0. Foundations (done)

Workspace, toolchain, lints, the `xtask` gate with its checks, project rules, licences,
documentation, the feature inventory and the GitHub configuration files.

### M1. Engine without interface (done)

`platter-core`, `platter-dsp`, `platter-decode`, `platter-engine`, `platter-testkit` and
`platter-cli`. The command line tool reports file information and renders a scripted two-deck mix
to a WAV file with the same processor the live audio will use. Decoding of MP3, M4A (AAC), WAV,
AIFF, FLAC and Ogg Vorbis. A static progress page generated from the inventory.

### M2. Live sound (done)

`platter-audio-io`, `platter-ui-egui` and the `platter` desktop application: two decks with
frequency-coloured waveforms, the mixer (gain, three-band EQ with kill, filter, pan, faders,
crossfader, master with safety limiter), audio settings, an in-app file selector, editable
keyboard shortcuts and English and Spanish texts.

### M3. First complete version

Sync and master clock, beatgrid detection and editing, hotcues, loops, beatjump, flux and
reverse, key detection with key lock, the browser with collection, playlists, history and
preview deck, background analysis, tag writing, MIDI controllers with declarative TOML mappings
and learn mode, external mixer mode, autogain, two deck sizes and the release workflow.

Decisions already taken for this milestone:

- Tempo, beatgrid and key analysis sits behind a project trait. It starts with a community crate
  and is measured against tracks with known tempo and key.
- Key lock sits behind a project trait. Two candidate time-stretching libraries are measured and
  the choice is recorded in an ADR before the milestone closes.
- The library uses SQLite behind a project trait.
- Platter writes tempo, key and cues to file tags by default, with a setting to turn it off.
  Writes are atomic (temporary file and rename) and never touch a file that could not be read
  completely.

Pending from M2:

- The clock of the status header, once the local time zone can be read
  (`docs/behavior/interface.md`, Differences).

### M4. Full feature level

Four decks, layouts per task and custom layouts, effect units and the per-channel mixer effect,
stems, flexible beatgrids, mix and loop recorders, automatic mixing, smart playlists, an own
maximiser, the remaining EQ and filter models, the remaining waveform colour modes, collection
import and an MCP server.

Decisions already taken for this milestone:

- Effects are the project's own. The effect chain is designed so it can host CLAP plugins later.
- Stem separation runs a model through ONNX. The model is a separate download and never part of
  the repository. No model trained on data restricted to non-commercial use.

### M5. Extras

DVS with an own timecode and decoder, remix decks, a pattern sequencer with own sounds, HID
controllers, MIDI clock, compatibility with Ableton Link as an own implementation, live input and
microphone, broadcasting to a server the user configures.

### After M5

Hosting third-party CLAP plugins.

Replacing external dependencies with project code, piece by piece, as the maintainer decides
(`docs/dependencies.md`). With it, audio output below the device period on Windows through
the exclusive mode of WASAPI (`docs/decisions/0010-audio-output.md`, section Deferred).
