# MIDI controllers: mappings and ports

A MIDI controller sends a message when a button, fader, knob or encoder moves. A mapping says
which command each message triggers. This note covers the mapping file and how messages become
commands, in `platter-control`, the MIDI inputs that receive them, in `platter-device-io`, and
how the application joins the two, named in its settings file. The controller manager of the
preferences, learn mode and the lights of the controller come later in M3, on top of them. The
choices behind them are in `docs/decisions/0024-midi-mappings.md`,
`docs/decisions/0025-midi-ports.md` and `docs/decisions/0026-midi-in-the-application.md`. No
real controller has been tried yet (`ROADMAP.md`, checks waiting for equipment).

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Preferences*, section *Controller Manager page*
- The chapter on configuring MIDI controllers, sections *Controller Manager Overview*,
  *Assigning MIDI Controls* and *Available Controller Types*

The controller manager keeps the mappings of the keyboard and of MIDI controllers, which can
be added, imported, exported, duplicated and deleted. Every mapping is active at once, each
with its input and output port. A mapping is a table of assignments: a control of the
application, the control of the device it listens to, and how it does. Learn assigns the next
control the user moves.

An assignment is to one deck, to the deck that has the focus, or global. Buttons hold a
function while pressed, toggle it, or set it to a fixed value. Faders and knobs set the value
directly or move it relatively, optionally inverted, with a soft takeover that waits for the
hardware to reach the value. Encoders send relative steps in one of two encodings, with a
sensitivity and an acceleration. A mapping has eight modifiers, values set by controls of the
same mapping; an assignment can require up to two of them to have given values, and modifiers
of one mapping are not seen by another. Outputs drive the lights of the device.

Each mapping has an input port and an output port, both set to all ports at first; the manual
advises choosing the port of the device. Virtual ports pass MIDI between two programs on the
same computer.

## What Platter does

### The file

A mapping is a TOML file, in the shape of the shortcut file (`docs/behavior/keyboard.md`):

```toml
[[binding]]
midi = "note 1 36"
command = "deck.play_pause"
deck = "a"

[[binding]]
midi = "cc 1 7"
command = "mixer.fader.set"
deck = "a"
param = "position"

[[binding]]
midi = "note 1 40"
modifier = 1
value = 1
hold = true

[[binding]]
midi = "note 1 37"
command = "deck.cue"
deck = "a"
action = "press"
hold = true
release = { action = "release" }
when = ["1 = 1"]
```

- `midi` names the control: `note C N` for a key or pad, `cc C N` for a control change and
  `pitch C` for the pitch bend of channel `C`, from 1 to 16. `N` is from 0 to 127 for a note
  and from 0 to 119 for a control change; 120 to 127 are channel mode messages.
- `command` is the identifier of a command. Every other key is a parameter of the command, as
  in the shortcut file, except the keys of the binding: `midi`, `command`, `control`, `hold`,
  `release`, `when`, `param`, `range`, `invert`, `step`, `encoding`, `up` and `down`. The
  mapping lists each command it may send, with its parameters, so that the application can
  check them when it reads the file; that comes with the connection to the engine.
- A binding without `command` sets a modifier instead: `modifier` from 1 to 8 and `value` from
  0 to 7. Modifiers start at 0. A binding with both `command` and `modifier` is a mistake.
- `when` lists up to two conditions, `M = V`: the binding acts only while modifier `M` has
  value `V`. Conditions are checked against the modifiers as they were before the message.
- `control` is `button`, `fader` or `encoder`. A note is a button; a control change is a fader
  unless the binding says otherwise, and can be any of the three; the pitch bend is a fader.
- A key that belongs to another kind of control is a mistake: `range` on a button, `hold` on a
  fader.

### Buttons

- A note-on with a velocity above 0 presses a button; a note-off, or a note-on with velocity 0,
  releases it. A control change presses with a value above 0 and releases with 0.
- By default a button sends the command when it goes down. A toggle is a
  command of its own (`deck.keylock.toggle`), and a fixed value is a parameter of the binding
  (`mixer.crossfader.set` with `position = 0.5`).
- With `hold = true` it also sends the command when it comes up, with the parameters of
  `release` in place of those it names; a command on hold needs `release`. A modifier on hold
  goes back to 0 on release.
- The conditions are checked when the button goes down. A button that acted sends its release
  even if the modifiers changed in between; one that did not act sends nothing.
- A press of a button already down, or a release of one already up, does nothing.

### Faders and knobs

- The position of a control change, from 0 to 127, or of the pitch bend, from 0 to 16 383,
  becomes a number from 0 to 1, with the centre of the pitch bend exactly half way, or from the
  bottom to the top of `range = [bottom, top]` when
  the binding gives one, and goes in the parameter named by `param`. `invert = true` turns the
  control upside down.
- The range is linear. A channel gain in decibels takes a range such as `[-24, 12]`.

### Encoders

- An encoder sends steps. With `encoding = "7f-01"`, the default, 1 to 63 are steps up and 127
  down to 65 are steps down, 127 being one. With `encoding = "3f-41"`, 65 and above are steps
  up, 65 being one, and 63 and below are steps down, 63 being one. 0 and 64 are no step.
- With `param`, each message sends the command once, with the number of steps times `step` (1
  when absent) in that parameter: `browser.move` with `param = "delta"`.
- With `up` and `down` instead, each step sends the command once, with the parameters of `up`
  or of `down` in place of those they name, at most 16 times for one message:
  `deck.beatjump` with `up = { direction = "forward" }` and `down = { direction = "back" }`.
  An encoder takes one way or the other, not both.

### Limits

- A file of more than 1 MiB, or with more than 4 096 bindings, is refused. A binding with a
  mistake is refused naming its number, as in the shortcut file.
- A message that is not a note, a control change or a pitch bend, or that is cut short or has a
  data byte above 127, is ignored.
- A value too large to be a number, such as a huge `step`, is not sent.
- Several bindings may listen to the same control; each one whose conditions hold acts, in the
  order of the file.

### Ports

- Platter lists the MIDI inputs of the system by the names the system gives them, and opens an
  input by its name. When two inputs have the same name, the first is opened.
- System exclusive, clock and active sensing messages are left out at the port. A message
  longer than three bytes, which a channel message never is, is left out and counted.
- The messages of an open input wait in a queue of their own, in the order they arrived. The
  port never waits for whoever reads them: when the queue is full, a message is counted and
  lost.
- An input that does not exist, or that another program holds, is an error naming it. An
  input closes when Platter lets it go.

### In the application

Until the controller manager exists, the settings file names the controllers, each with its
input and its mapping:

```toml
[[midi]]
input = "Example Controller"
mapping = "example-controller.toml"
```

- A mapping named without a folder is looked for in the folder of the settings file. At most
  8 controllers are read; an entry without an input or a mapping is left out. Saving the
  settings keeps them.
- When the application starts it reads each mapping and checks every command it may send, as
  `Mapping::commands` lists them: the command exists, takes every parameter the binding gives
  it, and every number is inside the range of its parameter, at both ends of a fader. A
  mapping with a mistake is not used, and the log names the mistake; the other controllers
  still open.
- A file larger than a mapping may be is refused before it is read whole.
- Then the input is opened. An input that is not there, or that another program holds, is
  written in the log; the controller stays off until the application starts again.
- A message wakes the window, which takes the messages of each input in the order they
  arrived, at most 64 per input and frame, and carries out the commands of each one in the
  order of the mapping, through the same path as the keyboard and the interface. What is left
  waits for the next frame, which comes at once.
- A command the engine refuses is written in the log the first time only, for each controller
  and command, so a fader moved on an empty deck does not fill the log. Messages lost because
  the queue of an input was full, or left out because they were too long, are written in the
  log at most once every 10 seconds, with their count.

## Differences

- Relative faders, soft takeover, the sensitivity and acceleration of encoders, and the
  outputs to the lights of the device need the current value of what they drive, read back
  from the engine. They come in a later unit of M3.
- The reference manual does not give the values a modifier takes, nor how the two encodings of
  an encoder count steps. Platter takes 0 to 7 for the values and the counting above. Taking a
  note-on with velocity 0 as a release is Platter's choice too, as controllers send it.
- A modifier is state of its mapping, not a command of the engine: it never leaves the mapping,
  as in the reference manual.
- The reference manual has a toggle mode and a mode that sets a fixed value. Platter has toggle
  commands and fixed parameters instead, so a button needs no state of its own beyond being
  down or up.
- The focus deck as a target comes with the controller manager.
- Mappings are TOML files of Platter. Mapping files of other applications are not read.
- Output ports, the choice of all ports for a mapping, virtual ports and noticing a device
  plugged in or out come with the controller manager. An input whose device is unplugged
  stops sending; it is not reported yet, and a device plugged in after the start is not
  opened until the application starts again.
- Choosing the controllers in the settings file by hand is a stand-in for the controller
  manager, as the folders of the file selector are.
- The messages are carried out by the frames of the window. Whether a minimised window still
  runs frames on each system, and the delay from a message to its command, have not been
  measured: they need a controller.

## Edge cases

| Input | Result |
| --- | --- |
| A note-on with velocity 0 | A release |
| A pitch bend at its centre, 8 192 | Exactly half way: 0.5, or the middle of the range |
| A message of channel 16 | Matches bindings of channel 16; the status byte counts from 0 |
| An encoder value of 0 or 64 | No step; the command is not sent |
| Bytes of another kind, a message cut short, a data byte above 127 | Ignored |
| A release whose press did not act | Nothing |
| A modifier condition with modifier 9 or value 8 | The binding is refused |
| `cc 1 120` | The binding is refused |
| A key that is a parameter of the command, `mode` or `type` | Passed to the command |
| A mapping that names a command that does not exist, or a parameter the command does not take | The mapping is not used; the log says why |
| A fader whose range goes past the range of its parameter | The mapping is not used |
| A controller whose input is not there at the start | Written in the log; the other controllers work |

## Tests

`crates/control/tests/mapping.rs` on invented mappings: the file and its mistakes, buttons on
press and on hold, modifiers and conditions, faders, encoders in both encodings, and messages
that are ignored. `crates/control/tests/untrusted.rs` feeds random bytes and random files and
checks that nothing panics. The unit tests of `crates/device-io/src/queue.rs` check the queue
of a port: order, a full queue, long and empty messages. The unit tests of
`apps/platter/src/midi.rs` check a mapping against the commands and turn invented messages
into actions of the engine and of the application; those of `apps/platter/src/config.rs` keep
the controllers of the settings file. Opening a real port needs a device and waits for one. The MIDI messages follow the summary of MIDI 1.0 messages of the MIDI
Association, <https://midi.org/summary-of-midi-1-0-messages>.
