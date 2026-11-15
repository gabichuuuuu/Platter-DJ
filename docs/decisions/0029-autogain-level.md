# 0029. Autogain: integrated loudness of ITU-R BS.1770 and a level of -10 LUFS

Status: accepted (M3).

## Context

The reference manual says autogain levels the signals of the decks, is on by default and
stores a value per track that can be restored (`docs/behavior/autogain.md`). It says neither
how the value is found nor what level it aims at. Platter has to choose a measure of how loud
a track is, a level to bring every track to, and how far the gain may move.

## Options

For the measure:

1. The peak of the track: cheap, but a track with one loud transient would be turned down
   for all its length, and two tracks with the same peak can differ in loudness by more than
   10 dB.
2. The RMS of the whole track: closer to what is heard, but bass weighs as much as the
   frequencies the ear is most sensitive to, and silence or a long quiet intro pulls the
   value down.
3. The integrated loudness of ITU-R BS.1770-5 (<https://www.itu.int/rec/R-REC-BS.1770>): a
   frequency weighting close to hearing, and two gates that leave out silence and quiet
   passages. It is a public recommendation, the base of the broadcast and streaming norms,
   and fits in a hundred lines.

For the level, a target in LUFS, the gain being the target minus the loudness of the track.

## Choice

- Option 3, written by the project in `crates/analysis/src/loudness.rs`. The two stages of
  the K filter are the coefficients the recommendation gives at 48 kHz, carried to the rate
  of the track by a bilinear map, so no third-party code or table is used. At 44.1, 96 and
  192 kHz the response stays within 0.05 dB of the one at 48 kHz from 20 Hz to 10 kHz.
- Both channels weigh one, as the left and the right channel do in the recommendation.
- Target -10 LUFS. Dance music for clubs is usually mastered louder than the -14 to -16 LUFS
  that streaming services normalise to; a target near the loud end turns most of those
  tracks down by a few decibels and brings quiet ones up, instead of turning everything down
  by 6 to 10 dB and leaving the DJ to raise the master. The target is a constant
  (`AUTOGAIN_LUFS` in `crates/engine/src/analyser.rs`), to revisit after listening.
- The gain is kept within ±12 dB, the range of the gain knob upwards: a near-silent track is
  not raised to the level of the others, and the knob keeps room to move.
- The value is the gain of the channel itself, not a stage beside it, so the gain knob shows
  it, the DJ moves it from there, and restoring it is setting the knob again.

## How it was checked

`cargo test -p platter-analysis` and `cargo test -p platter-engine --test autogain` on
Windows x64, with synthesised signals:

- The stages at 48 kHz equal the coefficients of the recommendation to within 1e-12, and the
  filter adds 0.691 dB at 997 Hz, the constant the formula takes away.
- A full-scale sine at 997 Hz in one channel reads -3.01 LUFS, the value the recommendation
  gives, at 44.1, 48 and 96 kHz; in both channels, 0 LUFS.
- Silence after a passage, a passage 20 LU below and one 5 LU below give what the gates
  define.
- A track at -20 LUFS gets +10 dB and is heard at -10 dBFS on the master; one at -1 LUFS
  gets -9 dB.

No real track was measured: the level is not yet checked by ear, and how long the measure
adds to the analysis of a long track was not timed.

## Pending

Listening with real tracks (`ROADMAP.md`, checks waiting for equipment or material): whether
-10 LUFS sits well against the master and the limiter, and whether tracks of different styles
sound equally loud after it.
