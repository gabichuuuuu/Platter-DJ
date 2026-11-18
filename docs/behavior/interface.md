# Interface

The main window: its zones, the status header, the deck header and the messages the
interface shows when something is missing.

## What the public manual says

Sections of the public manual, read on 13 November 2026:

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
  box 8 points under it, or above it when there is no room below. A knob, a fader, a number
  field and a list show their name the same way, and each part of a segmented choice its
  label with its shortcut. A control that sets a value shows no shortcut, since each of its
  shortcuts sends a value of its own; it shows no tooltip while it is dragged, and a list none
  while it is open. The box appears after
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
- **Window size.** The layout is designed 1440 points wide. In a wider window, full screen
  included, the mixer keeps its width and the two columns of decks share the rest, so the
  decks reach the edges of the window; the waveform lanes and the file list take the whole
  width. A taller or shorter window gains or loses rows of the file list. A narrower window
  shows the whole interface smaller. Until 18 November 2026 the decks kept 520 points and
  stayed in the middle, which left empty bands at both sides in full screen; the maintainer
  chose to widen them.

## Differences

- No indicators for battery or recording; recording arrives in M4.
- The documented application has two indicators for controllers, one for signals and one for
  connections. Platter draws one, with both: the count of open controllers and the light of
  the messages. A controller is counted as open when its input opened at start; one that is
  unplugged later is still counted, since the system does not tell the input it is gone
  (`docs/decisions/0025-midi-ports.md`).
- The clock is not shown yet. The standard library gives the time in UTC only; the time of
  day needs the offset of the local time zone, which needs either a dependency outside the
  approved list or a call into the system. It is drawn when the application is given a time,
  as the capture mode does.
- A disabled control shows no tooltip: the design system asks for the reason it is disabled,
  which the controls do not know yet. The shortcut of a key that acts
  while held is not shown.
- Full screen has no zoom of its own and no setting to start in it; the layout scales as it
  does for a window.
  Full screen has been checked only through its command and its key in the tests, not yet
  in a real window.
- The documented application hides or shows parts of the deck header by preference. Platter shows
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
| A window 1920 points wide | Decks of 760 points from edge to edge, the mixer of 396 points between them |
| A title longer than its place | It is cut with an ellipsis; the full title shows on hover |
| A track longer than 99 minutes | The times grow to three digits of minutes |

## Tests

`crates/ui-egui/tests/panels.rs`: every control emits its command, no text is written in the
code, every command can be reached with the keyboard, no colour comes from outside the design
tokens, and readouts keep their width while their digits change. `crates/ui-egui/src/shell.rs`
for the zones of a wide window, in both layouts.
`crates/ui-egui/tests/tooltips.rs` for the tooltips. `crates/ui-egui/tests/midi_indicator.rs`
for the MIDI indicator; no real controller has lit it yet.

## Deck sizes

### What the public manual says

Sections of the public manual, read on 17 November 2026:

- *Setting up*, section *Switching Deck Layouts*
- *Preferences*, section *Decks Layout*
- *Overview*, section *Deck header*

A deck can be drawn in several sizes, from a strip with play and a header of one row up to the
full deck with its advanced panel. The smaller sizes show a header of one row, with the title,
the remaining time and the tempo, and leave out the advanced panel; the full ones add the
header with every detail of the track, and the largest adds the advanced panel. In the layout
with the waveforms side by side above the decks there are two sizes: a slim one, with the
one-row header, the stripe, transport, key lock, the tempo fader, the phase meter, SYNC and
MASTER; and a full one, which adds the full header and the advanced panel. The size is chosen
on the Decks Layout page of the preferences, one setting for decks A and B and one for C and
D, and a double click on the header of a deck moves to the next size. The manual does not say
whether a smaller deck gives room to the browser.

### What Platter does

- **Two sizes.** Full, as before, and compact (`app.deck_size.set`, `full` or `compact`). One
  setting covers decks A and B, chosen on the General page of the preferences, kept in the
  settings file; full to begin with. A double click on the header of a deck, left of the
  tempo, sends the other size.
- **The compact deck.** Its header is one row, as `docs/design/DESIGN.md` gives it: the letter,
  the title, the current tempo and the large time; no cover, artist, album, tempo offset or
  second times. The advanced panel (MOVE, CUE and GRID with its selector) is left out. The
  overview stays, and so do SYNC, MASTER, the phase meter, the key controls, the hotcues, the
  loop row, the transport and the tempo column.
- **Room.** The decks share their band with the mixer, which keeps its size. With compact
  decks the band shrinks to the height the mixer needs and the file list below gains those
  rows; the overview of each deck takes what the deck has left over.
- A deck that shows a notice shows it in the compact size too, in place of its hotcues.

### Differences

- Two sizes instead of five: the waveforms of Platter are always the lanes side by side above
  the decks, so the sizes are those the manual gives that layout. The compact size keeps the
  hotcues and the loop row, which the design system gives it.
- The setting is on the General page: the Decks page has no room left, and the General page
  holds the other settings of the window.
- The compact header follows the large time: the time a click on it chose, remaining or
  elapsed.
- The commands of the advanced panel stay on the keyboard and on controllers while it is
  hidden.

### Edge cases

| Input | Result |
| --- | --- |
| A size other than `full` or `compact` in the settings file | Full |
| The size changes while a cue is being mapped | The mapping mode stays; the hotcues show it |
| A double click on the times | Swaps remaining and elapsed twice; the size does not change |

### Tests

`crates/ui-egui/tests/deck_size.rs`: the compact header, what it leaves out, the band and the
double click. `crates/ui-egui/tests/settings_mixer.rs` for the setting.

## Deck layout

### What the public manual says

Sections of the public manual, read on 17 and 18 November 2026:

- *Preferences*, section *Decks Layout page*, *Deck Layout*
- *Setting up*, section *Switching Deck Layouts*

The decks layout page of the preferences has an option that shows decks C and D below decks A
and B, with a size for decks A and B and another for C and D. A deck that is hidden is muted.
The manual does not say how the window shares its room between two and four decks.

### What Platter does

- **The setting.** Two decks or four (`app.deck_layout.set`, `two` or `four`), chosen on the
  General page of the preferences, in the Layout section above the deck size, and kept in the
  settings file under `[decks]` as `layout`; four to begin with. Restoring the defaults goes
  back to four.
- **Two decks.** The window is as before: the waveform lanes, deck A, the mixer with two
  channels and deck B, in the size the deck size setting gives.
- **Four decks.** `docs/design/DESIGN.md`, *Four-deck layout (M4)*: under the status header,
  the decks band of 580 points with deck A over deck C on the left, the mixer in the middle and
  deck B over deck D on the right, then the browser to the bottom of the window. There are no
  waveform lanes: each deck carries its own.
- **The deck of the four-deck layout.** A header of two lines (the letter, the cover, the
  title with the tag of its kind, `Track`, over the artist and album, the tempo over its offset
  and the key the deck plays, the large time over the next one), the scrolling waveform with
  the overview under it, the row of sync with the key controls, one row with play, CUE, CUP,
  Flux, reverse and the loop keys, and the eight hotcues in one row; the tempo column at the
  right, with its range, fader, reading and the two bend keys side by side. There is no advanced panel and no beatjump row in this size,
  and the deck size setting does not apply to it. The zoom of the waveform is the one of the
  deck, kept from the two-deck layout.
- **The mixer of the four-deck layout.** Four narrow channels in the order A, B, C, D, each
  with gain and its Auto key (`mixer.gain.restore`, lit while the gain is the autogain value
  of the track, disabled until the analysis has found it), the three bands with their kill
  keys, its meter, the filter, key and pan, pre-listen, the fader and the assign keys; the
  master column with its meter, level and, one under the other, the headphone mix and volume;
  the crossfader row under the four channels, its ends marked L and R.
- **Loading.** The browser shows Load A, Load B, Load C and Load D, left to right; a file
  dragged onto any of the four decks loads into it, and a file dropped from outside away from
  the decks goes to the first empty deck of the four, or deck A.
- **A larger window.** The mixer keeps its width, the two columns of decks share the rest of
  the width, and the browser takes the rest of the height (decided by the maintainer on
  18 November 2026).

### Differences

- The selector is in the preferences, not in the row of effects above the decks where the
  design system draws it: the maintainer chose it on 18 November 2026, and the row of effects
  waits for the effect units. Without them the four-deck layout has no row of effects and the
  browser takes its height.
- Decks C and D are not muted while the two-deck layout hides them: a track loaded into them by
  a command or a mapping plays and is heard (`docs/behavior/decks.md`).
- One layout for the four decks instead of a size per pair: the design system gives the decks
  of the four-deck layout one size.
- The controls the design system draws for later functions are left out: the effect sends and
  the effect knob of each channel, the limiter key, the mix recorder and the stem lanes. Their
  place in the mixer stays empty.

### Edge cases

| Input | Result |
| --- | --- |
| A layout other than `two` or `four` in the settings file | Two decks |
| The layout changes while decks C and D play | They keep playing; the window shows or hides them |
| A window narrower than 1440 points | The whole interface is scaled down, as with two decks |
| A deck of the four-deck layout shows a notice | The notice takes the place of its transport and hotcues |

### Tests

`crates/ui-egui/tests/panels.rs`: the four-deck state goes through every check of the panels
(each control emits its command, no text out of the translations, no colour out of the tokens,
no overlap in English and Spanish), the place of each deck and channel, the load keys, the drops
and the setting on the General page. `crates/ui-egui/src/command.rs` for the command and
`apps/platter/src/config.rs` for the settings file.
