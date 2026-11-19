# Mixer

The mixer channels, the crossfader and the headphone output. M1 delivered gain, equaliser,
filter, fader and crossfader; M2 adds the kill buttons, pan, pre-listening and the controls of
the interface; M3 adds the assignment of a channel to a side of the crossfader and the step
and automatic fades of the crossfader.

## What the public manual says

Sections of the public manual, read on 13 November 2026 (*Crossfade controls* again on
15 November 2026, for the assignment):

- *Overview*, section *Mixer channel*
- *Overview*, section *Crossfade controls*
- *Advanced usage tutorials*, section *Using the cue channel for mixing*
- *Preferences*, section *Mixer page*

Each channel has a gain knob that sets the level before the fader, a level meter that also
shows clipping, three equaliser knobs for high, mid and low, a kill button per band that
removes the band completely, a pan knob that sets the balance between the left and the right
side, and a channel fader. A cue button sends the channel to the headphone cue channel, which
needs an audio device with more than one output pair. For the headphones, a volume knob sets
the level and a mix knob sets the balance between the cued channels and the main mix: turned
one way the cued channels, turned the other way the main mix. The crossfader blends the two
sides, and a preference shapes its curve continuously from smooth to sharp. Assign buttons
put a channel on the left or the right side of the crossfader, so it is heard when the
crossfader is on that side. Fade buttons move the crossfader step by step towards a side, and
auto fade buttons move it by itself from one side to the other; a preference of the mixer
page sets the time the automatic crossfade takes (*Preferences*, section *Mixer page*, read
on 15 November 2026). The manual gives neither the size of a step nor the unit of that time. The preferences
also offer several equaliser types and filter types; see *Equaliser and filter models* below.
The section of the manual on the mixer
channel does not describe the one-knob filter; the behaviour below (low-pass one way,
high-pass the other) is Platter's own definition.

## What Platter does

Signal path of a channel: gain, equaliser, filter, fader, pan, meter. Then the crossfader, the
sum of both sides and the master section (`docs/behavior/master.md`). The headphones take each
pre-listened channel after its filter and before its fader.

| Command | Effect |
| --- | --- |
| `mixer.gain.set` | Channel gain in decibels, silence to +12 dB |
| `mixer.gain.restore`, `mixer.autogain.toggle` | Autogain: `docs/behavior/autogain.md` |
| `mixer.eq.set` | Position of a band knob, 0 to 1: 0 removes the band, 0.5 leaves it untouched, 1 boosts it by 12 dB |
| `mixer.eq.kill.toggle` | Removes a band completely, or gives it back to its knob |
| `mixer.filter.set` | Position of the filter knob, -1 to 1: low-pass below zero, high-pass above, no effect at zero |
| `mixer.pan.set` | Position of the pan knob, -1 (left) to 1 (right) |
| `mixer.fader.set` | Fader position, 0 to 1 |
| `mixer.cue.toggle` | Sends the channel to the headphones, or stops sending it |
| `mixer.cue.mix.set` | Headphone mix, 0 (pre-listened channels alone) to 1 (master alone) |
| `mixer.cue.volume.set` | Headphone volume in decibels, silence to +12 dB |
| `mixer.crossfader.set` | Position, 0 (left side alone) to 1 (right side alone) |
| `mixer.crossfader.curve.set` | `smooth` (equal power), `linear` or `sharp` |
| `mixer.crossfader.step` | `direction` `left` or `right`: moves the crossfader a tenth of its travel that way |
| `mixer.crossfader.auto_fade` | `direction` `left` or `right`: moves the crossfader to that end by itself |
| `mixer.crossfader.fade_beats.set` | `beats` 1 to 64: how long an automatic fade across the whole travel takes |
| `mixer.xfader_assign.set` | `side` of the crossfader the channel is on: `left`, `thru` or `right` |
| `mixer.eq.model.set` | `model` of the equaliser of every channel: `smooth`, `soft`, `isolator` or `steep` |
| `mixer.filter.model.set` | `model` of the filter of every channel: `gentle`, `steep` or `ladder` |

- Every parameter follows a 10 ms ramp; none of them steps. That includes the kill buttons
  and the pre-listen buttons.
- The equaliser splits the signal at 250 Hz and 2.5 kHz
  (`docs/decisions/0005-equaliser-topology.md`). A killed band keeps the position of its knob
  and returns to it when the kill is released.
- The filter sweeps its corner exponentially: the low-pass from 20 kHz down to 60 Hz, the
  high-pass from 20 Hz up to 10 kHz (`docs/decisions/0006-channel-filter.md`).
- Pan is a balance control: at the centre both sides pass untouched; turning to one side
  fades the other side out in a straight line, down to silence at the end. The side the knob
  points at never gets louder.
- The fader has a squared taper around unity at three quarters of its travel: silence at the
  bottom, no change at 0.75 and 5 dB of gain at the top. It starts at unity.
- Deck A starts on the left side and deck B on the right one, as the crossfader has always
  blended them. A channel on a side gets the gain the curve gives that side; a channel on
  `thru` passes at full level wherever the crossfader is. Both channels can be on the same
  side, and then the crossfader fades both together. The change follows the 10 ms ramp.
- An automatic fade moves the crossfader at a steady speed: the whole travel in the number of
  beats set, 8 to begin with, at the tempo of the master clock (`docs/behavior/sync.md`); from
  the centre it takes half of that. It follows the clock when the tempo changes during the
  fade, and stops at the end. It runs on the audio thread, so it is as smooth as a ramp.
- During an automatic fade the snapshot shows where the crossfader is. `mixer.crossfader.set`
  or a step stops the fade where the crossfader is and acts from there; another automatic
  fade turns it to its own end. An automatic fade towards the end the crossfader is at does
  nothing.
- A step goes from where the crossfader is, during a fade too, and stops at the end.
- The assignment acts on the master only: pre-listening in the headphones is before the
  fader and the crossfader.
- The meter of a channel reads the peak of each side after the fader and the pan.
- In external mixer mode the decks go to their own outputs past this mixer:
  `docs/behavior/external-mixer.md`.
- The headphones are the third and the fourth output of the device. Their signal is the sum
  of the pre-listened channels and the master, weighted by the mix knob in a straight line,
  times the volume, through a safety limiter of their own. With a device of fewer than four
  outputs there is no headphone signal, and the interface says so.

## Differences

- The crossfader has three named curves instead of a continuous control.
- Four equaliser models and three filter models of Platter's own: see *Equaliser and filter
  models* below.
- The pan knob and the pre-listen button are always visible; the documented application shows them
  only in some layouts.
- The fader adds up to 5 dB above its unity mark.
- The public manual names a left and a right assign button and does not say what a
  channel assigned to neither does. Platter makes that state, `thru`, explicit: the channel
  bypasses the crossfader. Under each fader the window has L and R: a click on the side that
  is off sends the channel there, a click on the side that is lit sends it to neither.
- A step is a tenth of the travel and the time of an automatic fade is counted in beats of
  the master clock, so a fade ends on the beat count the DJ chose at any tempo; the manual
  says neither. Outside each badge of the crossfader, a key steps it towards that side and an
  *Auto* key fades it there by itself.
- The Fade In and Fade Out markers that start an automatic crossfade in the documented application
  are not part of this: the fade and load cue types are labels only
  (`docs/behavior/cues.md`).

## Edge cases

| Input | Result |
| --- | --- |
| A position or a gain outside its range, or not a number | The command is rejected; nothing changes |
| All three bands at zero or killed | Exact silence from the channel |
| Filter at exactly zero | The signal passes bit for bit |
| Crossfader at an end | The far side is exactly silent, with every curve |
| Pan at an end | The far side is exactly silent |
| A channel on a side with the crossfader fully on the other one | The channel is exactly silent on the master |
| A channel on `thru` | It passes bit for bit at every crossfader position and curve |
| A `side` other than `left`, `thru` or `right` | The command is rejected |
| A change of sample rate of the audio device | The assignment and the length of a fade are kept; a running fade stops where it is |
| A step past an end, or an automatic fade towards the end it is at | The crossfader stays at the end |
| `direction` other than `left` or `right`, `beats` outside 1 to 64 | The command is rejected |
| Pre-listening or a headphone test tone on a device with two outputs | Nothing reaches the outputs |
| Everything turned up in the headphones | The output stays inside full scale |

## Equaliser and filter models

### What the public manual says

*Preferences*, section *Mixer page*, *EQ / Filter Selection*, read on 18 November 2026. The
equaliser type of the internal mixer is chosen among six. One is the standard three-band
equaliser of the application, four are three-band equalisers modelled on other mixers, one
of them a full-kill isolator with slopes of 24 dB per octave, and one adds a second mid band,
four bands in all, which a controller may not reach. The filter type is chosen among three: a
ladder with a steep response and a lot of resonance, one with a mellow slope and less
resonance, and one that is steep but less resonant than the ladder. The manual describes the
other types only by the mixers they are modelled on.

### What Platter does

- One equaliser model and one filter model for every channel, chosen on the Mixer page of the
  preferences (`docs/behavior/settings.md`) and kept in the settings file.
- Equaliser models, all with the bands at 250 Hz and 2.5 kHz, the same knobs and the same
  kill: `smooth` (edges of 6 dB per octave), `soft` (12), `isolator` (24, the default) and
  `steep` (48). The steeper the edge, the less of its neighbours a removed band takes with it:
  two octaves below the edge of the low band, a removed low band leaves a tone 12, 25, 48 or
  96 dB down. Every model is flat with the knobs at the centre and silent with all bands at
  zero.
- Filter models, all with the same knob, ranges and fade around the centre: `gentle` (12 dB
  per octave, a slight emphasis at the corner, the default), `steep` (24 dB per octave, no
  emphasis) and `ladder` (24 dB per octave with a resonant peak about 8 dB high at the
  corner).
- A change of model fades over 10 ms while the music plays, so it does not click. A change
  asked for during that fade follows when it ends.
- The models are kept when the audio device changes its sample rate.
- The design of each model and how it was measured: `docs/decisions/0032-mixer-models.md`.

### Differences

- The models are Platter's own and are named by their response; none imitates a mixer.
- Four equaliser models instead of six. There is no four-band model: a fourth knob per
  channel needs a design of the mixer channel that does not exist yet.
- The ladder can lift the corner by about 8 dB; the safety limiter of the master holds what
  passes full scale.

### Edge cases

| Input | Result |
| --- | --- |
| A `model` the mixer does not have | The command is rejected; nothing changes |
| A model stored in the settings file that Platter does not know | The default model |
| A change of model while a band is killed or the filter is turned | The kill and the position stay; the new model applies them |
| Several changes of model faster than 10 ms | Each waits for the fade before it; the last one asked for stays |

### Tests

`crates/dsp/src/eq.rs`, `crates/dsp/src/filter.rs`, `crates/dsp/src/switch.rs`,
`crates/engine/tests/mixer_models.rs`, `crates/engine/tests/realtime_memory.rs` and
`apps/platter/src/config.rs`.

## Tests

`crates/dsp/src/eq.rs`, `crates/dsp/src/filter.rs`, `crates/dsp/src/crossfade.rs`,
`crates/dsp/src/balance.rs`, `crates/engine/tests/mixer.rs` and
`crates/engine/tests/monitoring.rs`; the steps and the automatic fades in
`crates/engine/tests/crossfade.rs`.
