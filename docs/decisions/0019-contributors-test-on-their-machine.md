# 0016. Contributors test on their own machine; CI keeps running for every pull request

Status: accepted (M3).

## Context

- `cargo xtask ci` runs on CI runners with synthesised signals. The runners have no sound card,
  no MIDI or HID controller and nobody listening, so audio output, latency, controllers and the
  interface in use are only tested on the author's machine.
- Until now the pull request template asked "How it was tested" without saying what to report,
  so a description could omit the hardware entirely.
- Pull requests are limited to collaborators (`0001-pull-requests-limited-to-collaborators.md`),
  so CI only runs code written by people with write access.
- Some projects that test on hardware do not run their full CI by themselves for pull requests:
  a maintainer starts it by hand, or it starts once the author has a merged contribution. That
  protects the runners from untrusted code and saves CI minutes.

## Options

1. Keep the template as it was.
2. Ask the author for a structured report of the tests run on their machine, and keep CI running
   by itself for every pull request.
3. Option 2, and also run the full CI for pull requests only when a maintainer starts it.

## Choice

Option 2. The template lists what to report (the gate, operating system, audio API, sample rate
and buffer size, controller, listening check), and `CONTRIBUTING.md` states which changes need a
run on hardware and that the report follows the privacy rules.

Option 3 protects against code from outside the project, and there is none while pull requests
are limited to collaborators. Starting CI by hand would only delay the collaborators' own pull
requests. It is decided again as part of opening to external pull requests
(`docs/maintainers/github-setup.md`).

## How it was measured

Not measured. The choice rests on what CI runners can and cannot test, and on who can open a pull
request today.
