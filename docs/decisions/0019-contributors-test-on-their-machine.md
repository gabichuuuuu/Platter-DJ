# 0019. Contributors test on their own machine; CI on outside code waits for the maintainer

Status: accepted (M3).

## Context

- `cargo xtask ci` runs on CI runners with synthesised signals. The runners have no sound card,
  no MIDI or HID controller and nobody listening, so audio output, latency, controllers and the
  interface in use are only tested on the author's machine.
- Until now the pull request template asked "How it was tested" without saying what to report,
  so a description could omit the hardware entirely.
- Anyone may open a pull request (`0018-pull-requests-open-to-everyone.md`), so CI can be asked
  to run code written outside the project.

## Options

1. Keep the template as it was.
2. Ask the author for a structured report of the tests run on their machine, and let CI run by
   itself for every pull request.
3. Option 2, and run the workflows of a pull request from outside the project only once the
   maintainer approves them.

## Choice

Option 3. The template lists what to report (the gate, operating system, audio API, sample rate
and buffer size, controller, listening check), and `CONTRIBUTING.md` states which changes need a
run on hardware.

In the settings of Actions, the workflows of pull requests from outside collaborators wait for
the approval of the maintainer (`docs/maintainers/github-setup.md`). No code from outside the
project runs on the runners before the maintainer has looked at it; the pull requests of
collaborators run by themselves.

## How it was measured

Not measured. The choice rests on what CI runners can and cannot test, and on who can open a pull
request.
