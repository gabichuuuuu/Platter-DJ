# 0036. The crates that draw a frame are optimised in debug builds

Status: accepted (M4).

## Context

The interface tests of `platter-ui-egui` draw a fresh window for every control they operate
(`tests/panels.rs`, `every_control_emits_its_command`) and for every state, language and page
they check. Each window lays out, shapes and rasterises its text again. In the `test` profile,
which inherits `dev`, every dependency is built without optimisation. With the effect row and
the four effect units the binary of `tests/panels.rs` took 1228 s on the macOS runner of CI,
up from 850 s, and the job passed its limit of 30 minutes.

## Options

- **Raise the limit of the job.** It keeps the time growing with every control and makes the
  gate slower for everyone.
- **Fewer windows.** Sharing a window between controls, or sampling the controls, weakens the
  test: a control would see what another one left behind, or would not be operated at all.
- **Optimise every dependency** (`[profile.dev.package."*"]`). It also optimises wgpu, the
  decoders and the rest, which lengthens a build that misses the cache without shortening the
  tests by much more.
- **Optimise the crates on the path of a frame**: egui, epaint, emath and ecolor; the text
  stack skrifa, read-fonts, font-types and harfrust; the rasteriser vello_cpu, vello_common,
  fearless_simd, kurbo and peniko; and the containers ahash, hashbrown and smallvec they use
  on every glyph.

## Choice

The last one, as overrides of `opt-level = 2` under `[profile.dev.package]` in the workspace
`Cargo.toml`. Workspace crates keep the settings of `dev`, so their code is still checked with
debug assertions and overflow checks, and a debug run of the application draws faster too.

## How it was measured

`cargo test -p platter-ui-egui --test panels -- every_control_emits_its_command` on Windows
x64, with `CARGO_BUILD_JOBS=4`: 276 s without the overrides, 42 s with them. Building the test
binary after the change took 75 s, most of it the optimised crates.
