# MIDI controllers: mappings

A MIDI controller sends a message when a button, fader, knob or encoder moves. A mapping says
which command each message triggers. This note covers the mapping file and how messages become
commands, in `platter-control`. Opening the MIDI ports, the controller manager of the
preferences, learn mode and the lights of the controller come later in M3, on top of it; until
then nothing in the application uses it. The choices behind it are in
`docs/decisions/0024-midi-mappings.md`.

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
  `release`, `when`, `param`, `range`, `invert`, `step` and `encoding`. The application checks
  each command the mapping may send, with its parameters, when it reads the file.
- A binding without `command` sets a modifier instead: `modifier` from 1 to 8 and `value` from
  0 to 7. Modifiers start at 0.
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
- Each message sends the command once, with the number of steps times `step` (1 when absent)
  in the parameter named by `param`.

### Limits

- A file of more than 1 MiB, or with more than 4 096 bindings, is refused. A binding with a
  mistake is refused naming its number, as in the shortcut file.
- A message that is not a note, a control change or a pitch bend, or that is cut short or has a
  data byte above 127, is ignored.
- A value too large to be a number, such as a huge `step`, is not sent.
- Several bindings may listen to the same control; each one whose conditions hold acts, in the
  order of the file.

## Differences

- Relative faders, soft takeover, the sensitivity and acceleration of encoders, and the
  outputs to the lights of the device need the current value of what they drive, which comes
  with the connection to the engine. They are not there yet.
- The reference manual does not give the values a modifier takes, nor how the two encodings of
  an encoder count steps. Platter takes 0 to 7 for the values and the counting above.
- A modifier is state of its mapping, not a command of the engine: it never leaves the mapping,
  as in the reference manual.
- The reference manual has a toggle mode and a mode that sets a fixed value. Platter has toggle
  commands and fixed parameters instead, so a button needs no state of its own beyond being
  down or up.
- The focus deck as a target comes with the controller manager.
- Mappings are TOML files of Platter. Mapping files of other applications are not read.

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

## Tests

`crates/control/tests/mapping.rs` on invented mappings: the file and its mistakes, buttons in
on press and on hold, modifiers and conditions, faders, encoders in both encodings, and messages that are
ignored. `crates/control/tests/untrusted.rs` feeds random bytes and random files and checks
that nothing panics. The MIDI messages follow the summary of MIDI 1.0 messages of the MIDI
Association, <https://midi.org/summary-of-midi-1-0-messages>.
