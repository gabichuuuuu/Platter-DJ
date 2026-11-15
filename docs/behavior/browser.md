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

- No playlists, history, search or preview player yet; those come later in M3. The
  collection has its own section below.
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

## The collection

From M3 the tree starts with the collection, the tracks imported into the library
(`docs/behavior/library.md`), above the folders of the explorer.

### What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Overview*, section *Browser*
- *Basic usage tutorials*, section *Performing a Text Search*

The tree of the browser lists the sources of tracks: the collection, the playlists, the history
and an explorer of the drives. The list shows the tracks of the source selected. A search field
filters by the tags of the tracks as the user types: the list shown while typing, and with Enter
the whole collection. A track is loaded into a deck from the list.

### What Platter does

- **Nodes.** The tree shows *Collection* first, then the roots of the explorer as before. Its
  row, operated, shows the collection in the list (`browser.node.select`); it carries the
  number of tracks in the collection once the library has counted them. A folder of the
  explorer still shows its folder (`browser.navigate`).
- **The list of the collection.** One row per track, sorted by path, with the same columns as
  the files of a folder: title (the file name when the track has none), artist, album, tempo,
  duration and sample rate. The collection does not keep the container of a file, so the last
  column shows the sample rate alone. A track whose file was gone at the last consistency
  check is listed with "Missing". The list shows the first 1000 tracks; the line above it
  names the collection and says how many tracks it holds in all.
- **Import.** While a folder of the explorer is shown, *Import* adds the music of that folder
  and of the folders inside it to the collection (`library.import_folder`), as the store
  imports (`docs/behavior/library.md`, *Importing folders*). The tags come from the header of
  each file, read as the file list reads them. The import runs on the worker thread of the
  library; the line above the list says it is running and, at the end, how many tracks it
  added, how many it already knew and how many it could not read. One import runs at a time:
  the key is off until it ends.
- **The library file.** `library.sqlite`, in the folder of the settings. When it cannot be
  opened, or the folder of the settings cannot be found, the *Collection* row stays without a
  count, the list says the library is not available, *Import* stays off and the explorer keeps
  working.
- **Work off the interface thread.** The library runs on one worker thread of its own;
  readings of the collection and imports are sent to it and their results come back through
  a channel. After an import the collection is read again.
- Selecting, moving the selection and loading work in the collection as in a folder;
  Backspace does nothing there, since no folder holds the collection.

### Differences

- No search field yet: it comes with the next unit, with a text field of Platter's own.
- The columns cannot be chosen or sorted yet, and comment, key, colour and rating are not
  kept yet; they come with the units that add them to the store.
- Playlists, preparation list, history, favourites and the preview player are not in the tree
  yet.
- Import has no progress count while it runs, and cannot be stopped; a large folder keeps the
  library busy until it ends, and the list of the collection waits for it.

### Edge cases

| Input | Result |
| --- | --- |
| The library file cannot be opened or is damaged | The collection says it is not available; the explorer works |
| Import while the collection is shown | Nothing happens: *Import* acts on a folder of the explorer |
| Import of a folder already imported | Known files stay as they are; the status line counts them |
| An empty collection | The list says so |
| Loading a missing track | The deck says it cannot read the file |

## Tests

`crates/ui-egui/tests/files.rs` for listing, selection, loading and the tree, against folders
the test writes; `crates/ui-egui/tests/collection.rs` for the collection, against a library
and folders the test writes; `crates/ui-egui/tests/panels.rs` for the controls and the keys of the list.
