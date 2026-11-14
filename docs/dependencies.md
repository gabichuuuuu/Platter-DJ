# Dependencies

Every external dependency is temporary: it can stay or be replaced by project code, and the
maintainer decides that piece by piece. Until then each one is confined to a single crate or
module, so replacing it touches one place.

## In use

Every direct dependency of a workspace crate has a row here. `cargo xtask layers` fails when a
dependency has no row or a row has no dependency.

| Crate | Use | Licence | Confined to | Possible replacement |
| --- | --- | --- | --- | --- |
| `cargo_metadata` | Reads the workspace graph for the layer rules and the dependency registry | MIT | `xtask` (not distributed) | Parse the JSON of `cargo metadata` with project code |
| `cpal` | Lists the output devices and runs the output stream: WASAPI, CoreAudio, ALSA | Apache-2.0 | `platter-audio-io`, module `device`; none of its types is public (`docs/decisions/0010-audio-output.md`) | Own bindings to each system API, which would need `unsafe` |
| `eframe` | Opens the window, runs the event loop and puts the frames of egui on the screen through `wgpu` | MIT OR Apache-2.0 | `platter` (the application); its `accesskit` feature stays off (`docs/behavior/interface.md`) | An own window and GPU layer over the system APIs, which would need `unsafe` |
| `egui` | The interface: layout, input, text and painting | MIT OR Apache-2.0 | `platter-ui-egui`; the widgets and panels use its painter and input, nothing of its stock widgets (`docs/decisions/0013-interface.md`) | Another immediate-mode layer, or an own renderer |
| `proptest` | Property tests: damaged files in the decoder, command sequences in the engine | MIT OR Apache-2.0 | Tests of `platter-decode` and `platter-engine` (not distributed) | Hand-written generators with a fixed seed |
| `rtrb` | Lock-free single-producer single-consumer queues between the control side and the processor | MIT OR Apache-2.0 | `platter-engine`: `rt` and `control` modules | An own ring buffer, which would need `unsafe` |
| `serde` | Deserialises the feature inventory and the render script | MIT OR Apache-2.0 | `xtask` (not distributed); `platter-cli`, module `script` | A hand-written reader for the few structures involved |
| `symphonia` | Decodes MP3, AAC in M4A, WAV, AIFF, FLAC and Ogg Vorbis; reads title, artist and album | MPL-2.0 | `platter-decode`, behind its own types and a panic boundary (`docs/decisions/0008-decoder-panic-boundary.md`) | Own decoders from the public specifications |
| `toml` | Parses `docs/parity.toml`, the render script, the design tokens, the icons, the translations, the shortcut file and the settings file | MIT OR Apache-2.0 | `xtask` (not distributed); `platter-cli`, module `script`; `platter-ui-egui`, modules `tokens`, `icons`, `i18n` and `keymap`; `platter`, module `config` | A reader for the TOML subset the project files use |

Tools that are not dependencies: `cargo-deny` 0.20.2 (licences and advisories), installed
separately.

## Approved, not yet in use

Only these may be added without asking the maintainer, in the milestone given. Versions and
licences were read from crates.io on 12 November 2026; add each with `cargo add`, check the
licence again and read the documentation of the version obtained.

| Use | Crate | Version seen | Licence | Milestone | Why it is external |
| --- | --- | --- | --- | --- | --- |
| Audio I/O | cpal | 0.18.2 | Apache-2.0 | M2 | Access to WASAPI, CoreAudio and ALSA |
| Decoding | symphonia | 0.6.1 | MPL-2.0 | M1 | MP3, AAC, FLAC and Vorbis codecs |
| Lock-free queue | rtrb | 0.4.1 | MIT OR Apache-2.0 | M1 | An own queue would need `unsafe` |
| Interface | egui, eframe | 0.37.0 | MIT OR Apache-2.0 | M2 | GPU interface engine |
| Serialisation | serde, toml | 1.0.229, 1.2.1 | MIT OR Apache-2.0 | M1 (in `xtask` since M0) | Base of the Rust ecosystem |
| Property tests | proptest | 1.12.0 | MIT OR Apache-2.0 | M1 | Tests only; not distributed |
| Cargo metadata | cargo_metadata | 0.23.1 | MIT | M0 | `xtask` only; not distributed |
| MIDI | midir | 0.11.1 | MIT | M3 | Access to the MIDI ports of the system |
| Tags | lofty | 0.25.5 | MIT OR Apache-2.0 | M3 | Temporary, behind a project trait |
| Library | rusqlite | 0.42.0 | MIT | M3 | Temporary, behind a project trait |
| Stems | ort | 2.0.0-rc.15 | MIT OR Apache-2.0 | M4 | ONNX runtime |
| HID | hidapi | 2.6.7 | MIT | M5 | Access to the HID devices of the system |

Notes:

- symphonia is in the 0.6 series; do not assume the 0.5 API. Enable only the formats Platter
  supports. A DRM-protected or unsupported file returns a clear error.
- On Linux, cpal needs the ALSA development package even when PipeWire is in use.
- No async runtime.

### Community pieces for advanced functions

Allowed and temporary like everything external, each behind a project trait. Before adding one,
check its current licence on crates.io and in its repository; if it differs from this table,
stop and ask. Each gets a row in `ATTRIBUTION.md`.

| Use | Piece | Licence seen | Milestone | Conditions |
| --- | --- | --- | --- | --- |
| Tempo, beatgrid and key | stratum-dsp | MIT OR Apache-2.0 | M3 | Pure Rust. Measured against the corpus before adoption |
| Key lock | timestretch | MIT | M3 | Pure Rust. Measured against the next one |
| Key lock | Signalsmith Stretch, through signalsmith-stretch | MIT | M3 | C++ with FFI: needs a C++ compiler and an `unsafe` exception isolated in its crate and written in `AGENTS.md` before use |
| Stems | 4-stem Spleeter model exported to ONNX | See `docs/legal-notes.md` | M4 | Decided in M4; until then only the interface, without a model |
| Effects | Airwindows algorithms, adapted to Rust | MIT | M4 | Adaptation with attribution; Platter's own names |
| Sound kits | CC0 samples | CC0-1.0 | M5 | Each sample with its origin in `ATTRIBUTION.md` |

Never, even when popular: GPL, AGPL or LGPL code (Mixxx, xwax, libKeyFinder, Rubber Band,
SoundTouch, aubio, Essentia, the Ableton Link library) and models or data with a non-commercial
clause.

## Written by the project

No external crate for these:

| Need | Where | Milestone |
| --- | --- | --- |
| Error types, with `Display` and `Error` written by hand | each crate | M1 |
| Argument parser of `render` and `info` | `apps/platter-cli` | M1 |
| Logging to a local file, off the audio thread | `platter-core` | M1 |
| WAV writer for the render | `platter-engine` | M1 |
| Interpolation and resampling | `platter-dsp` | M1 |
| In-app file selector | `platter-ui-egui` | M2 |
| Jog wheels, faders, crossfader, meters and waveforms | `platter-ui-egui` | M2 |
| Drawing the icons of `docs/design/icons/icons.toml` with the egui painter, without an SVG dependency | `platter-ui-egui` or `xtask` | M2 |
| PNG writer for the visual comparison captures | `apps/platter` | M2 |
| Tempo, beatgrid and key analysis, if the temporary piece is replaced | `platter-analysis` | M3 or later |
| Key lock, if the temporary piece is replaced | `platter-dsp` | M3 or later |
| Decoders from the public specifications, if symphonia is replaced | `platter-decode` | After M3 |
| Own DVS timecode and its decoder | `platter-dsp` | M5 |
