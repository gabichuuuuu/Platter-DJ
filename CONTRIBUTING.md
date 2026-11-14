# Contributing to Platter

This guide describes how contributions work. Read `AGENTS.md` first: its rules apply to every
change, whoever or whatever writes it.

## Setting up

1. Install Rust with `rustup`. The toolchain pinned in `rust-toolchain.toml` is installed on
   first use.
2. Install the licence and advisory checker:
   `cargo install cargo-deny --version 0.20.2 --locked`.
3. On Linux, install the system packages listed in `docs/development.md`.
4. Run the gate: `cargo xtask ci`.

## The gate

`cargo xtask ci` is the only gate, and CI runs exactly the same command. It checks formatting,
lints, tests, the layer rules, licences and advisories, the clean-room rules, the privacy rules
and the feature inventory. A pull request is reviewed once the gate passes.

Do not weaken a test or a rule to make the gate pass. If a rule seems wrong, open an issue.

## Commits and pull requests

- [Conventional Commits](https://www.conventionalcommits.org/), with the crate as scope:
  `feat(dsp): add three-band EQ with kill`, `test(engine): cover seek fades`.
- One commit per logical unit. Pull requests are merged by squash, so the title of the pull
  request follows the same format.
- Every change in behaviour comes with tests. Every bug fix comes with a test that failed before
  the fix.
- A change that delivers a function of `docs/parity.toml` updates its status and names the test
  or command that demonstrates it.
- A new dependency needs the agreement of the maintainer before the pull request: open an issue
  first. Dependencies under GPL, AGPL or LGPL are not accepted.

## Clean-room rules

Platter matches the functions of a commercial product without using any of its material. A
contribution must respect the clean-room rules in `AGENTS.md`. In short:

- Behaviour comes from public documentation, public specifications and scientific papers. The
  reference manual is cited by page and section; other sources by URL.
- Nothing is copied from the reference product or its vendor, and their binaries and files are
  never inspected.
- No GPL or AGPL code is copied or used as an implementation guide. If you have studied the
  source of such a project for a function, do not implement that function here.
- Code under a permissive licence may be adapted with a row in `ATTRIBUTION.md`.
- Every resource is original or has a compatible licence and a row in `ATTRIBUTION.md`. Test
  audio is synthesised in the tests.

## Contributions written with AI tools

AI tools are allowed. The responsibility is not transferable:

- **One person answers for each pull request.** The author has read and understood every line,
  has run it, can explain it in review and replies to the review personally. "The tool wrote it"
  is not an answer.
- **Tests are mandatory**, as for any other contribution, and the author has seen them fail
  without the change and pass with it.
- **The clean-room rules apply to the output of a tool as they apply to a person.** Do not give
  a tool proprietary or GPL material as a reference for work on Platter, and do not submit
  output you cannot trace to public documentation.
- **Low-effort submissions are closed.** Pull requests from outside the project pass an automatic
  quality filter (`.github/workflows/pr-quality.yml`) before a person looks at them. Generated
  descriptions that do not match the diff, bulk changes without a stated purpose, invented APIs
  and filler code are reasons to close without review.
- Commits and pull requests carry no tool attribution, such as `Co-Authored-By` lines naming an
  assistant. The author of the commit is the person who answers for it.

## Licence of contributions

Platter is licensed under `MIT OR Apache-2.0`. There is no CLA and no DCO sign-off. By submitting
a pull request you agree to license your contribution under `MIT OR Apache-2.0`, without any
additional terms, and you state that you have the right to do so.

## Conduct

Participation is subject to `CODE_OF_CONDUCT.md`. Security problems are reported as described in
`SECURITY.md`, not in public issues.
