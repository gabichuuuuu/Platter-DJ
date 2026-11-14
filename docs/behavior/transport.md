# Transport

The transport of a deck: play, pause, the cue buttons, seeking, tempo and tempo bend. M1
delivered the engine commands; M2 adds the two cue buttons, the tempo ranges and the controls
of the interface.

## What the public manual says

Sections of the reference manual, read on 13 November 2026:

- *Overview*, section *Transport controls*
- *Overview*, section *Synchronization and tempo controls*
- *Preferences*, section *Transport page*

Play/Pause starts and pauses a deck. The CUE button works on a floating cue point: depending
on the playback state it sets a new point or jumps to the one set before. The CUP button
(cue and play) jumps to the cue point and starts playback when it is released. A tempo fader
changes the playback speed within a range chosen in the preferences, from 2 % to 100 %; at
100 % the fader reaches a full stop. Two bend buttons slow down or speed up the deck slightly
while held, to correct the offset between two tracks by hand.

## What Platter does

| Command | Effect |
| --- | --- |
| `deck.load` | Decodes a file on a worker thread, computes its waveform peaks and puts it in the deck, paused at its start, with the cue point at the start |
| `deck.eject` | Empties the deck |
| `deck.play`, `deck.pause`, `deck.play_pause` | Start, pause, toggle |
| `deck.cue` | The CUE button, with `action` `press` or `release`; see below |
| `deck.cup` | The CUP button, with `action` `press` or `release`; see below |
| `deck.cue.set` | Stores the current position as the cue point |
| `deck.cue.goto` | Returns to the cue point and pauses there |
| `deck.seek` | Jumps to a position in seconds; past the end lands on the end |
| `deck.tempo.set` | Speed as a ratio of the original, 0 (full stop) to 2 |
| `deck.tempo.range.cycle` | Moves the tempo fader to its next range: 8 %, 16 %, 50 % |
| `deck.nudge` | Temporary speed offset as a fraction of the tempo, -0.5 to 0.5; 0 releases it |

- Play, pause, seek, eject and a track change apply a 5 ms fade, so none of them cuts the
  waveform. A seek while playing fades out, jumps and fades in.
- Tempo and nudge follow a 20 ms ramp.
- There is no key lock until M3: pitch follows speed.
- The deck stops by itself at the end of the track and stays there.
- Commands sent right after a load apply to the track that arrives: load, seek and play in a
  row start the new track at that position, and a cue point set in between belongs to it.

### The CUE button

| State when pressed | Press | Release |
| --- | --- | --- |
| Playing | Returns to the cue point and pauses | Nothing |
| Paused, away from the cue point | Moves the cue point to the current position | Nothing |
| Paused, at the cue point | Plays while held | Returns to the cue point and pauses |

Play pressed while the button holds a preview keeps the deck playing after the release.

### The CUP button

Pressing it returns to the cue point and holds the deck there, paused. Releasing it starts
playback from the cue point.

### Tempo fader and bend buttons

The fader of the interface covers the current range either way around the original speed and
sends `deck.tempo.set`. Changing to a narrower range pulls a tempo outside it to the edge of
the new range, so the fader and the speed never disagree. The bend buttons send `deck.nudge`
with 4 % of the tempo while held and zero on release.

## Differences

- The manual states what CUE does by playback state but not the preview while held. Platter
  follows the convention of DJ players for it, as the table above describes.
- Three tempo ranges (8 %, 16 %, 50 %), chosen on the deck, instead of a list of fourteen in
  the preferences. `deck.tempo.set` itself accepts any ratio from 0 to 2.
- The bend is a fixed 4 %; the reference product has a sensitivity setting and a progressive
  mode.
- `deck.cue.set` and `deck.cue.goto` remain as separate commands for scripts and mappings.

## Edge cases

| Input | Result |
| --- | --- |
| Play, cue, CUP or seek on an empty deck | Nothing happens |
| A CUE or CUP release without its press | Nothing happens |
| Seek to a negative time, beyond 24 hours, or not a number | The command is rejected |
| Tempo outside 0 to 2, nudge outside -0.5 to 0.5 | The command is rejected |
| A file that cannot be decoded | The deck keeps what it had; a load-failed event carries the reason |
| A second load before the first finishes | The first result is discarded |
| A preview that reaches the end of the track | The deck stops there; the release returns to the cue point |

After a pause the output is below -100 dB within 20 ms and exact silence within 300 ms: the
equaliser filters ring down after the fade.

## Tests

`crates/engine/tests/transport.rs`; the tempo range in `crates/engine/tests/mixer.rs`
(`the_tempo_range_cycles_and_pulls_the_tempo_inside`).
