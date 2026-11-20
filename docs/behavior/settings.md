# Preferences

The preferences dialog and the files of the user. M2 has two pages, Audio and General; M3
adds the Decks, Mixer, Library, Controllers and Keyboard pages; the Controllers page is in
`docs/behavior/controllers.md`, *The controller manager*.

## What the public manual says

Sections of the public manual, read on 13 November 2026:

- *Preferences*, section *Audio setup page*
- *Preferences*, section *Mixer page*

The audio page chooses the audio device, its sample rate and its buffer size, and shows the
latency that results; a smaller buffer lowers the latency and raises the load, and too small a
buffer leads to dropouts. A routing page assigns the main output and the monitor output to
channels of the device. The mixer page shapes the curve of the crossfader.

Read again on 15 November 2026 for the Mixer page: *Preferences*, sections *Output Routing
page* and *Mixer page*. The routing page chooses between the internal and the external mixing
mode. The mixer page sets the time of the automatic crossfade and, for each mixing mode, turns
autogain on or off; in the external mode it also lets the master volume and the gain and
equaliser of each channel act on the outgoing signals.

Read on 15 November 2026 for the Library page: *Preferences*, section *Transport page*, setting
*Min. Playtime*, and *Managing Your Track Collection*, sections *Checking Consistency of your
Track Collection* and *Relocating Missing Tracks*. The minimum play time is how long a track has
to play before it counts as played; the manual gives no default. The consistency check is run
from the menu of the collection and shows its report in a dialog.

Read on 17 November 2026: *Preferences*, *File Management* page, settings *Music Folders*,
*Import Music-Folders at Startup* and *Show Consistency Check Report on Startup*. The page keeps
a list of music folders, with keys to add one, delete the one selected and change the path of
one that moved. With the import at start on, each start imports the tracks of those folders that
the collection does not hold yet. With the report on, the consistency check report shows at each
start. The manual states no default for either.

Read on 17 November 2026: *Preferences*, *Transport* page and *Global Settings* page. The
transport page sets the range of every tempo fader at once, from 2 % to 50 % or 100 %, and
holds the sensitivity of the tempo bend, the sync mode, which decks can become tempo master,
the key lock while scratching and how its key is shown, the size under which a track is a
loop, the minimum play time, the bars of a phrase, what a click in the waveform does and when
CUP starts playing. The global page shows or hides the global section and chooses its modules,
sets the zoom of full screen and whether the application starts in full screen, turns the
tooltips and the value shown over a control on or off, the warnings of the deck headers, the
dialogs hidden by the user and the sharing of usage data.

Read on 17 November 2026: *Preferences*, *Browser Details* page. It allows editing the tags in
the list by a double click, sets the font, its size and the height of the rows, shows or hides
the preview player, the cover art, the favourites, the track info bar and the status bar, and
chooses how keys are coloured while key lock is on: none, the matching keys (the default), also
the keys a semitone away, or all.

## What Platter does

The dialog opens from the status header, from the notice about the audio device and with
Ctrl and comma (`app.settings.open`). It closes with its Close key or with Escape
(`app.settings.close`). While it is open the window behind it takes no input and the
shortcuts of the decks and the mixer rest.

### Audio

| Setting | Command | Notes |
| --- | --- | --- |
| Audio device | `audio.device.set` | The default device of the system, or a device by name |
| Sample rate | `audio.sample_rate.set` | 44.1, 48 or 96 kHz; a rate the chosen device does not accept cannot be picked |
| Buffer size | `audio.buffer.set` | 64 to 1 024 samples |
| Output routing | `audio.routing.set` | The output of the device for each side of the master and of the headphones |
| Test tone | `master.test_tone.play` | One second of tone on the master or on the headphones |

- The status line shows whether a device is playing, the time one buffer takes and the
  dropouts counted since the start, or why no device is playing.
- A change of device, buffer size or routing opens the audio output again and keeps the
  engine as it is: the tracks stay loaded and the decks keep their position, with a short
  silence. A change of sample rate builds the engine again at the new rate: the controls keep
  where they stand and the tracks are loaded again, from their start and paused.
- The device decides the buffer it really uses. When that is not the size asked for, the
  page says which size is in use. On Windows the shared mode of WASAPI always uses the period
  of the device, 10 ms on most; see `docs/decisions/0010-audio-output.md`. Going below it needs
  the exclusive mode of WASAPI, left for a later version.
- With a device of two outputs the headphone routing and the headphone test tone are
  disabled and the page says why.

### General

| Setting | Command | Notes |
| --- | --- | --- |
| Language | `app.language.set` | English or Spanish; takes effect at once |
| Tooltips | `app.tooltips.toggle` | A key, lit while the keys show their tooltips (`docs/behavior/interface.md`) |
| Full screen at start | `app.fullscreen_at_start.toggle` | Off by default. When on, the window goes to the whole screen as Platter starts, as `app.fullscreen.toggle` does (`docs/behavior/interface.md`) |

### Keyboard

| Setting | Command | Notes |
| --- | --- | --- |
| Shortcuts | `app.keymap.open` | The list of shortcuts, read-only, and a key that opens the shortcut file |

### Decks

| Setting | Command | Notes |
| --- | --- | --- |
| Sync mode | `global.sync.mode.set` | Tempo, or tempo and the place in the beat (`docs/behavior/sync.md`); beat by default |
| Tempo range | `app.tempo_range.set` | 8, 16 or 50 %: the range every tempo fader gets at once, through `deck.tempo.range.set` (`docs/behavior/transport.md`), and again each time Platter starts; 8 % by default; kept under `[decks]` as `tempo_range` |
| Start at the load cue | `global.load_marker.toggle` | A track with a cue of the type load starts there, its cue point too; on by default, as the public manual's *Initially cue to load marker*; kept under `[decks]` |
| Load only into a stopped deck | `app.load_stopped_only.toggle` | A load sent to a playing deck is refused and written in the log; off by default |
| Key notation | `app.key_notation.set` | Musical with flats, the default, musical with sharps, or Open Key (`docs/behavior/browser.md`, *Analysis in the background and keys*); kept under `[decks]` |
| Range of tempos | `app.analysis_range.set` | The tempos the analysis looks in, 70 to 180 BPM by default (`docs/behavior/analysis.md`, *Range of tempos*); kept under `[decks]` as `bpm_min` and `bpm_max` |

- The setting follows the engine whatever changes it, a shortcut or a controller too, and is
  applied when Platter starts and on Restore defaults. `settings.toml` keeps it under
  `[decks]`.

### Mixer

| Setting | Command | Notes |
| --- | --- | --- |
| Mixing mode | `mixer.mode.set` | Internal, or external with each deck on its own outputs (`docs/behavior/external-mixer.md`) |
| Autogain | `mixer.autogain.toggle` | A key, lit while autogain is on (`docs/behavior/autogain.md`) |
| Equaliser | `mixer.eq.model.set` | Smooth, soft, isolator or steep (`docs/behavior/mixer.md`, *Equaliser and filter models*) |
| Filter | `mixer.filter.model.set` | Gentle, steep or ladder; one help line under both rows |
| Gain and equaliser | `mixer.external.eq_gain.toggle` | A key, lit while they act in external mode |
| Main volume | `mixer.external.master_volume.toggle` | A key, lit while it acts in external mode |
| Automatic fade | `mixer.crossfader.fade_beats.set` | 1, 2, 4, 8, 16, 32 or 64 beats of the master clock (`docs/behavior/mixer.md`) |
| Crossfader curve | `mixer.crossfader.curve.set` | Smooth, linear or sharp |
| Effect units, routing of each unit | `app.effect_units.set`, `fx.unit.routing.set` | Two or four; insert or post fader (`docs/behavior/effects.md`) |
| Fade cues | `global.fade_markers.toggle` | A key, lit while a deck crossing a fade-out cue starts its partner (`docs/behavior/auto-mix.md`) |
| Keep values | `fx.keep_values.toggle` | A key, lit while a newly chosen effect keeps the values of the controls (`docs/behavior/effects.md`) |

- The two keys of the external mode can be set in either mode; the page says they act only
  in the external one.
- The settings of the page follow the mixer whatever changes it, a shortcut or a controller
  too, and are applied when Platter starts.
- A length of fade set by another way that is not one of the choices leaves none selected,
  and is kept in the settings file to the nearest whole beat.
- The maximiser of the master (`docs/behavior/master.md`) has no row on the page yet. The
  Limiter key and its drive, ceiling and release are kept in `settings.toml` under `[mixer]`
  as `limiter`, `maximiser_drive_db`, `maximiser_ceiling_db` and `maximiser_release_ms`;
  they follow the mixer whatever changes them, are applied when Platter starts, and Restore
  defaults returns them to theirs.

Restore defaults (`app.settings.restore_defaults`) returns every setting of the pages to its
default.

### Library

| Setting | Command | Notes |
| --- | --- | --- |
| Minimum play time | `app.min_playtime.set` | 5, 10, 20, 30, 60, 120 or 300 seconds a deck plays a track before it counts as played (`docs/behavior/browser.md`, *Played state*); 30 by default |
| Check at start | `library.check_at_start.toggle` | Off by default. Opens the preferences on this page and *Collection maintenance* over it, with a check of the collection, once the library is open and the imports at start are done |
| Tag writing | `library.tag_writing.set` | Everything, the default, Platter's own field only, or nothing (`docs/behavior/tag-writing.md`, *When Platter writes*); kept under `[browser]` |
| Music folders | `library.music_folder.add`, `library.music_folder.remove`, `library.music_folder.select` | A list of up to 64 folders, one shown at a time in a list; the add key takes the folder the explorer of the browser shows, which the line under the row names, and the remove key drops the one shown. Kept under `[browser]` |
| Import at start | `library.import_at_start.toggle` | On by default. At each start the music folders are imported one after the other, as `library.import_folder` does (`docs/behavior/library.md`, *Importing folders*) |

- The new time applies at once, also to the tracks the decks are playing: one that has played
  longer than it counts at the next frame.
- A time written in the settings file that is not one of the choices, from 5 to 600 seconds,
  is kept and leaves none selected.
- The page also says where the library file is: `library.sqlite`, beside the settings file.
- *Collection maintenance* opens the dialog that checks, relocates and removes the missing
  tracks (`docs/behavior/browser.md`, *Collection maintenance*).
- A folder already in the list is not added twice; with the explorer showing no folder, or one
  already listed, the add key rests. A list of 64 folders takes no more.
- The imports at start wait until the library has opened, then run one at a time, each when
  the one before has finished; a folder that no longer exists gives its error in the status bar
  and the next one follows. The check at start runs after the last import, so its report counts
  what they added. With the library unavailable, neither runs.

### Files

Platter keeps its files in a folder of its own inside the configuration folder of the user:
`%APPDATA%\Platter` on Windows, `~/Library/Application Support/Platter` on macOS and
`$XDG_CONFIG_HOME/platter` or `~/.config/platter` on Linux.

| File | Content |
| --- | --- |
| `settings.toml` | Language, tooltips, audio device, sample rate, buffer size, routing, crossfader curve, the settings of the Decks, Mixer and Library pages, the folder the file selector showed last and its extra roots |
| `library.sqlite` | The collection, playlists, history, preparation list and favourites (`docs/behavior/library.md`) |
| `Backup/library-*.sqlite` | The 20 newest copies of the library, one made each time it opens changed since the last copy (`docs/behavior/library.md`, *Backups*) |
| `keymap.toml` | The keyboard shortcuts (`docs/behavior/keyboard.md`) |
| `platter.log` | The log of the last run |

`settings.toml` is written each time a setting changes: to a temporary file first, then renamed
over the old one. A value in it that is missing or makes no sense falls back to its default,
one setting at a time. Nothing in these files leaves the machine.

## Differences

- Seven pages, as the design system groups them. The rules of loading and the options of the
  key are on the Decks page. The public manual's loading page also stops a deck at the end of
  its track, which a Platter deck always does, and offers to load the next track of a
  playlist, to duplicate a deck and to reset the controls on a load, which Platter does not
  have.
- The range of tempos is on the Decks page, in a section of its own, not on an analysis page.
- Of the transport page, Platter has the range of the tempo faders and the sync mode, on the
  Decks page, and the minimum play time, on the Library page. Its ranges are the three of the
  deck, 8, 16 and 50 %. The bend of the tempo has one strength; in the automatic mode a deck
  that starts to play becomes the tempo master whether it is heard or not
  (`docs/behavior/sync.md`); the key lock has no mode for scratching, and the deck always shows
  the key it results in. No track is looped by its length, the deck counts no phrases, a click
  on the overview always seeks, and CUP starts playing when it is released
  (`docs/behavior/transport.md`).
- Of the browser details page, Platter has none as settings. The browser always shows the
  preview player, the covers, the favourites and the status bar, in the type and the row height
  of the design system, which draws the browser this way and gives it no settings of display.
  Tags are edited in the details dialog of a track, not in the list. Keys are coloured in one
  way, those that match (`docs/behavior/browser.md`, *Analysis in the background and keys*).
- Of the global page, Platter has the tooltips and the start in full screen, on the General
  page. It has no global section of effects before M4, no zoom of its own in full screen (the
  layout scales), no value shown over a control besides the tooltip, no dialog the user can
  hide, and nothing to share: it sends no data (`AGENTS.md`, Privacy). Warnings of a deck show
  in its header always.
- The minimum play time is on the Library page, not on a Transport page, which Platter does not
  have yet; the design system groups the browser settings on the Library page.
- The music folders, the import at start and the check at start are on the Library page, not on
  a file management page. A folder that moved is removed and added again: there is no key to
  change the path of one. A folder is added from the explorer of the browser rather than from a
  dialog of the system, which Platter does not open.
- Platter's check at start opens the maintenance dialog it already has, which shows the report
  and the keys to relocate and remove.
- The crossfader curve is on the Mixer page, under the length of an automatic fade, where the
  design system moves it from the General page of M2.
- The mixing mode is on the Mixer page, not on a routing page: Platter's routing is on the
  Audio page and chooses outputs, not modes.
- No setting for the latency itself: the buffer size is the setting.
- The routing chooses single outputs, not pairs to swap.

## Edge cases

| Input | Result |
| --- | --- |
| The device goes away while the page is open | The status line says so; the list of devices is read again every three seconds |
| A settings file that is not valid TOML | The defaults |
| The same output for both sides of the master | Both sides are written to it; the second one wins |
| A headphone output that is also a master output | No headphone signal |
| The configuration folder cannot be found or written | The application runs with the defaults and saves nothing |
| A music folder in the settings file that is empty or not text | Left out |
| More than 64 music folders in the settings file | The first 64 |
| A tempo range in the settings file that is not 8, 16 or 50 | 8 % |
| A setting of the maximiser in the settings file outside its range, or not a number | Its default; the others are kept |
| A music folder that is gone at start, with the import at start on | Its error in the status bar; the next folder is imported |

## Tests

`apps/platter/src/config.rs` for the settings file; `crates/ui-egui/tests/panels.rs` for the
controls of the Audio and General pages and `crates/ui-egui/tests/settings_mixer.rs` for the
Decks, Mixer and Library pages; `apps/platter/src/starting.rs` for the imports and the check at
start; `crates/audio-io/tests/null_backend.rs` for the routing.
