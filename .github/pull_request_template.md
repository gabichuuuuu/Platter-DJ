## What changes and why

## How it was tested

## Checklist

- [ ] `cargo xtask ci` passes.
- [ ] No panics in shipped code: no `unwrap`, `expect`, `panic!` or `unsafe` outside tests.
- [ ] Clean-room: behaviour comes from public documentation, cited by URL; nothing copied from the
      reference product or from GPL or AGPL code.
- [ ] Every new resource has a row in `ATTRIBUTION.md`.
- [ ] `docs/parity.toml` is updated if the status of a function changes.
