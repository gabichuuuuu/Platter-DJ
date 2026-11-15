# 0031. Transposition: one key lock per deck with a shift in semitones

Status: accepted (M3).

## Context

The public manual has two key locks: the one of the deck, which keeps the original key and
transposes it with semitone buttons, and the one of the mixer channel (Persist Key), which
keeps the key of the current speed and fine-tunes it with a key knob
(`docs/behavior/keylock.md`). It gives neither the range of the knob nor the size of a step,
and it says that turning the key lock of the deck off and on returns to the original key.

The stretch of key lock (`docs/decisions/0021-key-lock-stretch.md`) takes a pitch ratio
beside its tempo, from 0.25 to 4, with the pitch over the tempo between 0.25 and 32.

## Options

1. Two locks per deck, as the documented application shows them, each with its own state.
2. One key lock per deck with a shift in semitones. Locking the original key is a shift of 0;
   locking the key of the current speed is a shift of 12 * log2(speed); the semitone buttons
   and the key knob move the shift.

## Choice

- Option 2. The two locks of the documented application differ only in the key they keep, and
  both end in the same state: a stretch at the tempo asked for and a pitch ratio. One state
  leaves no question of which lock wins when both are pressed.
- Range ±12 semitones, an octave each way: wide enough for the knob and for a persist at the
  speeds the tempo fader reaches with its widest range (12 * log2(2) = 12), well inside the
  pitch ratio the stretch takes. Buttons move one semitone.
- The pitch ratio is 2^(shift / 12), and the stretch keeps it within its bounds against the
  tempo: at a tempo of 0.04 the pitch cannot be above 1.28, and at 3.9 not below 0.975. The
  tempo the deck asks for is kept in every case, so sync holds.
- Turning key lock off puts the shift back to 0, so turning it on again returns to the
  original key, as the manual says.
- The resulting key is computed on the control side from the key of the analysis and what the
  deck plays, so it needs no work on the audio thread.

## What the stretch needed

Two behaviours of timestretch 0.16.0 showed up in the measurement, on Windows x64 with
synthesised sines, and are handled in `crates/engine/src/stretch.rs`:

- A change of pitch while the session plays glides over about 300 ms; a seek starts the
  session again with the parameters it last applied, without a glide. Scheduled at the first
  chunk after a seek, as the tempo was before, a shift was heard gliding from the original key
  after every start, and the position the session reports drifted with it: 240 frames of the
  track behind for a shift of +2 at a tempo of 1.08, 2047 ahead for -12 at 1, against an
  unshifted deck. The stretch now applies the tempo and the pitch of the deck before a seek
  is done: it schedules them, lets the session apply them in one step without work, and
  seeks again. The position then matches an unshifted deck to within 34 frames.
- After a change of pitch while it plays, the session reports a position beyond the frame it
  plays by 2048 * tempo * (1 / new pitch - 1 / old pitch) frames of the track, 2048 being the
  frames it takes from the track at a time, until the next seek. Measured for changes from 0
  to 1, 0 to +12, 0 to -12, +2 to -2, +5 to 0 and +5 to +12 semitones at tempos 0.94, 1 and
  1.08: within 25 frames of that law, and a later change of tempo does not move it. The
  stretch takes it off the position; during the glide the position may be off by up to that
  amount.

## How it was checked

`cargo test -p platter-engine --test transposition` on Windows x64, with synthesised tracks:

- A sine at 440 Hz shifted by 1, -2, +12 and -12 semitones plays within 5 cents of the
  shifted pitch at tempos 1 and 1.08, clean by more than 40 dB, and on a 44.1 kHz track by
  ±2 semitones at 0.84 and 1.16.
- The position of a deck shifted by 2 stays within 1 ms of an unshifted one over 8 s at 1.08;
  so does a deck whose shift changes while it plays (0 to +12 at 1.08, 0 to -12 at 1, +5 to 0
  at 0.94), a second after the change.
- A tone that comes back after a gap at 3 s of the track is heard within 10 ms of the moment
  the position reaches 3 s, for -12, -2, +2 and +12 semitones at 1 and 1.08; the attack the
  stretch softens accounts for the rest. Before the seek applied the parameters, -12 at 1 was
  43 ms off.
- A synced deck shifted by 2 stays within 0.01 beats of the tempo master.
- A step of a semitone while playing steps by less than 1.5 times the steady sine.

Both changes in `stretch.rs` were taken out one at a time: each makes a test of the list fail.
`realtime_memory` runs a step, a set and a persist under the test allocator, and the command
sweep sends the three commands at random.

## Pending

Listening with real tracks (`ROADMAP.md`, checks waiting for equipment or material): the
sound of a transposed deck, the change of the shift while it plays, and the 32 ms bridge at
the untransposed pitch after a jump.
