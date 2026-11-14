# Legal notes

These notes record the licensing position of the project and the points that are still open.
They are not legal advice.

## Licence of the project

Platter is licensed under `MIT OR Apache-2.0`. The copyright notice in `LICENSE-MIT`,
`LICENSE-APACHE` and `NOTICE` names Gabriel Alejandro Spoleto Couto, the maintainer and author of
the project, together with the Platter contributors. There is no CLA and no DCO: each contributor
keeps the copyright of their contribution and licenses it under the same terms
(`CONTRIBUTING.md`).

## Clean-room position

Platter is an independent implementation of the functions DJs expect from professional DJ
software, and uses no material of any other product.

- Sources of behaviour: public user documentation of DJ software (mainly the reference manual),
  public specifications and scientific papers. Each behaviour is written down in Platter's own
  words in `docs/behavior/`. The URL of every cited section lives in `reference/sources.md`,
  which Git ignores.
- No code, icon, text, skin, sound, kit, mapping or screenshot of any other product is copied.
  Binaries and files of other products are not disassembled, inspected or instrumented.
- The layout follows the general conventions of DJ software; graphics, icons, colours, type,
  control shapes and texts are original (`docs/design/`).
- The names of the reference product and of its vendor appear nowhere in the repository, so
  no trademark of theirs is used; `cargo xtask cleanroom` enforces it.

## Third-party code

- Dependencies carry permissive licences only; `deny.toml` holds the list and `cargo deny check`
  enforces it. MPL-2.0 is accepted for unmodified dependencies (the decoder, from M1).
- GPL, AGPL and LGPL code is neither linked nor copied nor used as an implementation guide.
- Code under a permissive licence may be adapted with a row in `ATTRIBUTION.md`.
- The pull request filter `peakoss/anti-slop` is under AGPL-3.0. It runs as a CI tool on GitHub;
  it is not copied into the repository and not linked into any binary.

## Bundled fonts

`docs/design/fonts/` contains four static weights of Geist, Copyright (c) 2023 Vercel, in
collaboration with basement.studio, under the SIL Open Font License, Version 1.1. The licence
text is `docs/design/fonts/OFL.txt`.

What the licence asks of Platter:

- The copyright notice and the licence travel with every copy of the fonts, in the repository
  and, from M2, in the distributed application that embeds them.
- The fonts are not sold by themselves.
- The fonts stay under the OFL; bundling them does not change the licence of Platter, and the
  licence of Platter does not apply to them.
- From M2 the application embeds the four files in its binary, unmodified. A binary that is
  distributed must come with the copyright notice and the text of the licence; the release
  workflow of M3 has to ship `OFL.txt` beside it.
- The files are used unmodified. A modified version would have to respect any reserved font
  name.

## Open points

| Point | Status |
| --- | --- |
| AAC decoding in distributed binaries | AAC decoding sits behind its own cargo feature, on by default. Distribution of binaries that include an AAC decoder is pending review by the maintainer; no binary is distributed before that review |
| ASIO on Windows | The `asio` feature of cpal stays off: the ASIO SDK is available under GPL-3 or under an agreement with Steinberg. The decision is revisited after measuring WASAPI latency in M2 |
| Stem separation model (M4) | The candidate is the 4-stem Spleeter model. Its authors' paper states the MIT licence (<https://doi.org/10.21105/joss.02154>), but there is no confirmation from the publisher for the weights. Unless that is confirmed, Platter neither hosts nor packages the model: the user downloads it from the original publication with an explicit action, and the application states the doubt |
| Annotated corpora for measurement | Annotations under share-alike licences (for example GiantSteps Key, CC BY-SA 4.0, <https://zenodo.org/records/1101082>) and any audio stay outside the repository. Only aggregate percentages and the citation are committed |
