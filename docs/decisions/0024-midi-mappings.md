# 0024. MIDI mappings give command identifiers, and modifiers stay in the mapping

Status: accepted (M3).

## Context

`platter-control`, layer L1, turns the messages of MIDI controllers into commands
(`docs/architecture.md`). What it does is in `docs/behavior/controllers.md`. It cannot see the
commands of the engine, which is in L2, so it cannot build a `Command` nor read the range of a
parameter. Left open: what it gives, how a fader position becomes a value, and where modifiers
live, given that every action visible to the user is a command.

## Options

For the output:

1. Move `Command`, its specification and its untyped parameters to `platter-core`, so the
   mapping builds commands itself.
2. Give the command identifier and untyped parameters, and let the application build and check
   the commands, as the shortcut file does in the interface.

For faders: a position from 0 to 1 that the application turns into the value of the parameter,
or a range in the binding.

For modifiers: commands of the engine, or state of the mapping.

## Choice

- Option 2. The crate gives `Emit` values: an identifier and parameters, numbers or text.
  `Mapping::commands` lists every command a mapping may send, with its parameters, so the
  application builds each one with `Command::from_args` when it reads the file and refuses the
  mapping on a mistake, naming the binding. Moving the command types would touch the module
  that every open change of the engine touches, for no gain over checking at load time.
- A fader binding names its parameter and an optional linear range. The curves of the knobs of
  the interface are not a property of the commands but of the widgets, and a controller
  mapping should state the range it means; a channel gain takes `range = [-24, 12]`.
- Modifiers are state of the mapping. They select which binding acts, change nothing the user
  sees and are not seen outside their mapping, as in the reference manual; making them commands
  would put the private state of one mapping into the engine.
- The shape of the file follows `keymap.toml`, so the shortcuts can move into this crate later
  without changing their file. The keys of the binding avoid the names of command parameters:
  `control` and `hold`, not `type` and `mode`, which commands use.
- The MIDI messages follow the summary of MIDI 1.0 messages of the MIDI Association. How the two
  encoder encodings count steps, and that a modifier takes 0 to 7, are Platter's choice: the
  reference manual does not say.

## How it was checked

`cargo test -p platter-control` on Windows x64: the file and 16 kinds of mistakes, both button
modes, modifiers, faders on control changes and the pitch bend, encoders in both encodings,
and random bytes, random text and random bindings without a panic. The tests failed when the
value 0 of the offset encoding was taken as a step, and when a value too large for a number
was sent.

## Known limits

- Relative faders, soft takeover, encoder sensitivity and acceleration, and the lights of the
  device need the current value of their target and come with the connection to the engine.
- 14-bit control changes, sent as two messages, are not read; the pitch bend gives 14 bits.
- Nothing opens a MIDI port yet: that is `platter-device-io`, with `midir`.
