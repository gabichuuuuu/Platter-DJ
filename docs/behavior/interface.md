# Interface

The main window: its zones, the status header, the deck header and the messages the
interface shows when something is missing.

## What the public manual says

Sections of the reference manual, read on 13 November 2026:

- *Overview*, section *Header*
- *Overview*, section *Deck header*
- *Preferences*, section *Global Settings page*, read on 15 November 2026 for full screen
  and tooltips
- *Overview*, section *Header*, read again on 15 November 2026 for the MIDI and connection
  indicators

The header at the top of the window holds status indicators: one for the audio device, which
changes colour when no device is connected or when the built-in sound card is in use; a load
indicator, the share of the time of an audio buffer that processing it takes; a level meter of
the main output; and the time of day. Below it the decks sit side by side with the mixer
between them and the browser under all three. The header of a deck shows title, artist and
album of the loaded track, its time and remaining time, the current tempo, the position of the
tempo fader as a percentage and the base tempo of the track. The application can run on the
whole screen: the global settings choose the zoom of the full-screen view and whether the
application starts in it. The manual names no key for it. The same page switches tooltips,
short descriptions of the interface that the manual recommends while learning it; it does
not say whether they are on to begin with. Two more indicators concern controllers: one
lights while MIDI signals come in, whatever device sends them; another says whether the
controllers listed in the preferences are connected, all of them, some or none.

## What Platter does

The zones, top to bottom, are those of `docs/design/DESIGN.md`: status header, two waveform
lanes, deck A with the mixer and deck B, and the file selector to the bottom of the window.

- **Status header.** The kind of audio device in use (built-in, USB, Bluetooth), its sample
  rate, the time one buffer takes, the load of the audio thread, the dropout count, the master
  meter, the clock and the key that opens the preferences. The load reading turns to the
  warning colour above 70 %; the dropout count turns when it is above zero, and operating it
  resets it (`audio.dropouts.reset`). The latency shown is the time of one buffer of the
  device, not a measurement through the converters.
- **No audio device.** The window opens in every case. Without a device the header says so
  and a notice under it explains what that means, with a key that opens the audio page of the
  preferences. The engine keeps running without sound: tracks load, waveforms draw, positions
  move. The application looks for a device again every three seconds and starts playing
  through it when one appears.
- **Deck header.** Title, artist and album from the tags; a track without a title tag shows
  the name of its file. The current tempo, the offset of the tempo fader and the base tempo.
  The base tempo is the one the tag of the file states; Platter does not measure tempo before
  M3, so a file without the tag shows `--.--` and only the offset. No key is shown. The time
  that remains is the large reading, with elapsed and total time under it; operating the times
  swaps remaining and elapsed (`deck.time_display.toggle`). In the last 30 seconds of a
  playing track the remaining time turns to the warning colour.
- **A deck that waits.** An empty deck says so and how to load a track; its controls are
  disabled. While a file is read the deck says so, and the rest of the interface keeps
  working. A file that cannot be read leaves the deck as it was and shows a message in it
  until it is dismissed or another track loads.
- **Meters.** A meter has two columns, left and right, a peak that holds for a second and a
  clip indicator that lights when the signal passes full scale, with the word CLIP beside it.
  The indicator goes out after two seconds or when the meter is operated
  (`mixer.clip.reset`).
- **Languages.** English and Spanish. Every text comes from a translation file; changing the
  language in the preferences takes effect at once.
- **Full screen.** `app.fullscreen.toggle` shows the window on the whole screen, or as a
  window again. Its key sits in the status header beside the preferences, lit while the
  window is on the whole screen, and F11 does the same. The layout follows the size of the
  screen as it follows the size of a window. When the system takes the window out of full
  screen by itself, the key follows.
- **Tooltips.** A key shows its name, and its keyboard shortcut when it has one, in a small
  box 8 points under it, or above it when there is no room below. The box appears after
  the pointer has rested 600 ms on the key, or at once when the key gets the focus from the
  keyboard. It goes when the pointer and the focus leave, and Escape hides it until they
  do. Tooltips are on to begin with; `app.tooltips.toggle` turns them off and on, from the
  General page of the preferences, which keeps the choice in the settings file. Nothing
  behind an open dialog shows one.
- **MIDI.** When the settings file names at least one MIDI controller
  (`docs/behavior/controllers.md`), the status header shows a MIDI indicator after the dropout
  count: a dot, the label "MIDI" and how many of the controllers named are open, as "1/2".
  The dot is green when all of them are open, amber when some are and red when none is; its
  colour never stands alone, the count says the same. The label lights for 150 ms after each
  message any open controller sends, mapped or not, so a control that is moved can be seen
  to reach Platter. Without a controller in the settings the indicator is not drawn.
- **Window size.** The layout is designed 1440 points wide. A wider window gives the extra
  width to the waveform lanes and the file list; a taller or shorter one gains or loses rows
  of the file list. A narrower window shows the whole interface smaller.

## Differences

- No indicators for battery or recording; recording arrives in M4.
- The reference product has two indicators for controllers, one for signals and one for
  connections. Platter draws one, with both: the count of open controllers and the light of
  the messages. A controller is counted as open when its input opened at start; one that is
  unplugged later is still counted, since the system does not tell the input it is gone
  (`docs/decisions/0025-midi-ports.md`).
- The clock is not shown yet. The standard library gives the time in UTC only; the time of
  day needs the offset of the local time zone, which needs either a dependency outside the
  approved list or a call into the system. It is drawn when the application is given a time,
  as the capture mode does.
- Only keys have tooltips. The knobs, the faders and the segmented choices get theirs with
  the next change to their widgets. A disabled key shows none: the design system asks for
  the reason it is disabled, which the keys do not know yet. The shortcut of a key that acts
  while held is not shown.
- Full screen has no zoom of its own and no setting to start in it; the layout scales as it
  does for a window.
  Full screen has been checked only through its command and its key in the tests, not yet
  in a real window.
- The reference product hides or shows parts of the deck header by preference. Platter shows
  the same set always.
- Screen readers get no information yet: the bridge to the accessibility service of the
  system is a cargo feature that is off, because on Linux it brings three crates that an
  advisory marks as unmaintained. The controls already report their names and values, so
  turning the feature on needs no change in the interface.

## Edge cases

| Input | Result |
| --- | --- |
| No audio device, or the device goes away while playing | The header and the notice say so; the engine keeps running in silence |
| The device chosen in the settings is not connected | The notice says so; nothing else is tried |
| A window narrower than 1440 points | The whole interface is drawn smaller, down to a window of 960 points |
| A title longer than its place | It is cut with an ellipsis; the full title shows on hover |
| A track longer than 99 minutes | The times grow to three digits of minutes |

## Tests

`crates/ui-egui/tests/panels.rs`: every control emits its command, no text is written in the
code, every command can be reached with the keyboard, no colour comes from outside the design
tokens, and readouts keep their width while their digits change.
`crates/ui-egui/tests/tooltips.rs` for the tooltips. `crates/ui-egui/tests/midi_indicator.rs`
for the MIDI indicator; no real controller has lit it yet.
