# External mixer mode

In external mixer mode Platter does not mix: each deck goes to its own pair of outputs of the
audio device, and a hardware mixer blends them. This note covers the switch between the two
modes, what reaches each output and which controls of the internal mixer still act. The
internal mixer is in `docs/behavior/mixer.md`, the master section in
`docs/behavior/master.md`.

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Preferences*, section *Output Routing page*
- *Preferences*, section *Mixer page*, the options of the internal and the external mixing
  modes
- *Preferences*, section *Effects page*

The output routing offers two mixing modes. In the internal mode the software mixes and the
DJ picks a pair of outputs for the master, one for the headphones (monitor) and one for
recording. In the external mode the DJ picks a pair of outputs for each deck, plus pairs for
the preview player and the return of send effects; several decks can share one pair when the
audio interface has few outputs. In the external mode the audio goes past the internal mixer
straight to the outputs. Some controls of the software can still act on it, each behind its
own option on the mixer page: the master volume over all outgoing signals, the gain and the
equaliser of each channel, and the mixer effects. Autogain, the limiter and the headroom have
options for this mode too. Post-fader effects are not available in it. The manual does not
say how the filter, the fader, the crossfader or the beat tick behave in this mode. The
section on the beat tick says that with an external mixer the tick is on the signal of the
deck (`docs/behavior/beatgrid.md`).

## What Platter does

| Command | Effect |
| --- | --- |
| `mixer.mode.set` | `mode` `internal` or `external` |
| `mixer.external.eq_gain.toggle` | In external mode, the gain and the equaliser of each channel act on its deck, or not |
| `mixer.external.master_volume.toggle` | In external mode, the master volume acts on both decks, or not |

- Platter starts in internal mode, with both options of the external mode off.
- In external mode deck A goes to the outputs the routing of the audio settings gives the
  master, the first and the second by default, and deck B to those it gives the headphones,
  the third and the fourth by default (`docs/behavior/settings.md`). When the device cannot
  carry the headphones, both decks share the master outputs, summed; with one output, that
  output gets the sum of both sides at half level, as in internal mode.
- The signal of a deck is the track as it plays, with key lock, flux and loops; then the gain
  and the equaliser when their option is on; then the master volume when its option is on;
  then the beat tick of the deck; then a safety limiter, one per pair of outputs.
- The filter, the pan, the fader, the crossfader and pre-listening keep their positions and
  keep taking commands, but do not act on the outputs. They act again in internal mode.
- There is no headphone signal in external mode: the hardware mixer pre-listens. A headphone
  test tone plays nothing; a master test tone plays on the first pair.
- Autogain sets the gain of the channel (`docs/behavior/autogain.md`), so in external mode it
  acts only with the gain and equaliser option on.
- The meter of a channel reads the signal of its deck before the limiter; the master meter
  reads the first pair of outputs. The limiter reading of the snapshot is that of the pair
  with the most reduction.
- The switch between the modes and each option follow the 10 ms ramp of every mixer
  parameter, so none of them clicks.
- The mode and both options are kept when the audio device changes its sample rate.

## Differences

- The outputs of the decks are those of the master and of the headphones in the routing,
  under those names; the reference product names a pair per deck. There is no preview
  player yet and no send effect, so there are no pairs for them.
- The option for mixer effects waits for the effects (M4). The limiter is always on and has
  no headroom setting.
- Autogain has no option of its own in external mode: it follows the gain, as above.
- The mode and both options are on the Mixer page of the preferences
  (`docs/behavior/settings.md`), which keeps them in the settings file. The design system
  collapses the mixer panel to the controls that still act in external mode; the panel does
  not do that yet, it comes with the next change to the mixer panel.
- Measured only with synthesised signals in the tests. It has not run on a device with four
  outputs feeding a hardware mixer.

## Edge cases

| Input | Result |
| --- | --- |
| A `mode` other than `internal` or `external` | The command is rejected; nothing changes |
| External mode with both options off and a track well below full scale | Each deck reaches its pair untouched by the mixer |
| Fader down, crossfader at the far side or filter closed, in external mode | The deck is still heard on its pair |
| External mode with a two-output device | Both decks, summed, through one limiter |
| Pre-listening in external mode | Nothing reaches the third and the fourth output from the headphones |
| Back to internal mode | The mix is the one the internal controls set, as before |

## Tests

`crates/engine/tests/external_mixer.rs`.
