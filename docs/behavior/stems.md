# Stems

A stem deck plays a track as four parts, the stems, that sound together as the whole track: drums,
bass, other and vocals. Each stem has its own volume, mute, filter and effect send. This note
covers the stem deck, the controls of each stem and the mute buttons; the separation of a track
into stems is a function of its own, still to come.

## What the public manual says

Sections of the public manual, read on 21 November 2026:

- *Overview*, *Decks and Deck Types*, *Stem Deck* and *Multi-Track Waveform*
- *Advanced usage tutorials*, *Working with Stems*: *About Stem Files*, *Generating and deleting
  Stem files*, *Loading a Stem file*, *Playing back the Stem file*, *Adjusting the Stem volumes*,
  *Applying a Filter effect to the Stems*, *Sending Stems to FX Units*
- *Preferences*, *Loading page*, *Analyze Options page* and *Decks Layout page*

The release notes of the documented application, read the same day, add the always visible
mute buttons.

A stem deck works like a track deck. Instead of one waveform it shows one lane per stem, and each
lane has a volume knob with a mute button, a filter button with its knob and an effect send
button with its knob; an arrow shows or hides those controls. The stems of a generated set are
drums and percussion, bass, other (what is left of the instruments) and vocals; a purchased set
names its own. By default the four stems play at full volume and sound like the track.

- **Volume.** The knob sets the level of the stem; a double click returns it to the top, its
  default. The mute button silences the stem and brings it back at the level it had.
- **Filter.** A button turns the filter of the stem on; its knob is a high-pass on one side of
  the middle and a low-pass on the other, and the middle filters nothing. A double click returns
  it to the middle. The manual gives no range.
- **Effect send.** By default every stem goes fully to the effect units of the channel of the
  deck, like a track. The send of a stem sets how much of it the effect units get, so that, for
  example, only the vocals reach a delay. A button turns the send on; its knob sets the amount,
  and a double click returns it to the top.
- **Mute buttons.** Since a recent version a column of mute buttons stays visible on the deck,
  so that a stem can be muted without opening its controls.
- **Stems of a track.** Generated stems are kept apart from the collection and linked to the
  track they come from; the track has one row, and a mark says it has stems. Loading such a
  track loads its stems and turns the deck into a stem deck, unless the preference of the loading
  page, a modifier or a menu entry asks for the track. Deleting the stems of a track removes their
  file and the link; when the file cannot be found, only the link. A stem deck shows the stripe
  of the whole track.

## What Platter does

- **Stems of a track.** The library links a track to four audio files, one per stem, in the
  order drums, bass, other, vocals. Until Platter separates tracks itself, the files come from
  the user: the entry *Attach the stems in <folder>* of the context menu of a track takes the
  four files of the folder the explorer shows whose names, without the extension, are `drums`,
  `bass`, `other` and `vocals`, in any case. This is the layout separation tools commonly write.
  *Remove the stems* forgets the link and deletes no file: Platter did not write them. A
  track with stems shows a mark in the list.
- **Loading.** A track with stems loads as a stem deck (`deck.load_stems`, with the track and its
  four files); *Load as track* in the context menu loads the track alone (`deck.load`). The four files are
  decoded with the track on the load worker. Each stem has to have the sample rate of the track;
  a shorter one is completed with silence and a longer one is cut at the length of the track.
  When a stem cannot be read or its rate differs, the track loads alone and the deck says why.
- **The deck.** A stem deck is a track deck: transport, cues, loops, sync, key lock, flux and the
  mixer channel act on it as on a track. The waveform, the stripe, the analysis and the tempo
  are those of the track; the stems are not analysed. Key lock stretches each stem with its own
  stretch, so the four stay together.
- **Volume** (`deck.stem.volume.set`, 0 to 1, default 1) and **mute** (`deck.stem.mute.toggle`).
  A muted stem keeps its volume and comes back at it. Both ramp in 10 ms, like the mixer.
- **Filter** (`deck.stem.filter.set`, -1 to 1, default 0, and `deck.stem.filter.toggle`, default
  off). The filter of a stem is the filter of the mixer channel, with its model: below the middle
  a low-pass, above it a high-pass. While it is off the stem is not filtered, whatever the knob;
  the knob keeps its place.
- **Effect send** (`deck.stem.send.set`, 0 to 1, default 1, and `deck.stem.send.toggle`, default
  off). While the send is off the stem goes fully to the effects, as a track does; on, the knob
  sets how much of it they get. What a stem does not send goes around every effect of the
  channel: the effect units at the insert and after the fader, and the mixer effect. The gain,
  the equaliser, the filter, the fader and the balance of the channel act on all of it.
- **Reset.** Loading a track or a set of stems puts the controls of the four stems back to their
  defaults. The controls of a deck that plays a track change nothing until stems are loaded,
  and that load resets them.
- **Lanes and mute buttons.** In the four-deck layout a stem deck shows its four lanes in the place
  of the waveform, each with the colour and the name of its stem, the mute button, the volume bar
  and the send button (`docs/design/screens/M4-effects.html`). The mute buttons are always
  visible. The filter and the send amount are set from the context menu of a lane and from
  commands.

## Differences

- Platter does not write or read a container of its own for stems yet: a set is four files the
  user attaches. Separation by Platter (`generate`) will write the same four files and attach
  them.
- What a stem does not send skips the mixer effect too, not only the effect units: the channel
  has one path after the effects, and keeping a second path through the mixer effect would need a
  second instance of it.
- The filter and the send amount have no knob of their own in the lane: the layout gives the lane
  the mute button, the volume and the send button.

## Edge cases

- A stem shorter than the track is silent past its end; one longer is cut.
- A stem at another sample rate is refused and the track loads alone.
- `deck.stem.*` with a stem name that is not `drums`, `bass`, `other` or `vocals` is refused.
