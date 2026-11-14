# File selector

The strip along the bottom of the window. In M2 it is a file selector: a tree of folders and
the audio files of the folder chosen. In M3 it becomes the browser, with a collection,
playlists and search.

## What the public manual says

Sections of the reference manual, read on 13 November 2026:

- *Overview*, section *Browser*

The browser is where tracks are loaded into decks from. It has a tree with the sources of
tracks, among them an explorer of the drives of the computer, and a list with the tracks of
the source selected.

## What Platter does

- **Tree.** The home folder of the user and any folder named under `roots` in the `[browser]`
  table of the settings file. A folder opens and closes with its chevron, or with the right
  and left arrows when it has the keyboard (`browser.toggle`); operating its row shows it in
  the list (`browser.navigate`). A folder that has been read shows how many audio files it
  holds.
- **List.** The folders of the current folder first, then its audio files, each group sorted
  by name without regard to case. A file is listed when its extension is one of the formats
  Platter plays (`docs/behavior/formats.md`). Columns: title, artist, album, tempo, duration
  and format with sample rate. A file without a title tag shows its file name. A file loaded
  in a deck carries the letter of that deck.
- **Reading.** A folder is read on a worker thread: its names appear at once and the columns
  fill in as the header of each file is read. Nothing is decoded for the list. Leaving a
  folder stops the reading of its files.
- **Selection.** A click selects a row (`browser.select`). With the list focused, the up and
  down arrows move the selection (`browser.move`), Enter opens the selected folder
  (`browser.activate`) and Backspace shows the folder above (`browser.up`), up to a root of
  the tree and no further. A double click opens a folder.
- **Loading.** How a track gets from the list into a deck is Platter's own definition for
  this milestone. Load A and Load B load the selected track (`browser.load_selected`), by default
  also with Shift and the left or right arrow. Dragging a row onto a deck loads it there. A
  file dropped on the window from outside the application loads into the deck under the
  pointer; when the system does not say where it fell, into the first empty deck, or deck A.
  Loading sends `deck.load` to the engine, which decodes on its own worker thread.
- The folder shown last is remembered in the settings file.

## Differences

- No collection, playlists, history, search or preview player; those are M3.
- The drives of a Windows machine are not listed by themselves. Finding them without a system
  call the project cannot make means asking for every drive letter in turn, and a letter that
  stands for a network share that does not answer blocks for a long time. A drive is reached
  by naming it under `roots` in the settings file, or by dropping a file from it on a deck.
- Hidden folders and files, those whose name starts with a dot, are not listed.
- A folder shows its first 10 000 entries.

## Edge cases

| Input | Result |
| --- | --- |
| A folder that does not exist or cannot be read | An empty list |
| A file with an audio extension that is not audio | Listed with "Can't read"; loading it shows the message of the deck |
| Loading while a folder is selected | Nothing happens |
| Moving the selection past either end | It stops at the end |
| A folder with no audio files | The list says so |

## Tests

`crates/ui-egui/tests/files.rs` for listing, selection, loading and the tree, against folders
the test writes; `crates/ui-egui/tests/panels.rs` for the controls and the keys of the list.
