# Automatic mixing with the fade cues

A deck that plays across a fade-out cue starts the next track on another deck, from its
fade-in cue.

## What the public manual says

Sections of the public manual, read on 20 November 2026:

- *Advanced usage tutorials*, section *Working with cue points*: the cue point types and
  *Setting Fade Out and Fade In cue points in tracks for automated mixing*
- *Preferences*, *Loading* page, the option that activates the fade markers, and *Mixer page*,
  the time of the automatic crossfade
- *Overview*, section *Crossfade controls*
- *Advanced operation*, the section on the automatic playback of a playlist

The fade-out and fade-in types of a cue (`docs/behavior/cues.md`) serve the automated start of
tracks: one track needs a fade-out cue and another a fade-in cue. When the playback of one
deck crosses its fade-out cue, the other deck starts playing from its fade-in cue; a fade-in
cue triggers nothing by itself. A preference of the Loading page has to be switched on for
the markers to act, and describes them as the way to crossfade automatically between tracks.

A mode of automatic playback plays the tracks of a playlist or of the collection one after
the other: it loads the next track into the opposite deck and starts it a few seconds before
the playing one ends, with the crossfader in the middle. Fade cues can shape its transitions.

**Not stated.** Whether the crossfader moves when a fade-out cue starts the other deck, and if
so in what time; whether the other deck has to be stopped; what happens when it has no track
or no fade-in cue; which deck is the other one with four decks; how sync takes part.

## What Platter does

- *Fade cues*, a key of the *Crossfader* section of the Mixer page
  (`global.fade_markers.toggle`, off to begin with, kept in the settings file under `[mixer]`
  as `fade_markers`), switches the fade cues on for every deck.
- While it is on, a playing deck whose play position moves forward across one of its
  fade-out cues starts its partner: deck A starts deck B and B starts A; C starts D and D
  starts C.
- The partner starts only if it holds a track, is not loading one, is paused and has a
  fade-in cue. It seeks to its first fade-in cue, in the order of the slots, and plays
  (`deck.seek` and `deck.play`, with the fade of a few milliseconds of every start).
- If the two decks are on opposite sides of the crossfader, the crossfader then fades to the
  side of the partner, as the *Auto* key does (`mixer.crossfader.auto_fade`), in the length of
  the automatic fade of the Mixer page. A deck on neither side moves nothing.
- The deck that crossed its fade-out cue plays on; the DJ stops it or lets it end.
- The crossing is seen each time the engine is polled, which the window does at every frame:
  the partner starts within a frame of the cue, not on its exact sample.

## Differences

- **No automatic playback of a playlist.** Platter does not load or start the tracks of a
  playlist by itself; a fade-out cue starts only a track the DJ has already loaded.
- **The crossfader moves.** The manual does not say whether it does; Platter fades it, so that
  a fade-out cue makes a whole transition on its own.
- **The preference is on the Mixer page**, next to the length of the automatic fade it uses,
  not on a page of loading options.
- **Pairs of decks.** The manual names the other deck only with two decks; with four, Platter
  keeps the pairs A and B, C and D.

## Edge cases

| Input | Result |
| --- | --- |
| The partner is playing already | Nothing changes, not even the crossfader |
| The partner has no track, is loading one, or has no fade-in cue | Nothing happens |
| A seek, a jump or a hotcue moves the play position forward across the fade-out cue | Not playback: nothing happens. A step of more than one second between two polls counts as a jump |
| Reverse or a loop moves the play position back before the cue | Nothing happens; when playback crosses the cue forward again, the partner starts if it is paused |
| Several fade-out cues | Each crossing starts the partner, if it is paused at that moment |
| The command queue is full at the crossing | The crossing is tried again at the next poll |

## Tests

- `crates/engine/tests/auto_mix.rs`: crossing a fade-out cue starts the partner from its
  fade-in cue and fades the crossfader towards it; nothing starts without the preference,
  without a fade-in cue or on a jump; a partner already playing is left alone.
- `crates/ui-egui/tests/settings_mixer.rs`: the key of the Mixer page emits its command.
- The settings round trips in `apps/platter` (`config`).
