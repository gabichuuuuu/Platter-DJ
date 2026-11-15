# 0025. MIDI inputs through midir, with a bounded queue per input

Status: accepted (M3).

## Context

Platter needs to receive the messages of MIDI controllers on Linux, macOS and Windows.
`docs/dependencies.md` approves `midir` for it in M3, in `platter-device-io` (layer L3). What
the ports do is in `docs/behavior/controllers.md`, section *Ports*. Left open: how messages
leave the thread that receives them, and which messages are kept.

## Options

1. Hand each message to a function the caller gives, on the thread of `midir`.
2. Copy each message into a bounded queue that the caller empties when it wants.

## Choice

- Option 2, with the standard bounded channel and a non-blocking send. The thread of the port
  never waits for the application, and a message that does not fit is counted rather than
  held. Option 1 would run the code of the caller on a thread of the system, which on Windows
  is the thread of the MIDI driver.
- `midir` 0.11.1, MIT, confined to the module `port`; no type of it is public. Its sources
  were read for each backend: WinMM, ALSA and CoreMIDI call back once per message, and a
  channel message is at most three bytes, so the queue holds messages of three bytes and
  counts longer ones as left out. System exclusive, clock and active sensing are ignored at the
  port, since no mapping uses them yet.
- An input is opened by name, the first one with that name. The stable identifier `midir`
  gives each port is not used yet: the settings that will remember a port come with the
  controller manager.
- The project does not write the access to the MIDI system of each platform: it would need
  `unsafe` bindings to three system APIs.

## How it was checked

`cargo test -p platter-device-io` on Windows x64: the queue keeps the order, counts what a
full queue loses, leaves out long and empty messages, and does not fail when nobody reads it.
`cargo deny check licenses bans` passes with `midir` and what it brings. On the computer used
for development the system listed no MIDI input, and opening a missing input gave its error;
no real port was opened and no message from a device was received. The ALSA and CoreMIDI
backends are compiled by CI only.

## Known limits

- No output ports, no virtual ports and no notice when a device is unplugged; an unplugged
  input stops sending.
- Two devices with the same name cannot be told apart yet.
