## What changes and why

## How it was tested

<!-- What you ran, and on what. Describe the setup generically: no device names as the system
shows them, no machine model, no local paths. "Not tested on hardware" is a valid answer. -->

- `cargo xtask ci`: <!-- passes, or the step that fails -->
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
- [ ] `docs/parity.toml` is updated if the status of a function changes.
