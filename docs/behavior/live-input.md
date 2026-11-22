# Live input

Platter takes audio from the inputs of an audio device and adds it to the mix: a turntable, a
synthesiser, a microphone or a hardware mixer. This note covers the input routing, the deck
that plays an input in the place of a track and the auxiliary input. How the input reaches the
engine, and why, is in `docs/decisions/0040-audio-input.md`. The output routing is in
`docs/behavior/settings.md` and `docs/behavior/external-mixer.md`.

## What the public manual says

Sections of the public manual, read on 21 November 2026:

- *Preferences*, section *Input Routing page*
- *Overview*, section *Decks and Deck Types* and section *Live Input*
- *Overview*, section *Mixer*, the AUX control

The input routing page chooses which inputs of the audio interface feed the software. For each
of the four decks it picks an input pair, for the deck to play an external device. A further
pair feeds the send effects from an external mixer (*FX Send*), and another the auxiliary input,
which exists only in the internal mixing mode. A level meter beside each choice shows the signal
when there is one. Several decks may take the same pair.

A deck can be of four types; one of them, the live input, is not a deck in the full sense: it
plays the audio of an external source in the mix and offers no control apart from the focus of
the deck. Through the mixer channel the signal takes the gain, the equaliser, the fader and the
effects as any deck does.

The mixer has an AUX knob, the volume of the auxiliary input, which can carry a microphone.

The manual does not say what happens when the input device stops, when the input and the output
are different devices, or what the meter reads.

## What Platter does

| Command | Effect |
| --- | --- |
| `input.device.set` | `device`: the name of an input device, or none to take no input |
| `input.routing.set` | `target` `deck_a` to `deck_d`, `aux`, `recorder` or `fx_send`; `input`: none, one input of the device (`1` to `8`) heard on both sides, or a pair (`1-2`, `3-4`, `5-6`, `7-8`) |
| `deck.type.set` | `type` `track` or `live`: the deck plays its track, or the input routed to it |
| `mixer.aux.set` | `level` 0 to 1, the volume of the auxiliary input in the master |

- No input is open until the user picks an input device: Platter starts with none, so it asks
  no permission of the system for the microphone until then.
- The input device is chosen apart from the output device, since most systems show the inputs
  and the outputs of one interface as two devices. It opens at the sample rate of the output; a
  device that does not accept that rate is refused with a message. The buffer size is that of
  the output.
- A single input is heard on both sides at its level, so a microphone on one input is centred.
  A pair keeps its sides.
- A deck of the live type plays the input routed to it through its channel: gain, equaliser,
  filter, pan, fader, crossfader, effects and pre-listening act on it as on a track. Its
  transport, cues, loops, sync and tempo do nothing; the track it holds, if any, is paused and
  stays loaded, and plays again from where it was when the deck goes back to the track type.
  With no input routed to it, a live deck is silent.
- Switching the type of a deck fades the outgoing signal out and the incoming one in over the
  10 ms of every mixer parameter, so it does not click.
- The auxiliary input joins the master after the channels and before the master volume, the
  maximiser and the limiter. It is silent in external mixer mode, as the manual says, and at
  level 0, where it starts.
- Each input of the device has a level meter in the snapshot, read as the meters of the
  channels are.
- When the input falls behind or runs ahead of the output, Platter plays it a little slower or
  faster, never more than half a percent, to keep the delay steady; when it stops, the inputs
  are silent and the snapshot counts the gap. The delay from input to output is about one
  buffer of the input and two of the output (`docs/decisions/0040-audio-input.md`).
- The input device, the routing and the type of each deck are kept with the settings, and the
  input opens again when the output does: after a change of device, rate or buffer size. The
  level of the auxiliary input is saved when Platter closes, as the knobs of the effects are.
- The *Input* page of the preferences holds them: the input device, and a row for each deck and
  for the auxiliary input with its input, a meter of it, the type of the deck or the level.
  It offers the inputs the open device has, two while none is open.
- An input whose stream ends is opened again at once. A device that is gone is then looked for
  every few seconds, and opens when the input devices of the system change, or when it was only
  held by another program.

## Differences

- **The FX Send is a target of the routing**, `fx_send`, for the units in send routing
  (`docs/behavior/effects.md`); its return goes to outputs 5 and 6 in external mixer mode.
- **Single inputs as well as pairs**, so a microphone on one input is centred, where the manual
  offers pairs only.
- **A separate input device**, because the systems Platter runs on often show one interface as
  an input device and an output device.
- **The auxiliary input is a command and a setting of the Input page**, with no knob on the
  mixer yet: the mixer of `docs/design/` has no place for it, which is a question for the
  maintainer. The same holds for the face of a live deck, which still shows its track.
- **The recorders take an input of their own**, the target `recorder` of `input.routing.set`:
  the external source of the mix recorder (`recorder.source.set`) and of the loop recorder
  (`External`), `docs/behavior/recording.md`. The loop recorder stays refused in external
  mixer mode.

## Edge cases

- An input device that goes away: the inputs are silent, the snapshot says the input is lost,
  and the output keeps playing.
- A routing to an input the device does not have: the target is silent, and the settings page
  offers only the inputs the device has.
- A live deck that is the tempo master or synced: it has no tempo, so it is never chosen as the
  master and the clock does not follow it.
- Loading a track into a live deck loads it and leaves it paused; it plays when the deck goes
  back to the track type.
