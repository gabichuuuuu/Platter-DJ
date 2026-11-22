# Gaps

What Platter does not plan to build, and where it departs on purpose from common DJ software.
Functions that are planned but not built yet are not gaps: they are in `docs/parity.toml`.

## Out of scope

- Previews and streaming from commercial music stores and streaming services.
- A bundled third-party maximiser. Platter writes its own (M4).
- Hardware integrations that depend on closed protocols. Controllers are supported through MIDI
  (M3) and through HID when the protocol is public or documented by the community (M5).
- Reduced editions sold as separate products.
- WMA files and DRM-protected files.
- A WebAssembly or browser build.

## Deliberate differences

- Key notation is musical ("A minor"), with compatible keys highlighted in the browser. No
  third-party branded key notation is used.
- Controller mappings are declarative TOML files, not the mapping format of the documented
  application, and there is no scripting language.
- The effect catalogue starts with eight original effects in M4. The rest of the catalogue has
  no milestone yet.
- Link compatibility has no milestone: it waits for a public description of the protocol that is not code under the GPL (`docs/decisions/0043-link-later.md`). It will be an own implementation; the official library is not linked.
- Stem separation uses a model the user downloads separately (M4); see `docs/legal-notes.md`.
- No telemetry, no automatic reports, and no network connection without an explicit user action.
