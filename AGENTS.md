# Rules for working on Platter

Platter is an independent, open-source DJ application written in Rust, with the functions DJs
expect from professional DJ software. When a function needs a specification, it comes from
public documentation, mainly the user manual of an established DJ application (the reference
manual, of the reference product), and is written down in Platter's own words. Platter uses no
code or resources of any other product. The identity of the reference product and the URL of
every cited section live in `reference/sources.md`, which Git ignores; the maintainer gives it
to collaborators.

These rules bind every contributor, human or automated. `docs/architecture.md` describes the
design, `docs/development.md` the daily workflow and `ROADMAP.md` the milestones.

Everything that enters the repository is written in English: code, comments, documentation,
commit messages and names.

## Closed decisions

Do not reopen these on your own. If one proves unworkable, stop and explain why.

| Topic | Decision |
| --- | --- |
| Name | Platter DJ wherever the product is named for its users: the window, the interface texts, the README. Platter for short in prose. Crates are named `platter-*` and set `publish = false`. Binaries: `platter` and `platter-cli`. The folder of the settings keeps the name Platter (`docs/decisions/0017-display-name.md`) |
| Licence | `MIT OR Apache-2.0`. No GPL, AGPL or LGPL dependency. What exists only under those licences is written by the project |
| Dependencies | Everything external is temporary and replaceable. Only the dependencies approved by the maintainer enter, each confined to one crate or module and listed in `docs/dependencies.md`. Do not write a replacement for one unless the maintainer asks |
| Language | Rust, edition 2024, stable toolchain pinned in `rust-toolchain.toml` |
| Platforms | Linux, macOS and Windows build and pass the tests in CI. Development and real testing happen on Windows x64 |
| Audio | `cpal`; WASAPI on Windows. The `asio` feature of cpal stays off |
| Interface | `egui` with `eframe` and `wgpu`, as a thin, replaceable layer. Desktop only, no WebAssembly target |
| Layout | Decks, mixer and browser where DJs expect them in professional DJ software. Graphics, icons, colours, type, control shapes and texts are Platter's own, from `docs/design/` |
| Interface language | English, translatable from M2. Spanish is the second language |
| Audio formats | MP3, M4A (AAC), WAV, AIFF, FLAC and Ogg Vorbis. No WMA, no DRM-protected files |
| Theme | Dark only. Every colour comes from one central token table |
| Accessibility | Full keyboard operation from M2; texts follow scaling |
| Controller mappings | Declarative TOML files. No scripting language |
| Privacy | No telemetry and no automatic reports. The application opens no network connection without an explicit user action. Crash logs stay local |
| Measurement data | Only synthesised signals in the repository and in CI. Real corpora stay in a cache outside the repository; only aggregate results and the citation are committed |
| Commits and versions | Conventional Commits and SemVer; 0.x until M4 closes |
| Pull requests | Limited to collaborators for now (`docs/decisions/0001-pull-requests-limited-to-collaborators.md`) |
| Remote | A private repository in the maintainer's GitHub account. Push, open a pull request or merge only when the maintainer asks. Never change its visibility |

## Non-negotiable rules

### Clean-room

- Behaviour comes from public documentation: the reference manual, cited by page and section;
  public specifications (MIDI, USB HID, audio formats) and scientific papers, cited by URL in
  `docs/` or in the module comment.
- Never copy code, icons, texts, skins, sounds, kits, mappings or screenshots from the vendor of
  the reference product. Never disassemble, inspect or instrument its binaries and files.
- Never copy GPL or AGPL code, and never use it as an implementation guide: Mixxx, xwax,
  libKeyFinder, Rubber Band, Ableton Link and their derivatives.
- Code under a permissive licence may be adapted with a row in `ATTRIBUTION.md`.
- The names of the reference product and of its vendor appear nowhere in the repository: not in
  code, documentation, commit messages, names, identifiers or the interface. Write "the
  reference product" and "the reference manual". `cargo xtask cleanroom` checks the tracked
  files.
- Every resource (icon, font, test audio) is original or has a compatible licence, and has a row
  in `ATTRIBUTION.md`. Test audio is synthesised in the tests; no music is committed.

### It never crashes

- Shipped code does not panic. Fallible operations return `Result` or `Option`. Files, mappings,
  MIDI messages and command parameters are untrusted input.
- Forbidden outside tests: `unwrap()`, `expect()`, `panic!`, `unreachable!`, `todo!`,
  `unimplemented!`, `dbg!`. The workspace lints enforce it.
- No `unsafe`. The single exception planned up to M2 is the test allocator of `platter-testkit`.
  Any later exception (FFI) lives alone in its crate and is written here before it exists.
- Sizes and counts read from files are bounded before allocating or looping. Floats are checked
  with `is_finite()` before use as an index, a length or a parameter.
- Every bug fix comes with a test that failed before the fix.

### Real time

Inside the audio callback and everything it calls:

- No allocation and no deallocation. Buffers are allocated beforehand; whatever must be freed
  goes back to a normal thread through a queue.
- No `Mutex`, `RwLock`, blocking channel, I/O, logging or system call.
- Bounded time: no loop whose iteration count depends on external data without a limit.
- Communication with the rest of the program uses lock-free single-producer single-consumer
  queues (`rtrb`) and atomics.
- Parameter changes are smoothed so they do not click. Play, pause and seek apply a fade of a
  few milliseconds.
- A test under the test allocator fails if `process` allocates or frees memory.

### Everything is a command

- Every action visible to the user is a command with a stable identifier (`deck.play_pause`,
  `mixer.crossfader.set`), a label, typed parameters and tests.
- Keyboard, interface and CLI emit the same commands; MIDI and MCP will too. Do not add a
  shortcut that changes state by another path.
- The interface reads a snapshot of the state and acts through commands. It holds no engine
  state.

### Layers

`cargo xtask layers` fails on a violation of the rules in `docs/architecture.md`. Slow work
(decoding, analysis, waveform peaks) runs on worker threads, never on the audio thread or the
interface thread.

### Anti-slop

- Verify before stating. Before using an API, open the documentation of the installed version
  (docs.rs, `cargo doc`, the source in the cargo registry). Do not write signatures, versions,
  cargo feature names or action SHAs from memory.
- No filler: no empty crates, no empty functions, no abstraction for a future use that has no
  second user today, no dead or commented-out code.
- Comments explain why. They do not narrate the next line.
- No emojis and no promotional language in code, documentation or commits. The README states
  the real status, including what does not work.
- Before adding a dependency, check whether the project can write it. A dependency the
  maintainer has not approved means stopping to ask. Each one is justified in its commit: what
  for, licence, and why the project does not write it.
- Report only what you ran. If you could not run something, say so. Nobody can verify sound by
  reading code: never claim that something "sounds good"; say what was measured.
- Do not weaken a test or a rule to make it pass. If a gate does not pass, stop and report.

### Familiar workflows, own design

- Platter follows the conventions DJs know from professional DJ software, so that decks, mixer,
  browser and workflows are where a DJ looks for them. Graphics, icons, colours, type, control
  shapes and texts are its own, and a screenshot of Platter looks like Platter.
- Before implementing a function of `docs/parity.toml`, write its note in
  `docs/behavior/<area>.md`: what it does according to the reference manual (with the page and
  section), where Platter departs from it and why, and the edge cases. The tests of the function
  refer to that note.
- The images of the manual serve to understand the layout and the purpose of each control, not
  to reproduce its look.
- Never read, open or analyse binaries, files, skins or resources of the reference product, and
  never rebuild its interface from screenshots, with tools or by hand.
- `docs/design/` is the design system, written by the maintainer. Do not modify, regenerate or
  reformat it. A need its tokens do not cover is raised as a question.

### No trace of the development machine

Nothing that enters the repository (files, Git history, commit messages) or the distributed
binaries may identify the particular machine used for development. The test: a fact that holds
for any other computer is allowed; a fact that points at this one is not.

- Allowed, being generic: operating system and architecture ("Windows x64"), that it is a
  laptop, the audio API (WASAPI), sample rates and buffer sizes, versions of Rust and the tools.
- Forbidden in tracked files, commits, ADRs, documentation, tests, fixtures and screenshots:
  absolute local paths (`C:\Users\<name>`, `/home/<name>`, `/Users/<name>`), the system user
  name, the machine or domain name, the make and model of the machine, CPU or GPU models, the
  names of audio, MIDI or display devices as the system shows them, serial numbers, IP or MAC
  addresses, and proxy, certificate or network configuration.
- Paths in code and documentation are relative to the repository root. Tests use temporary
  directories and no expected result contains an absolute path.
- Measurements (latency, performance, quality) are documented generically: "Windows x64,
  WASAPI, 256-sample buffer", without machine model or device name.
- The Git identity is a real name with a deliberate address, never one generated by the system.
- Local cargo or Git configuration (proxy, mirrors, paths, credentials) lives outside the
  repository. The tracked `.cargo/config.toml` holds only the `xtask` alias.
- Distributed binaries are built in CI only, with the paths of the build machine remapped.
- Images and resources carry no metadata with paths, user, machine name or model. Screenshots
  use synthetic data and show no real device names or paths.

`cargo xtask privacy` checks the tracked files and the whole history. Run it before each commit.

## Working method

- One gate, the same locally and in CI: `cargo xtask ci`. It must pass before every commit that
  closes a unit of work.
- One commit per logical unit, in Conventional Commits format with the crate as scope:
  `feat(dsp): add three-band EQ with kill`. The milestone goes in the body (`Milestone: M1`).
- No `Co-Authored-By` lines and no mention of agents or tools in commits or pull requests.
- Before each commit: `git status --short`, `git diff --stat`, `git diff --check`. Add explicit
  paths; never `git add -A`.
- Changes reach `main` through pull requests, never by a direct push. A milestone is split into
  one pull request per crate or logical unit, opened in dependency order, lower layers first.
  Each has a Conventional Commit title, passes the gate and is squash-merged. A branch is pushed
  and a pull request opened only when the maintainer asks.
- Pull request body: copy `.github/pull_request_template.md` and keep every line. Fill "What
  changes and why", "How it was tested" (what was run and on what, described generically as
  `CONTRIBUTING.md` asks; "Not tested on hardware" is a valid answer) and the `Depends on`
  (#N or none) and `Fixes` (Fixes #N or none) lines. Tick a box only when it holds; a box that
  does not hold stays empty and the description says why.
- No tags and no rewriting of history.
- A technical decision the rules leave open is recorded as a short ADR in `docs/decisions/`:
  context, options, choice, how it was measured.
- A function changes status in `docs/parity.toml` in the commit that delivers it; then run
  `cargo xtask parity`.
- Stop and ask when a dependency outside the approved list is needed, when something requires
  installing software or touching anything outside the repository, when a closed decision
  proves unworkable, or when a gate does not pass after reasonable attempts.
