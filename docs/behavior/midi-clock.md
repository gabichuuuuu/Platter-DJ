# MIDI clock

Platter sends the tempo and the beats of its master clock to other devices and programs as
MIDI clock. Following the MIDI clock of another device comes next. The master clock and the tempo
master are in `docs/behavior/sync.md`; the MIDI controllers in `docs/behavior/controllers.md`.
How the clock is sent and read on time is in `docs/decisions/0041-midi-clock.md`.

## What the public manual says

Sections of the public manual, read on 21 November 2026:

- *Preferences*, section *External Sync page*
- *Overview*, section *Master control panel*, and its *Further MIDI clock sync controls*
- *Global concepts*, section *Tempo synchronization*, *Master clock*
- *Advanced operation*, section *Syncing via MIDI clock*, with *Syncing to an external MIDI
  clock signal*, *Using the master clock as master* and *Syncing two instances*

The master clock is the tempo reference of the decks, the loops and the effects, and of other
devices and programs through MIDI clock. A page of the preferences turns MIDI clock on and
chooses the external clock source; with MIDI clock on, the master control panel shows an EXT
key and further controls: a key that starts and stops sending the clock, a key that makes the
devices that receive it start again, and the offset of the clock sent.

To follow another device, the DJ adds a generic MIDI device with the input that carries the
clock and turns EXT on; the master clock then follows the clock received once its sender
starts. The offset of a received clock is set on the sender. While the master clock follows an
external clock it is the tempo master, the automatic choice of the tempo master is off, and the
tempo changes only through the master clock.

To send, the DJ makes the master clock the tempo master, adds a generic MIDI device with the
output that goes to the other device, and starts sending. Two computers are checked against
each other by the tick of each, and the offset of the sender is moved until the ticks fall
together.

The manual does not say which messages are sent, how a received tempo is measured, how fast
the clock follows a change of tempo, the range or the unit of the offset, or what happens when
the clock received stops arriving.

The messages come from the MIDI 1.0 specification of the MIDI Association
(<https://midi.org/midi-1-0-core-specifications>), *System Real Time Messages*: Timing Clock
(`F8`), 24 to a quarter note; Start (`FA`); Continue (`FB`); Stop (`FC`).

## What Platter does

| Command | Effect |
| --- | --- |
| `clock.midi.output.set` | `port`: the MIDI output the clock is sent to, or none |
| `clock.midi.send.toggle` | Starts or stops sending the clock to that output |
| `clock.midi.restart` | Sends Stop and then Start on the next beat, so the devices that follow start again on it |
| `clock.midi.offset.set` | `ms` -50 to 50: how much earlier (negative) or later the clock is sent |

### Sending

- Nothing is sent until an output is chosen and sending is started. Sending starts with Start
  on the next beat of the master clock, then 24 Timing Clock messages to each beat; stopping
  sends Stop at once.
- The clock sent is that of the master clock: its tempo, bend included, and its beats, which
  are those of the deck that is the tempo master while one is (`docs/behavior/sync.md`).
- Each message leaves when the frame of its 24th of a beat is heard at the outputs of the audio
  device: the latency of the output is added, and then the offset, so a negative offset sends
  earlier. A total below zero sends the message as its frame is rendered.
- A jump of the tempo master, a seek or a beatjump of the deck that leads, is not sent as a
  burst of messages: the count goes on from the new place in the beat, and a device that
  follows takes the new phase at the next restart.
- The output, the offset and whether the clock is sent are kept with the settings. At start,
  sending is switched on again if it was on; messages go out once the output opens.
- The *Sync* page of the preferences holds the output, the Send switch, the Restart key and the
  offset.

### What the snapshot shows

Whether sending is on, and whether Start has gone out and Stop has not.

## Differences

- **The output is chosen by name in the preferences**, rather than by adding a generic MIDI
  device: a clock needs no mapping. The documented application puts the Start and Restart keys
  in its master panel; the master clock of `docs/design/` has no place for them, which is a
  question for the maintainer, so they are on the Sync page.
- **Start on the beat.** The manual does not say when Start is sent; Platter sends it on the
  next beat, so a device that follows starts in phase.
- **No song position.** Platter does not send Song Position Pointer: a track has no bar number a
  sequencer could use.
- **Following a received clock** (the EXT source of the manual) is not there yet. The input, the
  measurement of its tempo and the phase of the master clock against it come next, behind the
  same loop (`docs/decisions/0041-midi-clock.md`).
- **Link** is not part of this note: its public description is a project under the GPL, which
  Platter may not use as a guide.

## Edge cases

| Input | Result |
| --- | --- |
| The output goes away while sending | Its thread ends, the log says so and the Sync page says the output cannot be opened; choosing it again opens it |
| `clock.midi.send.toggle` without an output | Accepted: the switch is on and nothing is sent until an output opens |
| A new sample rate or buffer size | The sender keeps its output and takes the new latency; a new engine gives it a new queue |
| The same port as the output of the clock and as a controller | The clock uses a connection of its own |

## Tests

`crates/engine/src/rt/midi_clock.rs` for where the messages fall in the frames;
`crates/engine/src/midi_clock.rs` for the loop that times them;
`crates/engine/tests/midi_clock.rs` for the clock of the engine, a bend and a deck as the tempo
master; `crates/ui-egui/tests/settings_sync.rs` for the page; `apps/platter/src/config.rs` and
`apps/platter/src/clock_sender.rs` for the settings and the delay.