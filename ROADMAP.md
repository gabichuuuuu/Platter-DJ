# Roadmap

Milestones are delivered one at a time. A milestone closes when its gate passes; the gate always
includes `cargo xtask ci`. Versions stay at 0.x until M4 closes.

## Progress

The table is generated from `docs/parity.toml` by `cargo xtask parity` in each release pull
request, so it shows the last release. The progress page, https://gabichuuuuu.github.io/Platter-DJ/,
has the count of `main`. Do not edit the table by hand.

<!-- parity:start -->

| Milestone | Features | Done | In progress | Planned |
| --- | ---: | ---: | ---: | ---: |
| M1 | 9 | 9 | 0 | 0 |
| M2 | 30 | 30 | 0 | 0 |
| M3 | 76 | 73 | 3 | 0 |
| M4 | 31 | 31 | 0 | 0 |
| M5 | 11 | 5 | 3 | 3 |
| Later | 5 | 0 | 0 | 5 |
| Total | 162 | 148 | 6 | 8 |

First complete version (M0 to M3): 112 of 115 features done.

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

- Tempo, beatgrid and key analysis sits behind a project trait. The community crate first named
  for it failed its measurement, so Platter detects them with its own code
  (`docs/decisions/0020-own-analysis.md`), measured against recordings with known tempo and
  key.
- Key lock sits behind a project trait. Two candidate time-stretching libraries are measured and
  the choice is recorded in an ADR before the milestone closes.
- The library uses SQLite behind a project trait.
- Platter writes tempo, key and cues to file tags by default, with a setting to turn it off.
  Writes are atomic (temporary file and rename) and never touch a file that could not be read
  completely.

Pending from M2:

- The clock of the status header, once the local time zone can be read
  (`docs/behavior/interface.md`, Differences).

Checks waiting for equipment or material the project does not have yet. Each one stays here
until it is done and its result is written in the note or the decision it names:

- **A MIDI controller.** Open a real input, receive its messages and drive the engine through
  a mapping, on Windows first, then on macOS and Linux, whose MIDI backends are only compiled
  in CI. Also: two devices with the same name, a device unplugged while open, the delay from
  a message to its command, and whether a minimised window still carries out the messages
  (`docs/decisions/0024-midi-mappings.md`, `docs/decisions/0025-midi-ports.md`,
  `docs/decisions/0026-midi-in-the-application.md`).
- **An audio input and a MIDI clock.** A live deck and the auxiliary input fed by a real input
  device, on the same interface as the output and on another one, with the delay from input to
  output (`docs/decisions/0040-audio-input.md`); the MIDI clock sent to a device that follows
  it and received from one that sends it, with the jitter of each
  (`docs/decisions/0041-midi-clock.md`).
- **Listening with real tracks.** Key lock, including the attack of kicks and the start of
  each pass of a short loop (`docs/decisions/0021-key-lock-stretch.md`); sync and the master
  clock; loops, beatjump and hotcues; flux and reverse (`docs/decisions/0030-flux-and-reverse.md`);
  a transposed deck, its change of shift and the untransposed bridge after a jump
  (`docs/decisions/0031-transposition.md`); the level autogain aims at
  (`docs/decisions/0029-autogain-level.md`); the step and the automatic fade of the crossfader
  (`docs/behavior/mixer.md`); the maximiser limiting hard, the bass above all, and its key
  switched while the music plays (`docs/decisions/0034-maximiser.md`); a set recorded by the mix
  recorder, played back, and a recording through a change of device
  (`docs/behavior/recording.md`); the cells of a remix deck with real samples, their start on the
  beat and the change from one to another (`docs/behavior/remix-deck.md`). Until then the README
  says nobody has listened to them.
- **An audio device with four outputs.** The beat tick and the metronome in the headphones
  against a real track (`docs/behavior/beatgrid.md`, `docs/behavior/sync.md`), and external
  mixer mode feeding a hardware mixer (`docs/behavior/external-mixer.md`).
- **An audio device at 96 kHz.** Key lock in the callback at that rate.
- **Full tracks with known tempo and key.** The detection was measured on 2,248 annotated loops
  of a public dataset (`docs/decisions/0020-own-analysis.md`, *Measurement on real
  recordings*); the full-length tracks of a DJ collection, on a corpus kept outside the
  repository, are still to be measured.
- **A real music collection.** The time and the result of importing a large folder; links
  on Windows, which need a privilege to create and were checked by hand with a junction only
  (`docs/decisions/0023-folder-import.md`).
- **A look at the real window.** Full screen and the tooltips of the keys, which the tests
  draw without a window (`docs/behavior/interface.md`).
- **macOS and Linux machines.** Everything above runs on Windows; the other systems only build
  and pass the tests in CI.

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

Remix decks, a pattern sequencer with own sounds, HID controllers, MIDI clock, live input and
microphone, broadcasting to a server the user configures. Compatibility with Ableton Link
(`docs/decisions/0043-link-later.md`) and DVS with an own timecode
(`docs/decisions/0045-timecode-later.md`) left M5.

Where it stands:

- **Done.** Input routing with live decks, the microphone and the auxiliary input, the recorders
  and the FX Send fed by an input (`docs/behavior/live-input.md`); the MIDI clock sent and
  followed (`docs/behavior/midi-clock.md`). Measured with synthesised signals only; the checks on
  real equipment are listed under M3.
- **In progress.** The remix deck: four slots of sixteen cells, quantize, loop and one-shot, latch
  and gate, reverse, punch and capture from a deck, with its grid on the screen in four pages
  (`docs/behavior/remix-deck.md`, `docs/decisions/0042-remix-deck.md`, `RemixGrid` in
  `docs/design/DESIGN.md`). Still to do: key lock, filter, effect send and cue per slot; saving a
  set; capture from the loop recorder.
- **Waiting for a decision of the maintainer.** The pattern sequencer and its own sounds, and the
  look of the live deck, the auxiliary knob and the keys of the MIDI clock, which `docs/design/`
  does not draw yet.
- **HID controllers, in progress.** Mappings name the bits, bytes and words of the reports of a
  HID device, and Platter lists the HID devices and reads their reports on a thread of their own
  (`docs/behavior/hid-controllers.md`, `docs/decisions/0044-hid-devices.md`). Still to do:
  choosing a HID device in the controller manager, learn mode and output to the device.
- **Broadcasting, in progress.** The mix coded as Opus in Ogg at 48 kHz, resampled from the rate
  of the device (`docs/behavior/broadcasting.md`, `docs/decisions/0046-broadcast-encoding.md`).
  Still to do: the connection to the server, which the user starts, its settings and the
  Broadcast key.

### After M5

Hosting third-party CLAP plugins.

Compatibility with Ableton Link as an own implementation, once the protocol has a public
description that is not code under the GPL (`docs/decisions/0043-link-later.md`).

DVS: timecode records and CDs with a signal of Platter's own and its decoder, with the absolute and
relative modes (`docs/decisions/0045-timecode-later.md`).

Replacing external dependencies with project code, piece by piece, as the maintainer decides
(`docs/dependencies.md`). With it, audio output below the device period on Windows through
the exclusive mode of WASAPI (`docs/decisions/0010-audio-output.md`, section Deferred).
