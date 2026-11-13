# Transport

The inventory places the transport controls in M2, with the interface. M1 delivers the engine
commands behind them; this note covers those commands and is completed in M2.

## What the public manual says

Sections of the reference manual:

- *Overview*, section *Transport controls*
- *Overview*, section *Synchronization and tempo controls*

Play/Pause starts and pauses a deck. The cue button works on a floating cue point: depending
on whether the deck is playing, it sets the point or returns to it. A tempo fader changes the
playback speed within a range chosen in the preferences, up to a range that reaches a full
stop. Two bend buttons slow down or speed up the deck slightly while held, to correct the
offset between two tracks by hand.

## What Platter does in M1

| Command | Effect |
| --- | --- |
| `deck.load` | Decodes a file on a worker thread and puts it in the deck, paused at its start, with the cue point at the start |
| `deck.eject` | Empties the deck |
| `deck.play`, `deck.pause`, `deck.play_pause` | Start, pause, toggle |
| `deck.cue.set` | Stores the current position as the cue point |
| `deck.cue.goto` | Returns to the cue point and pauses there |
| `deck.seek` | Jumps to a position in seconds; past the end lands on the end |
| `deck.tempo.set` | Speed as a ratio of the original, 0 (full stop) to 2 |
| `deck.nudge` | Temporary speed offset as a fraction of the tempo, -0.5 to 0.5; 0 releases it |

- Play, pause, seek, eject and a track change apply a 5 ms fade, so none of them cuts the
  waveform. A seek while playing fades out, jumps and fades in.
- Tempo and nudge follow a 20 ms ramp.
- There is no key lock until M3: pitch follows speed.
- The deck stops by itself at the end of the track and stays there.
- Commands sent right after a load apply to the track that arrives: load, seek and play in a
  row start the new track at that position.

## Differences

- The reference product has one cue button whose action depends on the playback state.
  The engine exposes the two actions as two commands; the button of M2 chooses between them.
- Tempo is a ratio. Ranges in per cent are a matter of the fader in M2.
- CUP (play from the cue point while held) arrives in M2.

## Edge cases

| Input | Result |
| --- | --- |
| Play, cue or seek on an empty deck | Nothing happens |
| Seek to a negative time, beyond 24 hours, or not a number | The command is rejected |
| Tempo outside 0 to 2, nudge outside -0.5 to 0.5 | The command is rejected |
| A file that cannot be decoded | The deck keeps what it had; a load-failed event carries the reason |
| A second load before the first finishes | The first result is discarded |

After a pause the output is below -100 dB within 20 ms and exact silence within 300 ms: the
equaliser filters ring down after the fade.

## Tests

`crates/engine/tests/transport.rs`.
