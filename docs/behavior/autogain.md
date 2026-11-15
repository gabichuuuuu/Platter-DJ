# Autogain

Autogain sets the gain of a mixer channel from how loud the loaded track is, so that tracks
mastered at different levels play at about the same loudness and none dominates the mix. This
note covers the measurement, when the gain is set and how it is restored after the DJ turns
the gain knob. The gain knob itself is in `docs/behavior/mixer.md`; the analysis that runs on
load is in `docs/behavior/analysis.md`.

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Basic usage tutorials*, section *Optimizing gain and output levels*, subsection *Always
  level incoming audio signals*
- *Preferences*, section *Mixer page*, the autogain option of the internal and the external
  mixing modes
- *Managing your track collection*, section *Analysis dialog options*
- *Working with playlists*, section *Restore auto gain value*
- *Overview*, section *Mixer channel*

Autogain is on by default: the signals of the decks are levelled automatically, so that no
track is much louder than another. A preference turns it off, which the manual suggests with
an external mixer or a controller with a mixer of its own; the DJ then keeps the levels below
clipping by hand. The analysis of a track finds an autogain value for it; analysing the gain
again replaces the gain the track had with a new autogain value, and a locked track keeps its
value unless the analysis is told to replace locked values. The gain knob of the channel sets
the level before the fader. The gain of tracks that was changed can be restored to their
autogain value from a playlist. The manual does not say how the value is found nor what level
it aims at.

## What Platter does

| Command | Effect |
| --- | --- |
| `mixer.autogain.toggle` | Switches autogain on or off for both channels |
| `mixer.gain.restore` | Sets the gain of the channel to the autogain value of its track |

### The measurement

- The analysis of a loaded track (`docs/behavior/analysis.md`) also measures its integrated
  loudness, as ITU-R BS.1770-5 defines it (*Algorithms to measure audio programme loudness and
  true-peak audio level*, <https://www.itu.int/rec/R-REC-BS.1770>): the two channels are
  weighted by the K filter, their mean square is taken over blocks of 400 ms that overlap by
  three quarters, blocks below -70 LUFS are dropped, then blocks more than 10 LU below the
  loudness of the remaining ones, and the loudness of what is left is the result.
- The K filter is given in the recommendation for 48 kHz. At other rates it is the same filter
  carried over by a bilinear map, so its response at a given frequency stays within a fraction
  of a decibel of the one at 48 kHz.
- No loudness is found for a track shorter than one block or one whose every block is below
  -70 LUFS: silence and near silence.
- The autogain value of a track is -10 LUFS minus its loudness, kept between -12 and +12 dB:
  the gain that brings it to -10 LUFS (`docs/decisions/0029-autogain-level.md`).

### When the gain is set

- Autogain starts on.
- While it is on, the gain of a channel is set to the autogain value of the track in its deck
  when the analysis of that track finishes, as `mixer.gain.set` would set it: the knob shows
  it and it follows the 10 ms ramp of the gain. Until then the channel keeps the gain it had.
- The DJ can turn the gain afterwards; the value of the knob holds until the next track is
  analysed.
- Switching autogain on sets each channel whose track has an autogain value to that value.
  Switching it off leaves the gain where it is, and later tracks no longer change it.
- `mixer.gain.restore` sets the gain to the autogain value whether autogain is on or off.
- The snapshot shows whether autogain is on and, for each deck, the autogain value of its
  track once the analysis has found one.
- It stays on or off when the audio device changes, as the other mixer controls do.

## Differences

- The reference product keeps the autogain value of each track in its collection and restores
  it from a playlist. Platter has no collection on this branch yet: the value lives with the
  loaded track, is measured again on each load, and is restored per channel. Storing it with
  the track and the restore from the browser come with the collection.
- The analysis does not yet offer to measure the gain alone, nor to replace the value of a
  locked track: the value is always measured on load, and the grid lock does not keep it.
- The method and the level, -10 LUFS, are Platter's: the manual names neither.
- Autogain is switched on and off on the Mixer page of the preferences
  (`docs/behavior/settings.md`), which keeps the choice in the settings file. The mixer has
  no restore key yet; it comes with the next change to the mixer panel. The commands work
  from the command line and from mappings too.
- In external mixer mode autogain acts through the gain of the channel, so only while the
  gain and equaliser option of that mode is on (`docs/behavior/external-mixer.md`); the
  reference product has an autogain option of its own for that mode.

## Edge cases

| Input | Result |
| --- | --- |
| Silence, or a track shorter than 400 ms | No autogain value; the gain stays where it was |
| A track that would need more than +12 dB or less than -12 dB | The value stops at +12 or -12 dB |
| `mixer.gain.restore` on an empty deck, before the analysis finishes, or for a track without a value | Nothing happens |
| The DJ turns the gain while the track is being analysed | The analysis sets it when it finishes, as autogain is on |
| Autogain off while a track is analysed | The gain is not changed; the value is still shown and can be restored |
| A track longer than 20 minutes | Only its first 20 minutes are measured, as for tempo and key |
| A boosted track whose peaks pass full scale | Nothing clips inside the mixer; the safety limiter of the master catches what reaches it |

## Tests

`crates/analysis/src/loudness.rs` for the K filter against the coefficients of the
recommendation and for the gates; `crates/analysis/tests/loudness.rs` for the loudness of
sines and of signals with silence and quiet passages; `crates/engine/tests/autogain.rs` for
the gain set on load, the toggle, the restore, the bounds and the change of device.
