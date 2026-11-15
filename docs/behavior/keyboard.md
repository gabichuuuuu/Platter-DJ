# Keyboard

Keyboard shortcuts and keyboard operation of the controls.

## What the public manual says

Sections of the public manual, read on 13 November 2026:

- *Preferences*, section *Controller Manager page*

The documented application treats the keyboard as a controller: a mapping assigns keys to
functions, and the user can change the assignments and keep several mappings.

## What Platter does

### Shortcuts

A shortcut is a key chord and the command it triggers. The shortcuts are a table in
`keymap.toml`, in the configuration folder of the user. Platter writes the default table there
on its first run and reads the file when it starts.

```toml
[[binding]]
keys = "Shift + Left"
command = "browser.load_selected"
deck = "a"
```

- `keys` is one key, alone or with Shift, Ctrl (Command on macOS) or Alt. Keys are named in
  English: letters, digits, `F1` to `F12`, `Space`, `Enter`, `Left`, `Right`, `Up`, `Down`,
  `Comma`, `Period`, `Minus`, `Plus`.
- `command` is the identifier of a command. Every other key of the binding is a parameter of
  that command. The commands of the engine are in `crates/engine/src/command.rs` and those of
  the application in `crates/ui-egui/src/command.rs`, each with its parameters.
- A command that acts while a key is held (`deck.cue`, `deck.cup`) is pressed when the key
  goes down and released when it comes up. A binding may give the parameters to send on
  release in a `release` table; the tempo bend uses it to return to zero.
- A file with a mistake is reported in the log, naming the binding, and the default
  shortcuts are used.

The default table:

| Function | Deck A | Deck B |
| --- | --- | --- |
| Play or pause | Z | N |
| Cue | X | M |
| Cue and play | C | Comma |
| Bend tempo down, up | Q, W | O, P |
| Kill low, mid, high | 1, 2, 3 | 8, 9, 0 |
| Pre-listen | 4 | 7 |
| Load selected track | Shift + Left | Shift + Right |
| Zoom out, in | F1, F2 | F3, F4 |
| Loop on or off | A | L |
| Loop size down, up | S, D | J, K |
| Beatjump back, forward | E, R | U, I |

Ctrl and comma opens the preferences. F11 shows the window on the whole screen, or as a
window again.

### Controls

Every control takes the keyboard focus with Tab and shows a focus ring.

| Control | Keys |
| --- | --- |
| Key (button) | Space or Enter. A key that acts while held acts while Space or Enter is held |
| Knob | Up or Right raises, Down or Left lowers, by 1 %; with Shift by 0.1 %; Page Up and Page Down by 10 %; Home returns to the default |
| Channel fader, tempo fader | Up and Down; Page Up and Page Down; Home. The tempo fader moves by 0.05 %, with Shift by 0.01 % |
| Crossfader | Left and Right by 1 %; Page Up and Page Down by 10 %; Home returns to the centre |
| Drop-down | Space, Enter or Alt and Down opens; Up and Down move; Enter picks; Escape closes |
| File list | Up and Down move the selection; Enter opens a folder; Backspace shows the folder above |
| Folder of the tree | Space or Enter shows it; Right opens it; Left closes it |

A key the focused control uses does not trigger a shortcut: with a knob focused the arrows
move the knob. A chord with Ctrl or Alt always reaches the shortcuts.

## Differences

- One table of shortcuts in one file; no editor in the application and no second mapping.
  The preferences show the table read-only.
- The file is read at start; a change takes effect on the next run.
- Mouse, MIDI and HID mappings are M3 and M5.

## Edge cases

| Input | Result |
| --- | --- |
| A key held while the window loses the keyboard | Its command is released |
| A key repeated by the system while held | One press, one release |
| Two bindings on one chord | Both commands are sent |
| A modifier released before the key | The release is still sent |
| A binding for a command that does not exist, or with a wrong parameter | The whole file is refused and the defaults are used |

## Tests

`crates/ui-egui/src/keymap.rs` and `crates/ui-egui/tests/panels.rs`.
