## What changes and why

## How it was tested

<!-- What you ran, and on what. "Not tested on hardware" is a valid answer. -->

- `cargo xtask ci`: <!-- passes, or the step that fails -->
- Tests added or changed: <!-- names, or none and why -->
- Operating system: <!-- for example Windows x64 -->
- Audio: <!-- API, sample rate and buffer size; or n/a -->
- Controller: <!-- kind and protocol (MIDI or HID); or n/a -->
- Listening check: <!-- what was played and what was checked (clicks, dropouts, level); or not done -->

## Checklist

- [ ] `cargo xtask ci` passes.
- [ ] No panics in shipped code: no `unwrap`, `expect`, `panic!` or `unsafe` outside tests.
- [ ] Clean-room: behaviour comes from public documentation, cited by URL; nothing copied from the
      reference product or from GPL or AGPL code.
- [ ] Every new resource has a row in `ATTRIBUTION.md`.
- [ ] Every change in behaviour comes with tests, seen failing without the change and passing
      with it.
- [ ] A bug fix comes with a test that failed before the fix.
- [ ] A new user action is a command with a stable identifier, a label and tests.
- [ ] No new dependency, or the maintainer agreed to it in an issue first.
- [ ] Nothing in the audio callback allocates, locks, logs or does I/O.
- [ ] A function of `docs/parity.toml` has its note in `docs/behavior/` and its status updated.
- [ ] Depends on: <!-- #N of the pull request that goes first, or none -->
- [ ] Fixes: <!-- Fixes #N for the issue it closes, or none -->
