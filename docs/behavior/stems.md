# Stems

A stem deck plays a track as four parts, the stems, that sound together as the whole track: drums,
bass, other and vocals. Each stem has its own volume, mute, filter and effect send. This note
covers the stem deck, the controls of each stem and the mute buttons; the separation of a track
into stems has its section below, *Separation*.

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
  four audio files of the folder the explorer shows whose names, without the extension, are
  `drums`, `bass`, `other` and `vocals`, in any case (`browser.stems.attach`). This is the layout
  separation tools commonly write. A folder that lacks one of them, or has two files for one,
  attaches nothing and the log says why. *Remove the stems* (`browser.stems.remove`) forgets the
  link and deletes no file: Platter did not write them. A track with stems shows the word
  *Stems* at the end of its title in the list.
- **Loading.** `deck.load` loads the stems the collection links to the track, so a track with
  stems loads as a stem deck whichever way it is loaded: the load keys, a drop or a mapping.
  For such a track the context menu has *Load without stems on <deck>* for each deck shown
  (`browser.load_selected_track`, which sends `deck.load_track`). `deck.load_stems` loads a track
  with four files given, for the command line and remote control. The four files are
  decoded with the track on the load worker. A stem at another sample rate is brought to that
  of the track, with cubic interpolation; a shorter one is completed with silence and a longer
  one is cut at the length of the track. When a stem cannot be read, the track loads alone and
  the deck says why.
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
  of the waveform and the stripe, each with the colour and the name of its stem, the mute key
  `M`, the volume bar and the send key `FX`, and the scrolling waveform of the stem
  (`docs/design/screens/M4-effects.html`); the header tags the deck *Stems*. The mute keys are
  always visible. The context menu of the send key sets the amount of the send in steps of a
  quarter, turns the filter on or off and sets it to five positions; any amount and position
  are reached through the commands. A muted stem lights its mute key in its colour and dims
  its volume bar.

## Separation

What the public manual says, from the sections read on 21 November 2026 (*Generating and deleting
Stem files*): a track is separated from its context menu (*Generate Stems*); the progress shows in
the status bar of the browser and the job can be cancelled; it takes from a third to two thirds
of the playing time of the track, depending on the processor, and can run while the track plays.
The stems are kept apart from the collection, linked to their track; *Delete Associated Stems*
removes their file. The preferences choose the folder of the stems.

What Platter does (choices: `docs/decisions/0038-stem-separation.md`):

- **The model.** Platter separates with its own code, in the crate `platter-stems`: the short-time
  transform, the networks and the masks. The trained weights are not Platter's: they are those of
  the 4-stem Spleeter model of Deezer, release v1.4.0, which the user downloads from its original
  publication (<https://github.com/deezer/spleeter/releases/tag/v1.4.0>, file `4stems.tar.gz`,
  146 MB). Platter neither hosts nor packages them, and says why: the paper of its authors states
  the MIT licence for the models (<https://doi.org/10.21105/joss.02154>), their repository states it
  for the code only, and the publisher has not confirmed it (`docs/legal-notes.md`). Platter opens
  no connection for it: the user downloads the file with the browser.
- **Installing it.** With the folder of the downloaded file in the explorer, the context menu of a
  track of the collection has *Install the separation model from <folder>*
  (`stems.model.install`). Platter checks `4stems.tar.gz` of that folder against the SHA-256 the
  release publishes for it and refuses any other file, then reads its weights into a file of its
  own, `Models/separation.weights` in the folder of the settings, so that a model trained later
  for Platter can take its place. The downloaded file can then be deleted. The log says when the
  model is installed, or why not.
- **Separating.** *Separate into stems* in the context menu of a track of the collection, or
  `browser.stems.generate`, separates it on a worker: the track is decoded, brought to 44.1 kHz,
  the rate the model works at, separated into drums, bass, other and vocals, and written as four
  WAV files of 16 bits at 44.1 kHz into `Stems` in the folder of the settings, a folder per
  track; the four are then attached to the track as stems a user attaches. One track is separated
  at a time, on half the processors, so the audio and the window keep the rest; the status bar
  shows the progress, and the menu of a track stops it.
- **The result.** The model works on the first 11 kHz of the spectrum: what is above goes to no
  stem (as the model does), so the four stems played together sound like the track without its
  highest frequencies. The quality is that of the model: about 6 dB of signal to distortion on
  vocals and drums and 4 to 5 dB on bass and other on the test set of its authors.

## Differences

- Platter does not write or read a container of its own for stems: a set is four files, attached
  by the user or written by the separation. They are WAV files of 16 bits, larger than a
  compressed container.
- The separation model is downloaded by the user, not by Platter: the licence of its weights is
  not confirmed by their publisher.
- What a stem does not send skips the mixer effect too, not only the effect units: the channel
  has one path after the effects, and keeping a second path through the mixer effect would need a
  second instance of it.
- The filter and the send amount have no knob of their own in the lane: the layout gives the lane
  the mute key, the volume and the send key.
- The stem lanes take the place of the stripe of the whole track in the four-deck layout, as the
  layout draws them; the manual keeps the stripe.
- The two-deck layout has no stem lanes yet: its design does not place them. A stem deck plays
  there and its stems are driven through commands, the keyboard and MIDI.

## Edge cases

- A stem shorter than the track is silent past its end; one longer is cut.
- A stem at another sample rate is brought to that of the track; one that cannot be read leaves
  the track playing alone.
- `deck.stem.*` with a stem name that is not `drums`, `bass`, `other` or `vocals` is refused.
