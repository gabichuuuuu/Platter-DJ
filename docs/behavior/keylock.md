# Key lock

Key lock keeps the key of a track while its tempo changes, so a deck can be sped up or synced
without its pitch going up with it. This note covers the key lock of a deck. Transposition by
semitones, the key knob of the mixer channel and the display of the resulting key come later
in M3, on top of it. The choice of the stretch behind it is in
`docs/decisions/0021-key-lock-stretch.md`.

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Overview*, section *Track deck*, the key lock controls
- *Overview*, the mixer channel, its key lock button and key knob
- *Basic usage tutorials*, section *Working with Key Lock*

A deck has a key lock control that locks the original key of the track; the same place
transposes it by semitones, and the manual advises against more than two semitones for the
sake of the sound. The key lock button of the mixer channel instead locks the key the track
has at its current speed, and its key knob then fine-tunes that key without changing the
tempo. Turning key lock off and on again brings back the original key. The deck header can
show the resulting key, the nearest key with how many cents it is off, with key lock on or
off. Without key lock, pitch follows speed. The manual does not say what key lock does while
scratching, at a jump or at the ends of the tempo range.

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

## Differences

- The reference manual has key lock on the deck and on the mixer channel. Platter has the
  key lock of the deck, which keeps the original key. The lock of the channel, which keeps the
  key of the current speed, comes with its key knob and transposition.
- The reference manual does not say what is heard at a jump with key lock on. Platter plays the
  first 32 ms after it with the pitch following the speed, because the stretch needs about
  5 ms of work before its first sound and one callback cannot hold that beside the other
  deck; the work is spread over those 32 ms. Each pass of a loop is such a jump, so a loop
  shorter than 32 ms plays without key lock, and on a loop of one beat at 128 BPM the bridge
  is about 8 % of each pass.
- The stretch takes tracks from 32 to 192 kHz only.
- The reference manual does not say whether key lock keeps the tempo or the pitch exact when
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

## Tests

`crates/engine/src/rt/lock.rs` for the bridge, the crossfades and the speeds out of range,
on a stand-in stretch; `crates/engine/tests/keylock.rs` with the stretch the engine ships, on
synthesised sines: the pitch with key lock on and off, the tempo through the position, sync of
a key-locked deck, a seek, a beatjump, jumps, loops and a hotcue without a click, the toggle,
a 44.1 kHz track from 0.5 to 1.6 times its speed, the next track, a new device rate and a
22.05 kHz track. `crates/engine/tests/realtime_memory.rs` runs the callback with key lock on
both decks through its jumps, loops and sync, then with key lock held long enough to check its
pitch.
