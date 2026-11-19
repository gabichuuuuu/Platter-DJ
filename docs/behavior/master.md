# Master section

## What the public manual says

Sections of the public manual:

- *Overview*, section *Master control panel*
- *Preferences*, section *Mixer page* (read again on 18 November 2026: *Internal Mixing Mode*
  and *External Mixing Mode*), and the page of the third-party maximiser that follows it
- *Basic usage tutorials*, section *Optimizing Gain and Output Levels*, read on 18 November
  2026

A main knob sets the volume of the main output. A limiter protects that output from clipping;
it can be switched off, and the level meter of the main output shows when it is working. A
click on the label under the main knob switches the limiter on or off. The Mixer page of the
preferences repeats that switch for each mixing mode, with a headroom of none, 3, 6, 9 or
12 dB and a choice of three limiter types: one that allows a slight distortion, one that
compresses less noticeably, and a third-party maximiser that raises the loudness while it
prevents clipping. The tutorial describes that maximiser: one control adds gain and so sets
where limiting starts, a menu offers two characters, a meter shows the gain reduction, and it
runs on the main, the monitor and the record outputs in internal mode, and on each deck in
external mode. The manual advises against switching the limiter off. It gives no ranges or
defaults for any of these.

## What Platter does

- `master.gain.set` sets the main volume in decibels, from silence (-90 dB or below) to
  +12 dB. The change follows a 10 ms ramp.
- A safety limiter is always in the path, last. It keeps every output sample
  inside full scale: when a frame would exceed it, the gain of both channels drops at once to
  the value that puts the peak exactly at full scale, and then recovers over about 200 ms.
  Design and trade-offs: `docs/decisions/0007-safety-limiter.md`.
- Between the main volume and the safety limiter sits the maximiser, which the Limiter key
  switches in and out: *Maximiser* below.
- The state snapshot carries the peak level of each side of the main output, the gain the
  safety limiter is applying and the gain the maximiser is applying. The interface draws the
  master meter from them, in the mixer and in the status header.
- `master.test_tone.play` plays a 440 Hz tone for one second, 18 dB below full scale, on the
  output it names: `master` (the first two outputs of the device) or `headphones` (the third
  and the fourth). It checks the routing of the audio device; the volume knobs and the drive of
  the maximiser do not affect it. On a device with two outputs the headphone tone is not played.
- In external mixer mode there is no master: the limiter, the meter and the test tone work as
  `docs/behavior/external-mixer.md` describes.

## Maximiser

Platter's own design, a look-ahead peak limiter; how it works and how it was measured:
`docs/decisions/0034-maximiser.md`.

| Command | Effect |
| --- | --- |
| `master.limiter.toggle` | The Limiter key: puts the maximiser in the path of the master, or takes it out |
| `master.maximiser.drive.set` | `db`, 0 to +12: gain before the maximiser, which raises the level and so how much is limited |
| `master.maximiser.ceiling.set` | `db`, -12 to 0: the highest sample peak the master reaches, in decibels of full scale |
| `master.maximiser.release.set` | `ms`, 10 to 1000: the time the gain takes to recover 90 % of a reduction once nothing needs it lower |

- The key starts off, with a drive of 0 dB, a ceiling of -1 dB and a release of 100 ms. The
  four are kept in the settings file (`docs/behavior/settings.md`) and survive a change of the
  sample rate of the device.
- Off, the master is exactly what it was before the maximiser existed: same samples, no delay.
- On, the master waits 1.5 ms (less one frame) while the maximiser looks at what comes, and its
  gain falls over those 1.5 ms ahead of each peak, so a peak is turned down rather than cut:
  no sample of the master passes the ceiling. Both sides share the gain, so the stereo image
  does not move. Under the ceiling with no drive, the master is the same signal 1.5 ms later,
  bit for bit. Once nothing needs it lower, the gain holds for 1.5 ms and recovers, 90 % of the
  way in the release time, and returns to exactly one.
- The ceiling is a sample peak. The peaks a converter rebuilds between samples can pass it; the
  default of -1 dB leaves room for most of them.
- The key fades over 10 ms between the master as it is and the master through the maximiser,
  so it does not click. During those 10 ms the two are 1.5 ms apart: a tone that is in
  opposite phase across 1.5 ms (about 1 kHz) dips for a moment, by 16 dB at the middle of the
  fade for a 1 kHz tone. A change of drive or ceiling follows the same 10 ms ramp; a change
  of release applies at once, since it only changes how fast the gain moves from then on.
- While the maximiser is on, the headphones take the pre-listened decks, the metronome and
  the beat ticks 1.5 ms later too, so a deck heard both pre-listened and through the master is
  not heard twice, 1.5 ms apart. The preview player is not delayed.
- The safety limiter stays after the maximiser. The music reaches it at or under the ceiling,
  so it does not act on the music; it remains for the test tone, which comes after the
  maximiser, and for rounding.
- The snapshot carries the gain the maximiser applies, one while it is off or idle.
- In external mixer mode the maximiser does not act: each pair of outputs keeps its safety
  limiter alone. The key keeps its state and takes commands, as the other controls of the
  mixer do there.

## Differences

- The Limiter key switches the maximiser, not the guard against clipping: the safety limiter
  cannot be switched off. With it off, a hot mix would clip at the converter.
- One maximiser of Platter's own design instead of three limiter types, one of them
  third-party (`docs/gaps.md`). Its drive takes the place of the gain control the manual
  describes; there is no choice of character.
- No headroom setting. The ceiling of the maximiser lowers the peak of the master instead.
- The maximiser runs on the master only. Platter has no record output yet; the headphones keep
  their safety limiter, and in external mode the decks keep theirs.
- The maximiser has no controls in the preferences yet: the design system draws none. Its
  settings are reached through the commands, by a mapping, the keyboard or the command-line
  tool, and through the settings file.
- The Limiter key is drawn in the master column of the four-deck layout, as the design system
  draws it. The two-deck layout has no place for it in the design system yet; there the
  command reaches it.
- The gain reduction is in the snapshot but no meter shows it yet.

## Edge cases

| Input | Result |
| --- | --- |
| Main volume above +12 dB, or not a number | The command is rejected; nothing changes |
| Main volume of negative infinity | Silence |
| A mix far above full scale, maximiser off | Output peak at full scale; the limiter gain reports the reduction |
| A mix far above full scale, maximiser on | Output peak at the ceiling; the safety limiter does not act |
| A sample that is not a finite number reaches the limiter or the maximiser | It is replaced by silence |
| A drive, a ceiling or a release outside its range, or not a number | The command is rejected; nothing changes |
| A single sample far above the ceiling, or a burst shorter than 1.5 ms | Turned down to the ceiling, not cut |
| The ceiling lowered while the music plays | The samples already waiting keep the ceiling they were measured against; the new one holds from the next |
| The key switched several times within 10 ms | The fade turns back from where it is |
| A device of two outputs | The maximiser acts on the master; there are no headphones to keep in time |

## Tests

`crates/dsp/src/limiter.rs`, `crates/dsp/src/maximiser.rs`, `crates/engine/tests/mixer.rs`
(`main_volume_is_in_decibels`, `the_limiter_keeps_an_overloaded_mix_inside_full_scale`),
`crates/engine/tests/maximiser.rs`, `crates/engine/tests/realtime_memory.rs` and
`crates/engine/tests/monitoring.rs`
(`the_test_tone_plays_for_a_second_on_the_output_it_is_sent_to`); the settings in
`apps/platter/src/config.rs` and the key in `crates/ui-egui/tests/panels.rs`.
