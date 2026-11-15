# 0026. MIDI controllers in the application: named in the settings, carried out by the frames

Status: accepted (M3).

## Context

`platter-control` turns MIDI messages into identifiers and untyped parameters
(`docs/decisions/0024-midi-mappings.md`), and `platter-device-io` receives the messages of an
input into a bounded queue (`docs/decisions/0025-midi-ports.md`). The application owns the
engine and carries out every command on the thread of the window. Left open: where the user
says which controller to use before the controller manager exists, when a mapping is checked
against the commands, and on which thread and how soon the messages are carried out. What the
application does is in `docs/behavior/controllers.md`, section *In the application*.

## Options

For the thread that carries out the messages:

1. The frames of the window: each frame takes the messages waiting, as it takes the keys.
2. A thread of its own that waits for messages and sends the commands to the engine.

For how soon a frame comes:

1. The idle repaint of the window, every 100 ms.
2. The thread of the port wakes the window after each message it queues.

## Choice

- The frames of the window, option 1. The engine and the application state have one owner,
  and MIDI goes through the same `dispatch` path as the keyboard and the interface, as
  `AGENTS.md` asks of every input. Option 2 would need the engine shared between two threads.
- The port wakes the window, option 2. `platter_device_io::open` takes a function that the
  thread of the port calls after each queued message; the application passes
  `request_repaint` of the egui context, which the egui documentation says wakes the thread of
  the window when called from another thread under eframe. `platter-device-io` knows nothing
  of egui. Waiting for the idle repaint would add up to 100 ms to a button.
- A frame takes at most 64 messages of each input; the window asks for the next frame at once
  when more may be waiting. That bounds the commands one frame sends to the engine, whose queue
  to the processor holds 256.
- A mapping is checked once, when it is read: `Mapping::commands` lists every command it may
  send, with a fader at both ends of its range, and each one must build with
  `Action::from_args` and keep its numbers inside the ranges of the command specification.
  A mistake is found when the application starts, not when a control is touched in a set.
- The settings file names each controller by its input and its mapping file, up to 8, until
  the controller manager writes them. A missing input is written in the log and not looked for
  again until the next start: noticing devices plugged in comes with the controller manager.
- What the engine refuses is written in the log the first time for each controller and
  command; lost messages at most every 10 seconds. A fader moved on an empty deck must not
  write to the log file on the thread of the window at the rate of the controller.

## How it was checked

`cargo test -p platter` on Windows x64: the check of mappings (unknown commands and
parameters, a parameter given only on release, a fader past its range, a fixed value out of
range), messages turned into actions of the engine and of the application, the refusal of a
mapping file larger than the limit, the reports of lost messages, and the controllers of the
settings file. `cargo test -p platter-device-io`: only a message that enters the queue wakes
the window. No MIDI input exists on the computer used for development, so the path from a
real port to the engine has not run. The installed egui 0.37 source was read for
`request_repaint`.

## Pending

These wait for a MIDI controller (`ROADMAP.md`, checks waiting for equipment):

- A real input driving the engine through a mapping, on Windows, then macOS and Linux.
- The delay from a message to its command, and whether a minimised window still runs frames
  on each system.
