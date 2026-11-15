# Key lock

Key lock keeps the key of a track while its tempo changes, so a deck can be sped up or synced
without its pitch going up with it. This note covers the key lock of a deck and, on top of
it, transposition by semitones, locking the key of the current speed and the resulting key.
The choice of the stretch behind it is in `docs/decisions/0021-key-lock-stretch.md`; the
choices of transposition in `docs/decisions/0031-transposition.md`.

## What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Overview*, section *Track deck*, the key lock controls
- *Overview*, the mixer channel, its key lock button and key knob
- *Basic usage tutorials*, section *Working with Key Lock*, subsections *Transposing the
  Original Key in Semitones*, *Locking a Specific Key*, *Resetting the Changed Key to the
  Original Key* and *Displaying Resulting Key in the Deck Header*

A deck has a key lock control that locks the original key of the track; the same place
transposes it by semitones, and the manual advises against more than two semitones for the
sake of the sound. The key lock button of the mixer channel instead locks the key the track
has at its current speed, and its key knob then fine-tunes that key without changing the
tempo. Turning key lock off and on again brings back the original key. The deck header can
show the resulting key, the nearest key with how many cents it is off, with key lock on or
off. Without key lock, pitch follows speed. The manual does not say what key lock does while
scratching, at a jump or at the ends of the tempo range.

On transposition the tutorial adds: semitone up and down buttons beside the key lock button of
the deck transpose the locked original key, and the key lock button shows the offset. Locking
the key of the current speed (Persist Key) moves the key knob to the matching value, and the
key stays when the tempo changes afterwards. Turning the key lock of the deck off and on again
returns to the original key and puts the key knob back to its centre. The manual gives neither
the range of the knob nor the size of a step of the buttons.

## What Platter does

- Each deck has key lock, off at start. `deck.keylock.toggle` turns it on and off, on a deck
  with or without a track, playing or not. The snapshot of the deck says whether it is on.
- With key lock on, everything that changes the speed of the deck changes its tempo and keeps
  its pitch: the tempo fader, the bend buttons, sync and the pull of sync back to the beat.
- The tempo is the one asked for, as without key lock: a synced deck stays on the beat of the
  tempo master. The pitch moves by at most a few cents to keep it so, less than an ear tells
  apart.
- The position of the deck is the position of what is heard. Sync, the phase meter, cues,
  loops and the waveform read it as they do without key lock.
- Play, pause, the cue button, hotcues, beatjumps and loops work as without key lock. Just
  after a jump, a play from pause, a pass of a loop or turning key lock on, the deck plays
  about 32 ms with the pitch following the speed while the stretch gets ready, then
  crossfades over 5 ms into the locked sound. At a speed of exactly 1 the two sound the
  same.
- A track below 32 kHz has no stretch: it plays with the pitch following the speed while
  key lock stays on for the next track.
- Turning key lock on or off while the deck plays crossfades from one sound to the other.
- Below 0.04 times or above 3.9 times the original speed the stretch does not play; the deck
  plays with the pitch following the speed there and takes key lock back when the speed comes
  into the range. The tempo fader and sync stay well inside it.

### Transposition

- Each deck has a shift of its key, in semitones, from -12 to +12; 0 at start. It acts only
  through key lock: the stretch plays the tempo asked for at the pitch of the original key
  moved by the shift.
- `deck.key.step` with `direction = "up"` or `"down"` moves the shift by one semitone, up to
  the ends of the range. `deck.key.set` with `semitones` sets it, fractions included, for the
  key knob. `deck.key.persist` sets it to the key the deck has at its current speed,
  12 * log2(speed), within the range, so the key heard stays while the tempo changes
  afterwards. All three turn key lock on when it is off.
- Turning key lock off puts the shift back to 0: turned on again, the deck plays its original
  key.
- A change of the shift while the stretch plays glides to the new key: measured from 0 to
  +12 semitones, within 1 % of the new pitch in about 300 ms. It does not click. The position
  of the deck stays that of what is heard; during the glide it may be off by up to the
  distance the stretch reports for the whole change, 2.6 ms of the track for a semitone at a
  tempo of 1.08 (`docs/decisions/0031-transposition.md`).
- After a jump, a play from pause or a pass of a loop the stretch starts at the shift at
  once, after the 32 ms bridge.
- The shift stays for the next track and when the engine is built again for another sample
  rate, as key lock does.
- A track below 32 kHz has no stretch, so no shift: it plays with the pitch following the
  speed, and its resulting key says so.

### The resulting key

The snapshot of a deck gives its shift, and the resulting key of its track: the key the
analysis found, moved by what the deck plays, as the nearest key and the cents it is off,
from -50 to +50. What the deck plays is the shift while key lock acts on a stretch, and the
pitch of the speed, 12 * log2(speed), otherwise: without key lock, on a track below 32 kHz,
in reverse and at a speed out of the range of the stretch. A track whose key the analysis did not find
has no resulting key.

## Differences

- The public manual has a key lock on the deck and another on the mixer channel. Platter
  has one key lock per deck with a shift in semitones: the key lock of the channel is
  `deck.key.persist`, which turns that key lock on with the shift of the current speed, and the
  key knob is `deck.key.set`. The buttons and the knob are not in the window yet: the commands
  work from the keyboard file, mappings and scripts.
- The range of ±12 semitones and the step of one semitone are Platter's choices; the manual
  states neither.
- The 32 ms bridge after a jump plays with the pitch following the speed, without the shift:
  with a deck transposed by two semitones, every jump and every pass of a loop starts with
  32 ms at the untransposed pitch before the crossfade into the transposed sound.
- The public manual does not say what is heard at a jump with key lock on. Platter plays the
  first 32 ms after it with the pitch following the speed, because the stretch needs about
  5 ms of work before its first sound and one callback cannot hold that beside the other
  deck; the work is spread over those 32 ms. Each pass of a loop is such a jump, so a loop
  shorter than 32 ms plays without key lock, and on a loop of one beat at 128 BPM the bridge
  is about 8 % of each pass.
- The stretch takes tracks from 32 to 192 kHz only.
- The public manual does not say whether key lock keeps the tempo or the pitch exact when
  both cannot be. Platter keeps the tempo, so sync holds.
- The stretch softens the attack of a kick and raises its peak: measured on a synthesised
  kick, its rise takes about 4 ms instead of 0.5 ms and its peak is about 1.3 dB higher. An own
  stretch that keeps the attack is left for later.

## Edge cases

| Input | Result |
| --- | --- |
| `deck.keylock.toggle` on an empty deck | Key lock changes; the next track plays with it |
| A new track loaded with key lock on | Key lock stays on |
| Key lock on at a speed of 1 | The track sounds as without key lock |
| The tempo fader moved while key lock is on | The tempo follows within about 12 ms; the pitch does not move |
| A loop shorter than 32 ms | The loop plays with the pitch following the speed |
| A track at 22.05 kHz with key lock on | It plays with the pitch following the speed |
| The end of the track | The deck stops at the end, as without key lock |
| A change of sample rate of the audio device | Key lock of each deck is kept |
| `deck.key.step` up on a deck without key lock | Key lock turns on, transposed one semitone up |
| `deck.key.step` up at +12 | The shift stays at +12 |
| `deck.key.set` with 13 | Refused: out of range |
| `deck.key.persist` at a speed of 1.06 | Key lock on with a shift of about +1.01 semitones |
| `deck.key.persist` at a speed of 2.5 | Key lock on with a shift of +12 |
| Key lock off and on after a shift | The original key |
| A shift on a track at 22.05 kHz | The pitch follows the speed; the resulting key shows it |
| A shift on a deck at a speed out of the range of the stretch | The pitch follows the speed until the speed is back in range |

## Tests

`crates/engine/src/rt/lock.rs` for the bridge, the crossfades and the speeds out of range,
on a stand-in stretch; `crates/engine/tests/keylock.rs` with the stretch the engine ships, on
synthesised sines: the pitch with key lock on and off, the tempo through the position, sync of
a key-locked deck, a seek, a beatjump, jumps, loops and a hotcue without a click, the toggle,
a 44.1 kHz track from 0.5 to 1.6 times its speed, the next track, a new device rate and a
22.05 kHz track. `crates/engine/tests/transposition.rs`, on synthesised sines: the pitch of a
shift of 1, -2, +12 and -12 semitones at tempos 1 and 1.08, the position with and without a
shift, a shift changed while playing, the position against a gap heard in the track, a synced
deck with a shift, a step without a click, key lock off and on, the range of the commands,
persist, a 44.1 kHz track, a 22.05 kHz track, the next track and a new device rate, and the
resulting key. `crates/engine/tests/realtime_memory.rs` runs the callback with key lock on
both decks through its jumps, loops and sync, a step, a set and a persist of the key, then
with key lock held long enough to check its pitch.
