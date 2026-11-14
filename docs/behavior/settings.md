# Preferences

The preferences dialog and the files of the user. M2 has two pages: Audio and General.

## What the public manual says

Sections of the reference manual, read on 13 November 2026:

- *Preferences*, section *Audio setup page*
- *Preferences*, section *Mixer page*

The audio page chooses the audio device, its sample rate and its buffer size, and shows the
latency that results; a smaller buffer lowers the latency and raises the load, and too small a
buffer leads to dropouts. A routing page assigns the main output and the monitor output to
channels of the device. The mixer page shapes the curve of the crossfader.

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
| Crossfader curve | `mixer.crossfader.curve.set` | Smooth, linear or sharp |
| Keyboard | `app.keymap.open` | The list of shortcuts, read-only, and a key that opens the shortcut file |

Restore defaults (`app.settings.restore_defaults`) returns every setting of the two pages to
its default.

### Files

Platter keeps its files in a folder of its own inside the configuration folder of the user:
`%APPDATA%\Platter` on Windows, `~/Library/Application Support/Platter` on macOS and
`$XDG_CONFIG_HOME/platter` or `~/.config/platter` on Linux.

| File | Content |
| --- | --- |
| `settings.toml` | Language, audio device, sample rate, buffer size, routing, crossfader curve, the folder the file selector showed last and its extra roots |
| `keymap.toml` | The keyboard shortcuts (`docs/behavior/keyboard.md`) |
| `platter.log` | The log of the last run |

`settings.toml` is written each time a setting changes: to a temporary file first, then renamed
over the old one. A value in it that is missing or makes no sense falls back to its default,
one setting at a time. Nothing in these files leaves the machine.

## Differences

- Two pages; the others arrive with the functions they configure.
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

## Tests

`apps/platter/src/config.rs` for the settings file; `crates/ui-egui/tests/panels.rs` for the
controls of both pages; `crates/audio-io/tests/null_backend.rs` for the routing.
