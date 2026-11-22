# Effects

The effect units, their assignment to the mixer channels, and the one-knob mixer effect of
each channel. M4 delivers effect units 1 and 2 in group and single mode, the insert and
post-fader routings, the first eight effects and the mixer effect. Design of the processing:
`docs/decisions/0033-effect-chain.md`.

This note was written before the code; the maintainer settled its open points on 18 November
2026 (*Decided* below).

## What the public manual says

Sections of the public manual, read on 18 November 2026:

- *Overview*, sections *FX Units*, *Mixer Channel* and *Global Section*
- *Advanced usage tutorials*, section *Working with FX Units* and its subsections on
  assigning units to channels, switching modes, single mode, group mode and snapshots
- *Advanced operation*, sections on enabling the third and fourth units and on send mode
- *Basic usage tutorials*, section *Working with Mixer FX*
- *Preferences*, sections *Effects page* and *Mixer page*
- *Effect Reference*, the sections of the delay, the reverb, the flanger, the phaser, the
  filter, the gater, the buffer effect that repeats slices of a bar, and the effect that
  lowers the bit depth and the sample rate

**Units.** There are four effect units. Units 1 and 2 are shown by default, at the left and
the right of the global section; a preference shows four, and then units 1 and 3 share the
left panel and units 2 and 4 the right one. Any unit can be assigned to any mixer channel, and
to several at once: each channel has one assign key per unit, lit while the channel goes
through that unit.

**Modes.** A context menu on the number of a unit switches it between two modes:

- *Group*: up to three effects in a chain. Each has a selector, an amount knob and an on key.
  One dry/wet knob mixes the dry signal and the output of the chain. Double clicking a knob
  puts it back to its default.
- *Single*: one effect, with a selector, a dry/wet knob, three parameter knobs, two parameter
  keys, an on key and a reset key that puts every control of the panel back to its default.
  The overview counts four knobs and four keys and its legend names three of each; the
  effect reference gives each effect up to five controls.

A third mode plays rhythmic patterns; it belongs to the pattern sequencer of M5.

**Routing.** The Effects page of the preferences sets the routing of each unit:

- *Insert*: the unit sits in the channel just before the channel filter, so the equaliser
  and the filter act on its output.
- *Post fader*: the unit sits after the channel fader, so its output stays audible after the
  fader closes. Not available in external mixer mode.
- *Send*: the unit runs outside the internal signal flow, fed by an input of the audio
  interface and returned to one of its outputs, for the send and return loop of an external
  mixer. It works only in external mixer mode; several send units chain in the order 1, 2, 3,
  4.

A preference chooses whether a newly selected effect starts from its default values. Holding
the unit number offers *Save snapshot*, which keeps the current values of the effect in that
unit and gives them back the next time the effect is selected there. Another preference
narrows and orders the list of effects the selectors offer.

**Effects.** The effect reference gives, for each effect, the controls of single mode and the
one control the amount knob drives in group mode:

| Effect | Single mode | Group mode amount |
| --- | --- | --- |
| Delay | Filter in the feedback loop, feedback, time in fractions of a bar from 1/32 to one bar following the master clock, freeze, stereo spread | Time, at a medium feedback |
| Reverb | Low cut and high cut in the loop, room size, freeze | Size |
| Flanger | Stereo spread, feedback, rate from 16 bars to 1/16 of a bar, sweep direction, free-running rate | Rate, free-running |
| Phaser | As the flanger | As the flanger |
| Filter | Low cut, resonance, high cut, band reject, a one-knob mode (low-pass one way, high-pass the other) | The one-knob mode |
| Gater | Added noise, gate shape, rate from a quarter to 1/32 of a bar, mute (noise only), stutter | Rate |
| Slices of a bar | On (captures one bar at the master tempo), mix of the captured slices, rotation in eighth notes, slice length, re-sync on each bar, reverse | Length |
| Bit depth and rate | Bit depth, sample-rate reduction, smoothing, stereo spread | Bit depth and rate together |

Delay and flanger keep to the master clock when it changes tempo. The reference does not say
whether a tail rings on after an effect is switched off, nor give default values.

**Mixer effect.** Each channel has a mixer effect knob with an on key and a selector. The
filter is always one of the choices and is the default; four slots of the Mixer page of the
preferences choose the other effects the selector offers. With the filter, the knob is
low-pass one way and high-pass the other. The manual does not describe how the other mixer
effects follow the knob, nor what its centre does.

**Not stated.** Default values, the order of several insert units on one channel, how a unit
assigned to several channels processes them, what the tails do when an effect is switched off
or a unit is unassigned.

## What Platter does

### Units and modes

- Four units exist in the engine. The effect row, under the status header in both layouts,
  shows units 1 and 2 (`docs/design/DESIGN.md`, *EffectUnit*).
- A preference of the Mixer page shows two units or four (`app.effect_units.set`, kept in the
  settings file under `[effects]` as `shown`; two to begin with). With four, the header of
  the left panel has the keys `1` and `3`, and the right one `2` and `4`: they choose the
  unit the panel shows (`app.fx_panel.show`). Going back to two shows units 1 and 2 again.
- With four units each channel has four assign keys, one per unit, as the public manual
  gives each channel one key per unit; with two it has the keys 1 and 2. A channel stays
  assigned to units 3 and 4 while they are hidden, and they keep acting on it, as decks C and
  D keep playing while the two-deck layout hides them.
- The Mixer page scrolls: its *Effects* section, under *Mixing*, holds the number of
  units, the routing of each unit (*Insert* or *Post fader*) and *Keep values*. The wheel scrolls the page,
  and the keyboard focus brings a control it reaches into view.
- The two-deck mixer has the assign keys 1 and 2 of each channel, side by side in the row of
  its crossfader assign keys, and no mixer effect knob: its channels have no room for one. The
  mixer effect of decks A and B is turned in the four-deck layout, or by command. The
  maintainer approved this placement on its capture on 18 November 2026.
- Each unit is in `group` or `single` mode (`fx.unit.mode.set`). A unit starts in group mode
  with three empty slots.
- Group mode: three slots, each with an effect (`fx.slot.effect.set`, or `none`), an amount
  from 0 to 1 (`fx.slot.amount.set`) and an on key (`fx.slot.toggle`). One dry/wet knob per
  unit, 0 to 1 (`fx.unit.dry_wet.set`).
- Single mode: one effect (`fx.single.effect.set`), the dry/wet knob, three parameter knobs
  from 0 to 1 (`fx.single.knob.set`, `knob` 1 to 3), two parameter keys
  (`fx.single.key.toggle`, `key` 1 or 2), an on key (`fx.single.toggle`) and a reset key that
  puts the three knobs and the two keys back to the defaults of the effect (`fx.unit.reset`).
- Switching the mode keeps what each mode had, so switching back finds it again.
- A newly selected effect starts from its defaults, or from its snapshot in that unit when
  there is one. *Keep values*, a key of the *Effects* section of the Mixer page
  (`fx.keep_values.toggle`, off to begin with, kept in the settings file under `[effects]` as
  `keep_values`), turns this round for every unit: while it is on, a newly selected effect
  leaves the three knobs and the two keys of single mode, and the amount of a slot of group
  mode, where they were, and no snapshot is applied. It is the public manual's option to
  restore the parameters when the effect changes, with its sense inverted so that the key is
  lit when it does something other than the default. *Save snapshot* (`fx.unit.snapshot.save`) is the menu of the unit title, which
  a click or the right button opens; it keeps the values of the effect of single mode, so the
  title opens it only in single mode with an effect. Snapshots, modes, effects, values, routings, assignments and
  the effect of each mixer effect knob are kept in the settings file, under `[effects]`;
  the on keys and the mixer effect knobs are not, so Platter starts with every effect off.

### Assignment and routing

- `mixer.fx_assign.toggle` (`deck`, `unit`) puts the channel through the unit, or takes it out.
  The assign keys 1 and 2 of each channel in the M4 screen are these keys.
- Each unit has a routing (`fx.unit.routing.set`): `insert` (the default), `post_fader` or
  `send`, set on the Mixer page of the preferences. In external mixer mode *Post fader* is
  disabled there.
- A unit in `send` routing acts on no channel. In external mixer mode it takes the input
  routed to the FX Send on the Input page (`input.routing.set` with the target `fx_send`,
  `docs/behavior/live-input.md`) and returns it on outputs 5 and 6 of the device; several
  units in send routing chain in the order 1, 2, 3, 4, each with its own dry/wet. In internal
  mode, or while no unit is in send routing, the return is silent; it fades in and out over
  the switch time of the units. The send chain of a unit holds its effects whatever its
  routing, so a unit switched to send is heard at once.
- Signal path of a channel: gain, equaliser, the units in insert routing it is assigned to,
  filter, fader, pan, the units in post-fader routing it is assigned to, meter. Then the
  crossfader. The manual places the insert just before the filter; in Platter's channel that
  is after the equaliser.
- A channel assigned to several units goes through them in the order 1, 2, 3, 4.
- A unit assigned to several channels processes each channel on its own, with the values of
  the unit: a delay on decks A and B keeps two separate echoes, each under its own fader and
  on its own side of the crossfader.
- Pre-listening takes the channel after its filter, so it hears the units in insert routing
  and not those in post-fader routing.
- In external mixer mode a unit in insert routing acts on the signal of its deck before the
  deck's output; post-fader routing is not offered, and a unit in it acts as insert until the
  mode goes back to internal.
- Dry/wet mixes the input of the unit with the output of its chain. At 0 the unit is
  transparent and passes its input bit for bit.

### Effects

Each effect has three knobs and two keys in single mode, and one amount in group mode. Times
follow the tempo and the beat of the master clock (`docs/behavior/sync.md`).

| Effect | Knob 1 | Knob 2 | Knob 3 | Key 1 | Key 2 | Group amount |
| --- | --- | --- | --- | --- | --- | --- |
| Delay | Tone: a band-pass in the feedback loop, open at the left | Feedback | Time: 1/8, 1/4, 1/2, 3/4, 1, 3/2, 2 and 4 beats | Freeze: closes the input and keeps the echoes | Ping-pong: echoes alternate between left and right | Time, feedback at one half |
| Reverb | Low cut | High cut | Size | Freeze | none | Size |
| Flanger | Stereo offset | Feedback | Rate: 1/4 beat to 64 beats | Direction of the sweep | Free rate, 1/30 s to 30 s | Rate, in beats |
| Phaser | Stereo offset | Feedback | Rate, as the flanger | Direction of the sweep | Free rate | Rate, in beats |
| Filter | Low cut | Resonance | High cut | Band reject | One knob: knob 3 becomes low-pass to the left and high-pass to the right | One knob |
| Gater | Noise | Shape: from a short click to a long decay | Rate: 1, 1/2, 1/4 and 1/8 beat | Mute: noise alone | Stutter: a gate of 3/4 beat | Rate |
| Slicer | Mix of the repeated slice | Rotation, in eighth notes | Slice length: 1/8 beat to one bar | Reverse | Re-sync on each bar | Slice length |
| Bitcrusher | Bit depth: 16 bits to 1 | Sample rate: the stream rate to 100 Hz | Stereo offset | Smoothing of the reduced rate | none | Bit depth and rate together |

- Switching an effect on fades it in over 10 ms; switching it off closes its input, and the
  delay and the reverb let their tails ring out while the others fade out over 10 ms.
- The slicer captures one bar when it is switched on, from the beat of the master clock, and
  keeps repeating from it until it is switched off; switching it off and on captures again.
- Knob changes follow the 10 ms ramp of the mixer. A change of delay time crossfades from the
  old time to the new one, so it does not click.

### Mixer effect

- `mixer.effect.select` (`deck`, `effect`) chooses the effect of the channel's mixer effect
  knob; `mixer.effect.set` (`deck`, `amount` 0 to 1) turns it. At 0 the knob does nothing;
  turning it raises the amount of the effect.
- The knob sits in the channel under the filter knob, with the name of its effect under it,
  as in the M4 screen. Clicking the name opens the list of effects.
- The catalogue of M4: delay, reverb, gater and bitcrusher, each driven as its group amount
  with dry/wet following the knob. A channel starts on the reverb. There is no filter in it:
  each channel has its own.
- The mixer effect sits after the filter and before the fader.

## Differences

- **The return of the send is outputs 5 and 6.** The manual returns the send to an output of
  the interface without saying which; Platter takes the pair after those of the decks in
  external mixer mode, so the device needs six outputs. A return with no limiter of its own:
  it carries what the external mixer sent, through the effects.
- **Units 3 and 4** share the panels of units 1 and 2 through a switch in the panel header,
  as the manual shares them.
- **No pattern mode**: it is the pattern sequencer of M5.
- **The effects are Platter's own**, with the purpose of those in the manual and their own
  controls and ranges: the time values of the delay are in beats and include two beats, the
  reverb's second key and the bitcrusher's second key are unused, and the gater's rate stops
  at 1/8 beat.
- **No list of preselected effects** per unit for now: every selector offers all eight.
- **The mode of each unit is chosen in its panel only**, not also on the preferences page,
  as the public manual allows; the Mixer page has the other effect settings of the manual's
  Effects page.
- **The mixer effect has no on key**: the M4 screen draws none, and the knob at zero is off.
- In external mixer mode the mixer effect knob and its name collapse with the channel, as the
  filter does. The assign keys stay: a unit in insert routing still acts on the deck.

## Decided

The maintainer approved these on 18 November 2026; `docs/design/DESIGN.md` records them under
*Four-deck layout (M4)* and *Effects*:

1. The mixer effect catalogue has no filter, since every channel has one; a channel starts on
   the reverb.
2. The mixer effect knob is one-sided, 0 to 1, off at 0.
3. Single mode takes the panel of group mode: the selector and the dry/wet knob, three
   knobs, two keys and the reset key.
4. The effect that repeats slices of a bar is named Slicer.
5. A preference shows two or four units.
6. Send routing came with the input routing of M5; its return is outputs 5 and 6.

## Edge cases

| Input | Result |
| --- | --- |
| A unit, a slot, a knob or a key out of range | The command is rejected; nothing changes |
| An amount, a dry/wet or a knob value that is not a number, or outside 0 to 1 | Rejected |
| An effect switched on in a unit no channel is assigned to | Its state changes; nothing is heard |
| *Keep values* on, and an effect chosen in a slot or a unit that held none | The amount or the knobs stay where they were; for an empty unit those are the neutral values of no effect |
| A channel taken out of a unit while an effect rings | The tail of that channel stops with a 10 ms fade |
| The master clock changes tempo during a delay | The echoes move to the new time with a crossfade |
| The engine restarts after a change of sample rate or device | Units, modes, effects, values and assignments come back as they were; tails do not |
| Every effect of every unit assigned to every channel | The callback keeps to its time budget; measured in the tests |
| A sample that is not a finite number reaches an effect | The effect is cleared and passes silence until its input is finite again |

## Tests

- `crates/dsp/tests/effects.rs`: every effect stays finite and bounded while its controls
  sweep; a chain at dry/wet 0 passes its input bit for bit; the delay echoes half a beat later
  at 120 beats per minute; a delay rings on after its slot switches off and a flanger does
  not; an effect switched off passes its input exactly; a value that is not a number gives
  silence; the gater opens on every step; the middle of the one-knob filter leaves the signal
  alone; the bitcrusher at one bit; the slicer repeats its slice; a swapped effect fades out
  and comes back to be freed; single mode; the mixer effect at zero.
- `crates/engine/tests/effects.rs`: a unit acts only on the channels assigned to it; a delay
  after the fader rings on when the fader closes and an insert does not; an engine at another
  rate keeps the effects and their assignments; commands out of range change nothing; a new
  effect keeps the values only when *Keep values* is on.
- `crates/engine/tests/realtime_memory.rs`: effects built, swapped and switched while the
  decks play, without an allocation in the callback.
