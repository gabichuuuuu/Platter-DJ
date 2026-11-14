# Manual check of milestone M2

The gate of M2 has a part no test can do: listening. This list is run by the maintainer on a
real machine with a real audio device. Nothing here is automated, and none of it has been
checked by anyone until this file says so at the bottom.

Start with `cargo run --release -p platter`. The log is `platter.log` in the folder of the
files of the user (`docs/behavior/settings.md`).

## 1. Start-up

- [ ] The window opens. The status header shows the kind of audio device, 48 kHz, a latency
      and a load below 10 % at rest.
- [ ] `platter.log` has one line for the start and one for the audio device, and no line that
      starts with `error`.
- [ ] With the audio device unplugged or disabled before starting, the window still opens, the
      header says there is no audio device and the notice under it has a key that opens the
      audio preferences. Plugging the device back in clears the notice within a few seconds.

## 2. Loading two tracks

- [ ] The file selector shows the home folder. Opening folders in the tree and in the list
      works with the pointer and with the keyboard.
- [ ] A folder with many audio files shows their names at once and fills in title, artist and
      the rest while the window stays responsive.
- [ ] Load A, Load B, Shift with the left and right arrows, and dragging a row onto a deck
      each load the track. The deck shows "Reading file…" and then title, artist, album, times
      and both waveforms. The interface does not freeze while a long file loads.
- [ ] A file dropped from the file manager of the system loads into the deck it is dropped on.
- [ ] A file that is not audio, renamed to `.mp3`, shows the message of the deck and leaves
      the deck as it was.
- [ ] One file of each format plays: MP3, M4A, WAV, AIFF, FLAC and Ogg Vorbis.

## 3. Playing

- [ ] Play starts the track without a click; pause stops it without a click.
- [ ] The scrolling waveform moves smoothly and the playhead of the overview follows.
- [ ] A click on the overview jumps there, playing and paused, without a click.
- [ ] CUE on a paused deck away from the cue point sets it; held at the cue point it plays
      until released and returns; on a playing deck it returns to the cue point and pauses.
- [ ] CUP returns to the cue point and plays when released.
- [ ] The tempo fader changes speed and pitch together. The range key steps through 8, 16 and
      50 %. The two bend keys slow down and speed up only while held.
- [ ] The zoom keys change how much of the track the lane shows.
- [ ] At the end of a track the deck stops and the remaining time reads zero.

## 4. Mixing, control by control

For each channel, with a track playing:

- [ ] Gain: silent at the bottom, louder to the top; the channel meter follows.
- [ ] High, mid and low: each removes its band at the bottom and boosts it at the top, with no
      zipper noise while turning. Each Kill key removes its band and gives it back.
- [ ] Filter: low-pass to the left, high-pass to the right, nothing at the centre.
- [ ] Pan: the far side fades out; the centre is unchanged.
- [ ] Fader: silent at the bottom, unchanged at the long tick, louder above it.
- [ ] Crossfader: deck A alone at the left, deck B alone at the right. Each of the three
      curves in the preferences changes how the middle sounds.
- [ ] Master level: silent at the bottom. With both channels and the master turned up the
      output gets louder and then stops getting louder instead of distorting harshly; the
      master meter shows CLIP only if the mix passes full scale before the limiter acts.
- [ ] Every knob returns to its default with a double click and with Home.

## 5. Preferences

- [ ] Changing the audio device, the buffer size and the routing each restart the audio after
      a short silence, and the tracks stay loaded.
- [ ] Changing the sample rate reloads the tracks; they play at the right pitch at 44.1, 48
      and 96 kHz.
- [ ] The test tone on the master is heard on both sides.
- [ ] With a device of four outputs: the test tone on the headphones is heard there and not
      on the master; a pre-listened deck is heard in the headphones with its fader down; Mix
      and Vol do what they say.
- [ ] Changing the language changes every text at once, and no text is cut in Spanish.
- [ ] Open keymap.toml opens the file. A changed shortcut works after a restart; a file with
      a mistake is reported in the log and the default shortcuts work.
- [ ] Settings are still there after a restart.

## 6. Keyboard

- [ ] Tab reaches every control and the focus ring is always visible.
- [ ] Every row of the table in `docs/behavior/keyboard.md` does what it says.

## 7. One hour

- [ ] Two tracks loaded, one of them looping by hand or replaced as it ends, for one hour at
      the smallest buffer the device accepts. Note the buffer the header shows, the highest
      load seen and the dropout count at the end.

On Windows the shared mode of WASAPI runs at the period of the device, 480 samples at 48 kHz
on most, whatever buffer size is asked for (`docs/decisions/0010-audio-output.md`). A run at
256 samples needs a system where the device accepts it.

## Result

Run in part on 13 November 2026, on Windows x64 with WASAPI in shared mode and buffers of 480
samples. The run was driven through the window of the application and through `platter-cli`
renders; nobody listened to it then, and the list above is not ticked.

On 14 November 2026 the maintainer listened to the application on the same system and
reported no problem. That listening is recorded as the maintainer stated it; no figure was
taken from it.

Checked by looking at the window or by measuring a render:

- All six formats decode and load; a file that is not audio shows the message of the deck
  and leaves it as it was.
- Load A and Load B with the pointer, Shift with the arrows, and dragging a row onto a deck.
- Play and pause, a click on the overview, CUE, CUP, the tempo fader (+50 % advanced the
  track about 1.6 times faster), the three ranges and the zoom keys.
- Gain: exactly -6 dB at -6 dB. Equaliser on white noise: high at zero -32 dB, mid at zero
  -16 dB, low at zero -15 dB, and a kill key equals the band at zero. Filter: low-pass and
  high-pass cut what they should. Pan: at an end the other side is silent. Fader at zero:
  silence. Crossfader at the middle: smooth 0 dB, linear -3 dB, sharp +3 dB, and at each end
  only one deck. Master: with +12 dB on the channel and on the master the peak stays at 1.00.
- Sample rate (44.1, 48 and 96 kHz) and buffer size apply with no dropout; the language
  changes at once; the settings survive a restart; Tab shows the focus ring.

Not checked by measurement: what has to be heard (clicks, zipper noise, distortion) and the
test tone, which rest on the listening of the maintainer. Not checked at all: outputs of a
four-output device, a file dropped from the file manager, an edited
`keymap.toml`, the whole table of `docs/behavior/keyboard.md` and the hour of playing.

Found and fixed, each with a test that failed before the fix:

- A fast drag from the file list carried another row than the one it started on.
- A click on a knob did not give it the keyboard, so the arrows and Home acted only after Tab.
- In Spanish the text of an empty deck needed three lines and the last one was cut. Wrapped
  text now keeps to its box and the Spanish text is shorter.
- Changing the language did not update the name of the root of the folder tree until the
  next start.
- The AAC track started 2112 samples late and lasted 30.070 s instead of 30.000 s: the edit
  list of the file was not applied. It now starts in step with the WAV of the same signal and
  lasts the 30.001 s the file declares.

Only the drag was checked again in the window. The other fixes were checked by their tests
and, for the AAC track, by a render.

| Date | System, audio API, buffer | Dropouts in the hour | Highest load | Notes |
| --- | --- | --- | --- | --- |
| 2026-11-13 | Windows x64, WASAPI shared, 480 samples | not run | 1 to 8 % at rest and playing | Partial run, see above |
| 2026-11-14 | Windows x64, WASAPI shared, 480 samples | not recorded | not recorded | Listened by the maintainer, no problem reported |
