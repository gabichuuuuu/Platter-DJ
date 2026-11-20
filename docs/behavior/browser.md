# File selector

The strip along the bottom of the window. In M2 it is a file selector: a tree of folders and
the audio files of the folder chosen. In M3 it becomes the browser, with a collection,
playlists and search.

## What the public manual says

Sections of the public manual, read on 13 November 2026:

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
- **Selection.** A click selects a row (`browser.select`); several rows are selected as
  *Selecting several tracks* says. With the list focused, the up and
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

- The preview player, the collection, search, sorting, the playlists, the played state and the
  history have their own sections below.
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

Sections of the public manual, read on 15 November 2026:

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
  library; while it runs, the line above the list counts the files looked at so far, and the
  key becomes *Stop import* (`library.import.stop`). At the end the line says how many tracks it
  added, how many it already knew and how many it could not read, and that it was stopped when
  it was. One import runs at a time.
- **Stopping an import.** The import stops before the next file it looks at. The tracks read
  until then stay in the collection; a later import of the same folder adds the rest, since it
  leaves known files as they are.
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

- The columns are chosen from a menu of the header (*Sorting*, *Columns*).
- The progress of an import is a count of the files looked at, not a share of the whole: the
  number of files of a folder and the folders inside it is only known once they are all read.
  The list of the collection waits for the import to end or stop.

### Edge cases

| Input | Result |
| --- | --- |
| The library file cannot be opened or is damaged | The collection says it is not available; the explorer works |
| Import while the collection is shown | Nothing happens: *Import* acts on a folder of the explorer |
| Import of a folder already imported | Known files stay as they are; the status line counts them |
| `library.import.stop` while no import runs | Nothing happens |
| An empty collection | The list says so |
| Loading a missing track | The deck says it cannot read the file |

## Search

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Basic usage tutorials*, section *Performing a Text Search*
- *Overview*, section *Browser*

The search field sits in the browser. As the user types, the list keeps the tracks whose tags
match, in the playlist selected or in the collection; Enter widens the search to the whole
collection.

### What Platter does

- **The field.** Above the list, between the path and the keys. It shows the text typed, or
  "Search" while it is empty, and a key that empties it while it holds text. A click or
  Ctrl+F (`browser.search.focus`) gives it the keyboard. Typed characters go in at its end and
  Backspace takes the last one away; Escape empties it and gives the keyboard to the list; the
  up and down arrows, Enter and Tab leave it for the list too. Every change of the text sends
  `browser.search.set` with the whole text, at most 200 characters.
- **What it searches.** The text is split into words at spaces; a row is kept when every word
  is found, ignoring case, in its title, artist or album, or in the genre of a track of the
  collection. In the collection the library searches all of it and returns at most 1000
  tracks, with how many matched in all (`docs/behavior/library.md`, *Search and sorting*). In
  a folder of the explorer the rows already read are filtered, the file name counting as a
  title; a folder is kept when its name matches.
- **One text for the browser.** The text stays when the list changes from the collection to a
  folder or back, and applies to what is shown. The selection goes to the first row kept.
- **Order of answers.** The library answers searches in the order they were asked and skips
  those a newer text has replaced; an answer for a text that is no longer the one in the field
  is dropped.

### Differences

- Enter does not widen the search: in the collection it already covers everything, and the
  playlists, where the difference would show, are not in the tree yet.
- The field has no cursor to move, no selection and no clipboard: text is typed and erased at
  its end. Input methods that compose characters are not supported.
- In the collection a track is found by its tags only: one without tags is not found by the
  name of its file, as it is in a folder of the explorer.
- The reference design also searches comments, tempo and key; the library does not keep
  comments or key yet, and tempo is not searched as text.

### Edge cases

| Input | Result |
| --- | --- |
| A text of spaces only | Everything is shown |
| Typing past 200 characters | The rest is not taken |
| A search with no match | The list says nothing matches |
| Escape with an empty field | The keyboard goes to the list |

## Sorting

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Overview*, section *Browser*
- *Working with Playlists*

The list can be shown sorted by a column; a playlist also keeps an order of its own, and only in
that order can its tracks be moved by hand.

Read again on 17 November 2026: *Working with Playlists*, sections *Sorting Tracks in
Playlists* and *Using the Color Picker*, and *Overview*, section *Browser*. A click on the
header of a column sorts by it and a second click turns the order round; the header of the
colour column sorts by colour. The order a playlist is sorted in is not kept when the
application starts again. The manual does not say how the columns are chosen; the design system
does: the columns can be shown and hidden from a menu of the header (`docs/design/DESIGN.md`,
*TrackList*, *Browser columns*).

### What Platter does

- **Sorting by a column.** Operating the name of a column at the top of the list sorts the
  list by it (`browser.sort`), ascending; operating the same name again turns the order round.
  The column sorted by shows a chevron after its name, up for ascending and down for
  descending. Every column can be sorted by except the comment and the last one, the format.
- **Columns.** The badge of a loaded deck, the cover and the title are always shown. The others
  are colour, artist, album, genre, tempo, key, time, rating, comment and format, in that
  order; all but genre and comment are shown at first. A secondary click on the header opens a
  menu with one entry per column, *Hide* and its name while it is shown, *Show* and its name
  while it is not; operating one shows or hides it (`browser.column.toggle`), which a shortcut
  of the keyboard file can send too. The title takes the width the hidden columns leave. With
  the format column hidden, a file that is missing or cannot be read is flagged at the end of
  its title. The choice is kept in the settings file, under `[browser]` as `columns`.
- **Colour.** Its column is a square of 8 pixels in the colour of the track; its header is an
  empty square of the same size, which sorts by colour. Ascending goes from the first colour to
  the eighth; tracks without a colour come last either way.
- **Rating.** Ascending goes from one star to five; tracks without a rating come last either
  way.
- **In the collection.** The library sorts the whole collection, so the first 1000 tracks shown
  are the first of that order (`docs/behavior/library.md`, *Search and sorting*): tracks
  without a value in the column come last, ties keep the order of the path, and text sorts by
  its letters with case ignored. A title that is missing sorts as missing, not by file name.
- **In a folder.** Its folders stay first, by name; its files sort by the column the same way,
  with a file without a title tag sorting by its file name. A file whose header has not been
  read yet sorts as one without a value, and moves into place when it is read.
- **One order for the browser.** The order stays when the list changes from the collection to
  a folder and back. Without a column chosen, the collection is in the order of the path and a
  folder in the order of the name. The selection stays on the same row when the order changes.

### Differences

- Columns cannot be resized or reordered: the menu of the header shows and hides them, in a
  fixed order. The design system also lets them be resized and moved.
- The design system starts with colour, title, artist, tempo, key, rating, time and comment.
  Platter starts with the columns the list showed before they could be chosen, album and
  format included and comment hidden, so the state of a file, *Missing* or *Can't read*, which
  the format column shows, is seen at first.
- The comment cannot be sorted by: the library keeps no form of it with case ignored.
- A file of the explorer has no colour, rating or comment: sorted by them, a folder keeps the
  order of the names.
- The order is not kept in the settings file: the browser starts unsorted.

### Edge cases

| Input | Result |
| --- | --- |
| Sorting an empty list | The chevron shows; nothing else changes |
| Tracks with the same value | The order of the path, or of the name in a folder |
| Sorting while a search is typed | The rows found keep the order chosen |
| Hiding the column sorted by | The order stays; the chevron shows again when the column does |
| A column name in the settings file that Platter does not know | Left out |
| No `columns` in the settings file | The columns shown at first |

## Playlists

The playlists of the library (`docs/behavior/library.md`, *Playlists and folders*) appear in the
tree and are shown in the list.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Working with Playlists*
- *Overview*, section *Browser*

Playlists live in a Playlists folder of the tree and can be grouped in folders of their own. A
playlist selected in the tree shows its tracks in the list, in its own order, and can be shown
sorted by a column; only in its own order are its tracks reordered by hand. A track can be in a
playlist more than once.

Playlists are managed from context menus, opened with the right button. On a playlist of the
tree the menu renames it, deletes it, empties it and removes its repeated entries; on the tracks
of a playlist shown in the list it takes them out of it. Each of these acts at once: the manual
names no question before deleting, emptying or removing. A playlist is created from the menu of
the Playlists folder, after a name is typed; tracks are added by dragging them onto a playlist,
at its end.

### What Platter does

- **The node.** Below *Collection*, the tree shows *Playlists* with the folders and playlists of
  the library under it, each level by name and indented one step more. A playlist carries the
  number of its entries. A folder has a chevron that closes and opens it
  (`browser.folder.toggle`); a closed folder hides what it holds. With the keyboard on its row,
  the left arrow closes it and the right arrow opens it. Folders start open, and which are
  closed is not kept when Platter starts again. Operating a folder or the *Playlists* row
  otherwise only gives it the keyboard, for its menu.
- **Showing a playlist.** Operating a playlist shows its tracks in the list
  (`browser.playlist.select`), in its own order, one row per entry: a track that is in it twice
  is listed twice. The line above the list names *Playlists* and the playlist and says how many
  tracks it holds. The search and the sorting of the list apply to it as to the collection; with
  no column chosen it keeps its own order.
- **The tree is read on the worker of the library**, once when the library opens. It shows at
  most 1000 items and 32 levels; deeper or later items are left out.
- Selecting, moving the selection and loading work in a playlist as in the collection.
- **New playlists and folders.** The menu of *Playlists*, or of a folder, offers *New
  playlist* and *New folder*. Each opens a name field in the tree, below *Playlists* or below
  the folder, that takes the keyboard; Enter with a name creates the item there
  (`browser.playlist.create`), without the spaces around the name. Escape, Enter with no
  name, or a press elsewhere close the field and create nothing. A new playlist is shown in
  the list at once; a new folder is not. Names are up to 200 characters; two items may share
  one.
- **Adding tracks.** A track dragged from the list onto a playlist of the tree goes at its
  end (`browser.playlist.add`); the playlist is outlined while the track is over it. With a
  playlist of the tree focused, Insert adds the track the list has selected. A file of a
  folder of the explorer that the collection does not hold yet is taken into it first, with
  the tags of its header (`docs/behavior/library.md`, *Importing one file*); a file that is
  not music, or cannot be read, adds nothing. A track can be added again: it is then in the
  playlist twice. Folders of playlists take no tracks.
- **Moving.** A playlist or a folder dragged onto a folder goes into it, and dragged onto
  *Playlists* goes to the top of the tree (`browser.playlist.move`); the folder is outlined
  while it is over it. *Move to*, in the menu of a playlist or a folder, opens in its place
  the list of the places it can go: the top of the tree and every folder but itself and those
  inside it. A folder never goes into itself or into a folder inside it.
- **Renaming.** *Rename*, in the menu of a playlist or a folder, puts the same field in its
  row, holding its name; Enter gives it the new one (`browser.playlist.rename`).
- **The menu of a playlist.** The right button on a playlist of the tree, or Shift+F10 while
  its row has the keyboard, opens its menu without showing it: *Rename*, *Move to*, then *Empty* takes every entry out
  (`browser.playlist.clear`), *Remove repeated tracks* keeps the first entry of each track
  (`browser.playlist.remove_repeats`) and *Delete playlist* deletes it
  (`browser.playlist.delete`). Each acts at once, as in the public manual. Deleting the
  playlist the list shows shows the collection, with the first row selected. `browser.playlist.delete` deletes playlists
  only: given a folder, it does nothing, since a folder goes with everything in it.
- **The menu of a row.** While a playlist is shown, the right button on a row selects it and
  opens a menu whose *Remove from playlist* takes that entry out (`browser.playlist.remove`);
  Shift+F10 opens it for the selected row, and Delete takes the selected entry out without
  the menu. The track stays in the collection. A track that is in the playlist twice loses
  only the entry selected. The selection stays in its place, or on the last row.
- **Menus.** A menu opens where it was asked for, inside the window, and takes the keyboard:
  Up and Down move, Enter or Space operates the item, Escape closes. The pointer takes the
  highlight when it moves over an item. A press outside closes it
  and still reaches what it lands on. Rows of the explorer have no menu.
- **Smart playlists** (`docs/behavior/library.md`, *Smart playlists*). The menu of *Playlists*,
  or of a folder, also offers *New smart playlist*, named in the same field
  (`browser.playlist.create` with `item=smart`); it starts without rules, so it holds the
  whole collection. It sits in the tree with the playlists, with the playlist glyph and the
  number of tracks it holds now, and is shown as a playlist is, in the order of the collection.
  Its menu offers *Edit rules*, which opens the dialog of its rules
  (`browser.smart.rules.open`), then *Rename*, *Move to*, *Save as playlist*, which makes a playlist beside it with its name and
  the tracks it holds now, at most 1000 (`browser.smart.to_playlist`), *Duplicate*
  (`browser.smart.duplicate`), *Delete playlist* and the export to the folder of the explorer.
  Tracks dropped on it are not taken, and the menu of its rows has no *Remove from playlist*.
- **The dialog of the rules** shows the name of the smart playlist, the line "A track must pass
  every rule." and one row per rule. Each row chooses its field (`browser.smart.rule.field`),
  which empties its value, and takes the value its field needs: words for the title, the
  artist, the album and the genre; *From* and *to* for the tempo in BPM, the length in seconds,
  the rating, the plays and the dates of import and of last play, written `YYYY-MM-DD` in UTC,
  either one empty; one of the eight colours or of the 24 keys, in the key notation of the
  preferences; *Yes* or *No* for a missing file and for an analysed track
  (`browser.smart.rule.choice`). A key at the end of the row takes it out
  (`browser.smart.rule.remove`); *Add rule* adds a row for the genre, up to 16
  (`browser.smart.rule.add`). A row whose value means nothing, such as a tempo of letters or a
  range turned round, is outlined in the warning colour, and *Save* waits until every row holds
  a rule; then it replaces the rules (`browser.smart.rules.set`) and closes the dialog.
  *Cancel* and Escape close it without saving (`browser.smart.rules.close`). The upper date
  takes its whole day. The dialog closes when its smart playlist is gone.
- **Changes run on the worker of the library**, in the order they were asked for. After each
  one the tree and the list are read again; a change that fails, such as on a playlist that is
  gone, changes nothing and leaves the library available.

### Differences

- Tracks are added one at a time, at the end; the public manual also drops several, and at
  a place inside the playlist. Insert is Platter's, for the keyboard.
- Entries cannot be reordered by hand in a playlist yet, and folders cannot be deleted from
  the window, since the store deletes a folder with everything in it. The store has both
  (`docs/behavior/library.md`).
- The public manual names a playlist in a dialog; Platter names it in a field in the tree,
  where the item goes, and keeps a dialog for what needs more room.
- Delete takes the selected entry out of the playlist shown. The public manual does it only
  from the menu; the key is Platter's, for the keyboard.
- The menus hold only what Platter does so far: no export, import, colours or played state.
- A smart playlist has no glyph of its own: the design system draws none. The public manual
  also opens the window of filters right after a smart playlist is created; Platter opens it
  from *Edit rules*.
- The design system names `browser.node.select` for showing a node of the tree. A playlist is
  shown with `browser.playlist.select` instead, because a command takes typed parameters and
  the choices of `browser.node.select` are fixed names, while a playlist is named by the number
  the library gives it.

### Edge cases

| Input | Result |
| --- | --- |
| Closing a folder that holds the playlist shown | The list keeps showing it; its row is hidden until the folder opens |
| A library without playlists | *Playlists* shows with nothing under it |
| An empty playlist | The list says the playlist is empty |
| The playlist shown is gone from the library | The list shows it empty and the tree is read again |
| A track of the playlist whose file is gone | Listed with "Missing", as in the collection |
| `browser.playlist.select` with an identifier that is not a playlist | As a playlist that is gone |
| `browser.playlist.delete` with a folder | Nothing happens; the folder and what it holds stay |
| Deleting the playlist the list shows | The list shows the collection |
| A change on a playlist that is gone | Nothing changes; the tree is read again |
| Delete while a folder or the collection is shown | Nothing happens |
| Emptying an empty playlist, or removing repeats from one without any | Nothing changes |
| A name of spaces only, or longer than 200 characters by command | Nothing is created or renamed; the field cuts typing at 200 |
| The folder or item of an open name field is gone | The field closes |
| A track dropped on a folder of playlists, or on the collection | Nothing happens |
| Insert with a folder row selected in the list | Nothing happens |
| A date that does not exist, such as `2026-02-30`, or a tempo of letters | The row is outlined and *Save* waits |
| A rule kept in the file that this version cannot read | Left out of the dialog; saving drops it |
| A track dropped on a smart playlist | Nothing happens |

## Played state

A track counts as played once a deck has played it for a while, and the list marks it.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Working with Playlists*, sections *History Playlists*, *Track Status Icons in the Track
  List* and *Resetting Played State of Tracks*
- *Preferences*, *Transport* page, setting *Min. Playtime*

A track counts as played once it has played for the minimum play time set in the preferences:
it then shows a check mark in the list, enters the history of the session and its play count
goes up. The played state of a track, and its play count, are reset from the menu of its row.
A track that is only previewed does not count. The manual gives no default for the time.

### What Platter does

- **When a track counts.** Each deck adds up the time it spends playing the track loaded in it,
  across pauses and jumps. When it reaches the minimum play time, the track counts as played,
  once for that load: loading it again starts a new count. The minimum is
  `browser.min_playtime` in the settings file, in seconds, from 5 to 600; 30 by default.
- **What counting does.** On the worker of the library, the track is taken into the collection
  if it is not there yet, as a track added to a playlist is (`docs/behavior/library.md`,
  *Importing one file*); its play count goes up and the session of this run records it, with
  its deck, when it started playing and the time it had played when it counted. The library
  opens that session when the window opens.
- **The mark.** A track of the collection or of a playlist played in this session shows its
  title in the secondary text colour with a check after it.
- **Reset.** The menu of a row of the collection or of a playlist offers *Reset played state*
  (`browser.played.reset`): the play count of the selected track goes to zero and it is no
  longer marked; its entries in the history stay.

### Differences

- The minimum play time is set on the Library page of the preferences
  (`docs/behavior/settings.md`), not on a Transport page, which Platter does not have yet.
- The rows of a folder of the explorer carry no mark: only the collection knows what was
  played.
- A track counts while its deck plays, whatever its fader and the crossfader say, as the
  public manual describes the time.

### Edge cases

| Input | Result |
| --- | --- |
| A track paused before the minimum and played on later | The time adds up; it counts when it reaches the minimum |
| The same track loaded again in the same deck | It can count again, and its play count goes up again |
| A file that cannot be taken into the collection | Nothing is recorded |
| The library is not available | Nothing is recorded; the decks play as always |
| Reset on a track that was never played | Nothing changes |

## History

The tree keeps what was played: the tracks of this session and those of the sessions before.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Working with Playlists*, section *History Playlists*

The tree has a history node with the tracks played in the current session. When the
application closes, the history is stored in a file, and the node starts empty at the next start.
The histories of earlier sessions, labelled with the date and time of their session, are in an
archive folder of the explorer (`docs/behavior/library.md`, *History*).

### What Platter does

- **The node.** *History* comes after the playlists, with the number of tracks played in this
  session. Operating it shows the history of this session in the list
  (`browser.node.select` with `history`).
- **Earlier sessions.** Under *History*, one row per earlier session where tracks were played,
  the latest first, labelled with the date and time it opened, in UTC (`2026-11-15 20:14`), and
  with the number of its tracks. Operating one shows its history (`browser.history.select`).
  The tree lists the latest 100 of them. While there are earlier sessions, *History* has a
  chevron that hides and shows them (`browser.history.toggle`), and the left and right arrows
  do the same with the keyboard on its row; they start shown.
- **The list of a history.** One row per track played, in the order they started playing, with
  the columns of the collection and the tags the track has now. A track played twice is listed
  twice. The line above the list names *History* and, for an earlier session, its label.
- **Search and sorting.** The search text filters a history and the columns sort it as they do
  a playlist (`docs/behavior/library.md`, *Search and sorting*); without a column the rows keep
  the order they were played in.
- **The rows.** They load, carry the played mark and offer *Reset played state* in their menu
  as rows of the collection do. A track that counts as played while the history of this session
  is shown appears in it at once: the list is read again after every change of the library.
- **The menu of a history.** *History* and each earlier session have a menu, by a secondary
  click or by the menu key while the row has the keyboard:
  - *Save as playlist* (`browser.history.to_playlist`) makes a playlist at the top of
    *Playlists*, named with the label of the session, with its tracks in the order they were
    played; a track played twice is in it twice, a track no longer in the collection is
    left out.
  - *Export to* and the name of the folder the explorer shows (`browser.history.export`) writes
    the history into that folder as an M3U file, named as the playlist would be, as a playlist
    is exported (*Playlist files*). It is offered while the explorer shows a folder.
  - *Delete* asks first, naming the session, then removes the history of that session from the
    library (`browser.history.delete`). The tracks stay in the collection with their play
    count. The history of this run cannot be deleted: its menu has no *Delete*. When the history
    deleted is the one the list shows, the list shows the collection.

Read again on 17 November 2026: *Working with Playlists*, sections *History Playlists* and
*Playlist Maintenance*. A history is stored as a playlist file in an archive folder of the
explorer when the application closes; the maintenance of playlists, renaming, removing tracks,
deleting and clearing, is described for playlists, and the manual does not say which of it
applies to a history.

### Differences

- Earlier histories are listed under *History*, not in a folder of the explorer: the library
  keeps them, not files of their own.
- Dates and times are in UTC: Platter has no source for the local time zone without a new
  dependency.
- The list has no column for the deck, the start time or the time played, which the library
  keeps; the columns chosen are those of the collection.
- A history cannot be renamed and a track cannot be removed from it: it is the record of what
  was played. *Save as playlist* gives a copy that can be named and edited.
- A track removed from the collection is left out of the list; the library keeps its entry, with
  the title and artist it had.
- The tree lists at most 100 earlier sessions.

### Edge cases

| Input | Result |
| --- | --- |
| No library | No *History* node |
| A run where nothing counted as played | Its session is not listed among the earlier ones |
| Nothing played yet in this session | *History* shows an empty list |
| A command naming a session the library does not have | An empty list |
| A track played in two earlier sessions | Listed in the history of each |
| *Save as playlist* when a playlist has that name | A second playlist of the same name; names need not be unique |
| *Export to* a folder that already has a file of that name | The name with ` (2)`, as for a playlist |
| `browser.history.delete` naming the session of this run | Nothing happens |
| A history all of whose tracks left the collection | An empty playlist, or an M3U file without entries |

## Preparation list

One playlist holds the tracks the DJ means to play next.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Working with Playlists*, section *Using Preparation Playlist*

Any playlist can be made the preparation list from its menu in the tree; its icon then changes.
The menu of tracks in the list appends them to the preparation list, or adds them as next: after
the track loaded last, inside the preparation list. A track in the preparation list shows a mark
in the first column of the list.

### What Platter does

- **Choosing it.** The menu of a playlist in the tree offers *Use as preparation list*
  (`browser.preparation.select`). The library keeps which playlist it is, one at most; choosing
  another moves it. In the tree that playlist carries a bookmark instead of the playlist icon.
- **The node.** *Preparation* comes after the playlists and before *History*, with a bookmark
  and the number of entries of the preparation list. Operating it shows the preparation list
  (`browser.node.select` with `preparation`); a track dragged onto it, or added with Insert
  while it has the keyboard, goes at its end. Without a preparation list it carries no count,
  is not operated and takes no track.
- **Adding to it.** While there is a preparation list, the menu of a row of the collection, a
  playlist or a history offers *Append to preparation list* (`browser.preparation.append`),
  which adds the selected track at its end, and *Add as next to preparation list*
  (`browser.preparation.next`), which adds it right after the entry of the track loaded last
  into a deck. When that track is not in the preparation list, or no track has been
  loaded, the track goes at its start, as the next to play.
- **The mark.** A row whose track is in the preparation list shows a bookmark in the first
  column, where a loaded track shows the letter of its deck; the letter wins.
- The preparation list is otherwise a playlist like any: it is shown, renamed, cleared and
  deleted as one. Deleting it leaves no preparation list.

### Differences

- The bookmark of the design system stands for the mark; the documented application draws its own
  icon.
- The rows of a folder of the explorer have no menu yet, so their tracks are added by dragging
  them onto the preparation list, and carry no mark: the explorer does not read the library.
- There is no way to stop using a playlist as the preparation list but to choose another or
  delete it.

### Edge cases

| Input | Result |
| --- | --- |
| No preparation list | The row menu has neither entry |
| A folder named by `browser.preparation.select` | Nothing changes |
| The track loaded last is twice in the preparation list | The new one goes after its last entry |
| A track already in the preparation list | It is added again; repeats are removed from the playlist menu |
| A library file from before the preparation list | It is brought up to date when opened, without a preparation list |

## Favourites

A row of chips above the tree reaches the sources used most with one press.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Overview*, section *Browser*
- *Managing Your Track Collection*, section *Assigning Playlists and Folders to the Favorites*

The favourites are at the top of the browser and give direct access to favourite playlists and
folders. Some come assigned: among them the preparation list, the history and the collection.
A playlist or a folder of the tree dropped on a free favourite is assigned to it; pressing the
favourite then shows it.

### What Platter does

- **The row.** Above the tree, eight chips in two lines (`docs/design/DESIGN.md`,
  *BrowserTree*). The first three are fixed: *Collection*, *Preparation* and *History*, each
  drawn with the icon of its node of the tree and named by it for the screen reader, so the
  places of the playlists have room for a name. The other five start free and each takes a
  playlist; a free one is drawn as an empty outline.
- **Using one.** Pressing a chip shows its source in the list, as its row of the tree does: the
  same commands (`browser.node.select`, `browser.playlist.select`). The chip of the source the
  list shows is drawn selected. *Preparation* does nothing while there is no preparation list.
- **Assigning.** A playlist of the tree dropped on a free chip goes to it, and the menu of a
  playlist offers *Add to favourites*, which puts it in the first free chip
  (`browser.favourite.set`, with the chip, 4 to 8, and the playlist). A playlist already in a
  chip is not offered again; with no free chip the entry is not shown.
- **Freeing.** The menu of an assigned chip, by the right button or Shift+F10, offers *Remove
  from favourites* (`browser.favourite.clear`). Deleting the playlist frees its chip.
- The library keeps the favourites, so they stay from one run to the next. A chip shows the name
  of its playlist, cut to fit.

### Differences

- Only playlists can be favourites yet: a folder of the playlists is not shown in the list, and
  the library does not keep the folders of the explorer. A folder dropped on a chip is ignored.
- The documented application has more fixed favourites for content Platter does not have.

### Edge cases

| Input | Result |
| --- | --- |
| A playlist dropped on an assigned chip | Nothing changes |
| A playlist dropped on a fixed chip | Nothing changes |
| `browser.favourite.set` with a chip outside 4 to 8 | Refused as an invalid parameter |
| The same playlist set in a second chip | It moves there; one chip per playlist |
| A library file from before the favourites | Brought up to date when opened, with every chip free |

## Deleting tracks

A track can be taken out of the collection from its row.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Managing Your Track Collection*, section *Deleting Tracks from the Track Collection*

The menu of tracks in the list deletes them from the collection. A security dialog asks first and
offers three ways: delete them from the collection and from every playlist; also remove the tags
the documented application wrote into the files; or also delete the files from the disk.

### What Platter does

- **Asking.** The menu of a row of the collection, a playlist or a history offers *Delete from
  collection*. It opens a second menu in its place that names the track and asks again; only
  its *Delete* entry deletes (`browser.track.delete`), and Escape or a press elsewhere keeps
  the track.
- **What goes.** The selected track leaves the collection and every playlist, as the library
  removes a track (`docs/behavior/library.md`, *Tracks*). Its entries in the history stay in the
  library, which keeps its title and artist, but the list of a history no longer shows it. The
  file is not touched.
- The list and the counts of the tree are read again; the selection stays on its place.

### Differences

- Only the first way of the documented application: Platter writes no tags of its own into the files
  yet, and deleting files from the disk is left to the system, where a mistake can be undone.

### Edge cases

| Input | Result |
| --- | --- |
| The track is loaded in a deck | The deck keeps playing it; if it then counts as played, the file enters the collection again as a new track |
| The track was already deleted, from another row of the same track | Nothing changes; the library stays available |
| A row of a folder of the explorer | It has no menu: the explorer shows files, not the collection |

## Collection maintenance

A dialog checks which files of the collection are gone, finds them again or takes them out.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Managing Your Track Collection*, sections *Checking Consistency of your Track Collection* and
  *Relocating Missing Tracks*

The consistency check looks for the file of every track of the collection and shows a report:
how many tracks there are, how many are missing and which ones, with a button to relocate the
missing tracks, one to remove them from the collection and one to analyse what needs it.
Relocating asks for a folder, which may be a top folder or the whole drive, and finds the
missing tracks in it.

### What Platter does

- **Opening it.** The Library page of the preferences has *Collection maintenance*
  (`library.maintenance.open`), which opens the dialog over the preferences, in the pattern of
  the dialog of the track details (`docs/design/DESIGN.md`). Close or Escape closes it
  (`library.maintenance.close`) and leaves the preferences open.
- **The report.** How many tracks the collection holds and how many are marked missing, the
  missing ones listed by path, the first 1000, and what the last action did.
- **Check consistency** (`library.check`) looks for the file of every track: a file that is gone
  marks its track missing and a file that came back clears the mark
  (`docs/behavior/library.md`, *Tracks*). It runs on the worker of the library like an import:
  the keys of the dialog are off until it ends.
- **Relocate** (`library.relocate`, with a folder) looks for the files of the missing tracks in
  the folder the explorer of the browser shows and in the folders inside it, by file name with
  case ignored; the key names that folder and is off when the list shows no folder of the
  explorer. A track is pointed at its new file only when the name is found once and no other
  missing track has it; everything the library knows about it stays. The report says how many
  were relocated, how many were not found and how many were left because the name was not
  unique. At most 200,000 files and 32 levels of folders are looked at.
- **Remove missing** asks first: the key turns into a question with the number of tracks, and
  only its *Remove* key takes them out of the collection and every playlist
  (`library.missing.remove`); *Keep them* leaves them. It acts on the tracks the last check
  marked, never by itself.

### Differences

- No tab for tracks to analyse and no Analyse key: the analysis of tracks runs when a deck
  loads them, not over the collection yet.
- The folder of a relocation is the one the explorer shows, chosen there, not in a dialog of
  its own: the design system asks file choices to use the in-app file selector.
- No check when the application starts.

### Edge cases

| Input | Result |
| --- | --- |
| A drive that is not plugged in | Its tracks are marked missing; they come back with the next check once it is plugged in, so removing them is left to the user |
| A file name found in two folders | The track is not relocated; the report counts it as not unique |
| Two missing tracks with the same file name | Neither is relocated |
| The path found belongs to another track | That track is left as it is |
| No missing tracks | Relocate and Remove missing are off |

## Playlist files

Playlists leave and enter the library as M3U files.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Working with Playlists*, sections *Exporting Playlists* and *Importing Playlists*

The menu of a playlist in the tree exports it: a name and a destination are asked for, and the
playlist file is written there, with a copy of its tracks. The menu of the Playlists folder, or
of a folder inside it, imports a playlist file: the playlist and all its tracks are added to the
collection. A playlist file dropped on the Playlists folder or on a folder of it is imported
too.

### What Platter does

The file format is the one of the library (`docs/behavior/library.md`, *M3U files*). File choices
use the explorer of the browser, as the design system asks: the folder it shows, or last showed,
is where files are written and read.

- **Exporting.** The menu of a playlist offers *Export to* with the name of that folder
  (`browser.playlist.export`, with the playlist and the folder). It writes the playlist, in its
  own order, to `<name of the playlist>.m3u8` there; characters a file name cannot hold become
  `_`. An existing file is never replaced: the name takes ` (2)`, ` (3)` and so on, up to 100.
  Every entry is written, those of missing tracks too, with the path the library has.
- **Importing.** The menu of *Playlists* and of a folder of the playlists offers *Import
  playlists from* with the name of that folder (`browser.playlist.import`, with the folder of
  the tree and the folder of files). Each `.m3u` and `.m3u8` file of that folder, not of the
  folders inside it, becomes a new playlist in the folder of the tree, named as the file without
  its extension, at most 100 files by name. Each entry whose file exists is taken into the
  collection when it is not there, as a track added to a playlist is
  (`docs/behavior/library.md`, *Importing one file*), and added in its order; the others are
  skipped.
- **The outcome.** The line above the list says what was written, or how many playlists and
  tracks were imported and how many entries were skipped. Both run on the worker of the
  library; the keys that import and export are off until it ends.
- Without a folder in the explorer, neither entry is in the menus.

### Differences

- The tracks are not copied with an exported playlist: the file names them where they are, so
  another computer finds them only at the same paths. Copying music is left to the system.
- The playlist format of the documented application is not written or read: M3U is the format other
  software shares.
- Dropping a playlist file on the tree does not import it yet.

### Edge cases

| Input | Result |
| --- | --- |
| A playlist whose name holds `/`, `:` or `?` | Those characters become `_` in the file name |
| A name that is empty once cleaned | The file is named `Playlist.m3u8` |
| 100 files with the name already | Nothing is written; the line above the list says so |
| A playlist file larger than 4 MiB | It is skipped and counted as not imported |
| An entry of another URL scheme or a broken path | Skipped and counted |
| An empty playlist | An empty file is written; an empty file read makes an empty playlist |

## Selecting several tracks

Some actions of the list take several tracks at once.

### What the public manual says

Sections of the public manual, read on 17 November 2026:

- *Managing Your Track Collection*, sections *Editing Track Metadata using the Edit Dialog*
  and *Editing Track Cover Artwork*

Several tracks are selected in the list and then edited together, or given or relieved of a
cover, from the menu of the selection. The sections do not say how a selection is made.

### What Platter does

The ways of selecting are Platter's own, the ones desktop lists share.

- **Selecting.** A click selects one row (`browser.select`) and makes it the current row. A
  click with Control (Command on macOS) adds the row to the selection and makes it current, or
  takes it out (`browser.select.toggle`); taking out the current row makes the first
  selected row current. A click with Shift selects every row from the current row to it
  (`browser.select.extend`); the current row stays. With the list focused, Shift and the up
  or down arrow move the far end of the selection by one row, the same command. Control and A
  selects every row of the list (`browser.select.all`), unless a text field or a dialog has
  the keyboard.
- **Showing it.** Every selected row has the colour of a selected row; the current row is
  the one the arrows move from.
- **What ends it.** The arrows, a plain click, a search, a sort and another folder or
  playlist leave the current row selected alone. Rows are selected by their track, never by
  their place: when the library answers with the list again, after a save for instance, the
  same tracks stay selected wherever they now are, and a track that is twice in a playlist is
  selected in both rows.
- **The menu.** The secondary click on a selected row keeps the selection; on another row it
  selects that row alone. With several rows selected, the menu offers only the entries that
  act on all of them.
- **What takes the selection.** *Edit details*, *Import cover* and *Delete cover* act on every
  selected track of the collection, a playlist or a history; a track that is twice in a
  playlist counts once. Every other action of the list (loading, the preview, colour, rating,
  played state, deleting, the preparation list, removing from a playlist) acts on the current
  row, as before, and is in the menu only while one row is selected.

### Differences

- Colour, rating and deleting act on the current row only, not on the selection.
- Selections are not kept: a change of the list ends them.
- A single row is added to the selection or taken out with the pointer only; from the
  keyboard a selection grows from the current row, or takes the whole list.

### Edge cases

| Input | Result |
| --- | --- |
| Shift click with no current row | Selects that row alone |
| Shift and an arrow past either end of the list | Nothing changes |
| Control click on the only selected row | The row stays selected: the selection is never empty while the list has rows |
| Control and A in an empty list | Nothing |
| A row index past the end in a command | Nothing changes |
| Selected rows of folders or files of the explorer | Left out of the actions that take tracks of the library |

## Colour and rating

A track of the collection carries a colour tag and a rating that the DJ gives it.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Working with Playlists*, section *Using the Color Picker*
- *Managing Your Track Collection*, sections *Editing Track Metadata in the Track List* and
  *Editing Track Metadata using the Edit Dialog*

The menu of a track gives it a colour, from a picker, or takes it away with *None*; the list can
be sorted by colour and searched by it. Rating and comment are properties of a track, edited in
the list or in the edit dialog of a track.

### What Platter does

- **Showing them.** The list of the collection, a playlist or a history shows the colour of a
  track as a square of 8 pixels in its own column before the title, in one of the eight hotcue colours of the
  design system, and its rating as five stars, filled for the rating, in a column after the time
  (`docs/design/DESIGN.md`, *TrackList*). A track without a colour shows no square; a rating of
  zero shows five empty stars.
- **Setting them.** The menu of a row offers *Colour*, which opens in its place the eight
  colours and *None* (`browser.track.colour.set`, 1 to 8, or 0 for none), and *Rating*, which
  opens six choices from no star to five (`browser.track.rating.set`, 0 to 5). Both act on the
  selected track.
- **Keeping them.** The library keeps colour, rating and a comment with each track; version 4
  of its file adds them, and an older file is brought up to date when opened, with every track
  at no colour, no rating and no comment. A new import leaves them as they were.
- The comment is kept but not shown or edited yet: it comes with the dialog of the details of a
  track.

### Differences

- The colours have numbers, not names: the design system gives the eight hotcue colours no
  names.
- The list is sorted by colour and by rating (*Sorting*); it is not searched by them.
- A colour or a rating is set for the current row, not for a selection of tracks (*Selecting
  several tracks*).

### Edge cases

| Input | Result |
| --- | --- |
| A colour above 8 or a rating above 5 in a command | Refused as an invalid parameter |
| A track removed from the collection | Its colour and rating go with it |
| A row of a folder of the explorer | No square and no stars: the explorer does not read the library |

## Track details

A dialog edits what the library knows of a track.

### What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Managing Your Track Collection*, sections *Editing Track Metadata in the Track List* and
  *Editing Track Metadata using the Edit Dialog* (read again on 17 November 2026)

Not every property of a track shows in the list; the edit dialog, opened with *Edit* from the
menu of a track, shows them all. *OK* or *Apply* keeps the changes, *Restore* undoes them and
*Cancel* closes the dialog without them. A selection of tracks can be edited at once: a ticked
box beside a field means the tracks share its value, and what is typed into a ticked field is
saved to every selected track; an unticked box means the values differ. *Previous* and *Next*
go through the selected tracks one by one. The section lists no fields and says nothing of
unticked fields or of changes not applied when moving.

### What Platter does

- **Opening it.** The menu of a row of the collection, a playlist or a history offers *Edit
  details* (`browser.track.details.open`), which opens the dialog over the window, in the
  pattern of `docs/design/screens/Track-details.html`, for the selected tracks. While it is
  open the window behind it takes no input.
- **The fields.** Title, artist, album, genre and comment as text, filled with what the
  library has, and the rating as six choices from no star to five. Above them, the name of the
  file and its folder, or, for several tracks, how many. Each text field takes what is typed
  while it has the keyboard; Tab goes to the next one.
- **Several tracks.** With more than one track selected, each field has a tick key before its
  name (`browser.track.details.tick`, with the field), lit while the field is ticked. A field
  whose value every selected track shares opens ticked and shows that value; one whose values
  differ opens unticked and empty. Typing into a field, or choosing a rating, ticks it.
- **What is saved.** A field is written when it is ticked and its text is not the one it
  opened with, or it opened unticked; for one track, when its text changed. The other fields
  stay as each track has them, in the library and in the file. Nothing to write saves
  nothing.
- **Saving.** *Save* (`browser.track.details.save`, with every field, the fields to write,
  the position and what follows) stores the fields in the library for each track it is for:
  an empty field clears that tag, and a text longer than 1000 characters is cut, as the
  library keeps tags. The list and the tree are read again, and the dialog closes. *Cancel*,
  Escape (`browser.track.details.close`) or the end of the track in the list close it without
  saving.
- **Previous and Next.** With several tracks, the dialog first shows them all, position 0;
  *Next* saves what it shows and shows the first selected track alone, in the order of their
  files (by folder, then by name), then the second; *Previous* goes back, to all of them from the first. Both are the
  save command with *previous* or *next* in place of *close*. The name of the shown position
  sits between them ("All 5", "2 of 5"). With one track they are not shown.
- The fields are written into the file as well, unless the Library page of the preferences
  says otherwise (`docs/behavior/tag-writing.md`, *When Platter writes*); the rating stays in
  the library. All the tracks of one save go to the worker of the library as one change.

### Differences

- No label, year or key fields, no choice of cover and no new detection of tempo or key: the
  library does not keep those yet, and the analysis runs when a deck loads a track. The cover
  has its own entries in the menu (`docs/behavior/cover-art.md`).
- No *Restore* and no *Apply* that keeps the dialog open: *Cancel* and opening the dialog
  again give the same, and *Previous* and *Next* save as they move.
- The public manual does not say what *Previous* and *Next* do with changes not yet applied;
  Platter saves them, so a change is never lost by moving.
- A field offers no list of the values the collection already has.
- The fields cannot be edited in the list itself.

### Edge cases

| Input | Result |
| --- | --- |
| The selected row is a folder or a file of the explorer | No *Edit details* in its menu |
| The track is deleted while the dialog is open | Saving changes nothing; the library stays available |
| A title made of spaces | Kept as typed; an empty field clears the title, and the list shows the file name |
| The selection changes while the dialog is open, by a controller | The dialog closes without saving |
| A position past the selection in a command | Nothing is saved; the dialog closes |
| An unknown name in the fields to write | Refused as an invalid parameter |
| A ticked field left empty for several tracks | That tag is cleared on every one |
| The list is sorted by a field the dialog edits, and a save moves the rows | The dialog stays open and each position names the same track: the order is that of the files, not of the list |

## Analysis in the background and keys

### What the public manual says

Sections of the public manual, read on 16 November 2026:

- *Overview*, section *Browser*, the status bar
- *Global concepts*, section *Harmonic mixing*, *Open Key notation* and *Key lock and
  color-coding*
- *Preferences*, *Analyze options page*, *Musical key*; *Browser details page*, *Key
  highlighting*; *File management page*, *Analyze new imported tracks*

The status bar of the browser shows the progress of the analysis of tracks, error messages
and the state of the application. Tracks can be analysed as they are imported. The key column
shows the key in the notation the preferences choose: musical, musical with sharps only, or
Open Key, which numbers the twelve keys round the circle of fifths with `d` for major and `m`
for minor. Tracks in the same key mix best; tracks of the same mode one number away also work,
and so does the other mode of the same number, with a change of mood. When the key of the
master deck is locked, the browser colours the keys that match it; a preference chooses
whether no key, only the matching keys, also the keys a semitone away, or every key is
coloured.

### What Platter does

- **Analysis in the background.** Once the library is open, the tracks of the collection that
  were never analysed are analysed one at a time on a worker thread of their own, the most
  recently imported first. Each result is kept as the result of a load is
  (`docs/behavior/library.md`, *Cues, grid and analysis*): the key and the loudness, and the
  tempo found with a marker on its first beat for a track whose kept grid is empty and
  unlocked. A track a deck has analysed counts as analysed.
- A file that cannot be read is counted as analysed with nothing found, and is not tried
  again until it is imported again. A track a deck holds is left to the deck.
- The analysis waits while a deck loads a track, so a load is never slowed by it, and stops
  when Platter closes; it goes on with the next track at the next start.
- **Status bar.** Under the list, one line: while the analysis runs, "Analysing tempo,
  beatgrid and key: 3 of 12 tracks", counting the tracks still to analyse when it started;
  otherwise the last problem of the library, if any; and on the right the selected track
  with its duration and format.
- **Key column.** The list of the collection and of a playlist shows the key of each analysed
  track. The notation is a setting of the Decks page of the preferences: musical with flats
  (`D♭ maj`, `A min`, the default, as the design system writes it), musical with sharps
  (`C♯ maj`), or Open Key (`1d` C major to `12d` F major round the circle of fifths, `1m` A
  minor to `12m` D minor).
- **Compatible keys.** The key the tempo master deck plays, its resulting key (with key lock
  and transposition), is the reference. The key of a row is compatible when it is the same
  key, the other mode of the same Open Key number, or the same mode one number away either
  way. A compatible key is drawn in `key-match` and its tooltip says so; the text of the key is
  always there. Without a tempo master playing a track with a key, nothing is marked.
- The key column sorts by Open Key number, then major before minor.

### Differences

- The documented application colours keys only while key lock is on in the master deck;
  Platter marks them whenever the master plays a track with a key, since the resulting key it
  plays is known with or without key lock.
- One way of marking, compatible keys in one colour, as the design system gives; the
  documented application has four levels and a colour per key.
- The key cannot be edited: it is what the analysis found.
- The design system gives a compatible key the tooltip "Compatible key"; the list has no
  tooltips yet, so the colour and the key name say it.
- The design system draws a thin progress bar under a row while it is analysed; the status
  bar says how far the analysis has gone instead.
- The words of the musical notation are translated (`maj` and `min` in English); the names of
  the notes stay letters in every language.
- The analysis of the collection runs whenever the library has tracks to analyse; there is
  no setting to turn it off and no command to analyse a selection again yet.

### Edge cases

| Input | Result |
| --- | --- |
| A track with no key found | Empty key cell, never marked |
| The master deck plays a track with no key found | Nothing marked |
| A track deleted while it is analysed | Its result is dropped |
| The library becomes unavailable during the analysis | The analysis stops; the status bar says why |

## Preview player

### What the public manual says

Sections of the public manual, read on 16 November 2026:

- *Overview*, section *Browser*, the preview player
- *Basic usage tutorials*, section *Previewing tracks in the browser*
- *Preferences*, *Output routing page*

The preview player plays a track without loading it into a deck. A track dragged onto it
shows its stripe; the play button starts and stops it and a click in the stripe moves the play
position. It plays through the output of the monitor, the headphones, in internal mixing mode,
and through an output of its own in external mode. Playing a track there changes neither its
play count nor its played state.

### What Platter does

| Command | Effect |
| --- | --- |
| `preview.load_selected` | The selected track of the list goes into the preview player and starts playing |
| `preview.load` | `path`: the same for a file, from a script or a mapping |
| `preview.play_pause` | Starts or stops the preview from where it is |
| `preview.seek` | `position` 0 to 1: moves the play position to that share of the track |

- The player sits in the toolbar of the list, as the design system draws it: play or pause
  with the headphones icon, a strip of the track in one colour, 200 by 20 pixels, with its
  play position, and the time played. A click in the strip moves the play position; dragging
  a row onto the player loads it.
- The track is decoded on a worker thread, as a deck loads one, and plays from its start at
  its own speed, without tempo, key lock or effects, through the headphones only, mixed with
  them whatever the mix knob says and at their volume. It stops at its end.
- It changes nothing of the track in the collection: no played state, no play count, no
  history, no analysis.
- Play and pause fade in and out over a few milliseconds, as a deck does.
- With a device of two outputs there are no headphones: the player is off and its tooltip
  says why, as the pre-listen keys of the mixer are.

### Differences

- The documented application has an output pair of its own for the preview in external mixer
  mode. Platter plays it through the headphones in internal mode only; in external mode the
  player stays silent.
- Dropping a track on the player loads it, as in the documented application; the menu of a
  row and Enter do not.

### Edge cases

| Input | Result |
| --- | --- |
| A file that is not audio | The player stays empty and the log says why |
| A seek past the end, or a position that is not a number | The position is clamped to the track, or refused |
| Another track loaded while one plays | The new one plays from its start |

## Recordings

The tree has a node for the files of the mix recorder.

### What the public manual says

Sections of the public manual, read on 19 November 2026:

- *Overview*, section *Browser*
- *Advanced operation*, section on recording the mix

The tree of the browser has a node for the audio recordings, after the explorer, which holds
the recordings made with the recorder of the application; the recorder writes them into the
folder chosen in its preferences. The sections read do not say what can be done with the
files of the node beyond what the browser does with tracks: loading them into decks.

### What Platter does

- **The node.** *Recordings* comes last in the tree, after the folders of the explorer, with the
  folder icon. It is there whenever the application has a folder for the recordings
  (`docs/behavior/recording.md`, *Files*), with or without a library, and whether or not the
  folder exists yet.
- **Operating it** shows the recordings folder in the list, as the explorer shows a folder
  (`browser.navigate` with the path of the folder): its audio files load into the decks, sort
  and search as the files of any folder do, and the tree opens down to it. The node is marked
  as the current one while the list shows that folder, as is the row of that folder in the
  explorer when the tree has it open.

### Differences

- The node is a way into a folder of the explorer, not a list of its own: Platter keeps no
  record of the recordings beyond the files, so a file moved or renamed outside Platter is
  shown as it is.
- The folder is fixed (`docs/behavior/recording.md`, *Differences*).

### Edge cases

| Input | Result |
| --- | --- |
| No recording made yet, so no folder | An empty list; browsing does not create the folder |
| No folder for the recordings (no home folder and no settings folder) | No *Recordings* node |
| A recording finished while the list shows the folder | Its file appears the next time the folder is read, as in any folder of the explorer |

## Tests

`crates/ui-egui/tests/files.rs` for listing, selection, loading and the tree, against folders
the test writes; `crates/ui-egui/tests/collection.rs` for the collection, the playlists and their changes, the
played state,
the search and sorting, the history, the preparation list, the favourites, deleting tracks, the maintenance of the collection, playlist files, colour and rating, the selection of several tracks and the details of a track, against a library and folders the test writes; `crates/ui-egui/tests/search_field.rs`
and the unit tests of `crates/ui-egui/src/widgets/field.rs` for the search field;
`crates/ui-egui/tests/playlist_menu.rs` and the unit tests of
`crates/ui-egui/src/widgets/menu.rs` for the context menus;
`crates/ui-egui/tests/panels.rs` for the controls and the keys of the list;
`crates/ui-egui/tests/tree_folders.rs` for the folders of playlists, *History* and *Recordings* in the tree;
`crates/ui-egui/tests/keys.rs` and the unit tests of `crates/ui-egui/src/keys.rs` for the key
column, its notations and the compatible keys; the unit tests of
`apps/platter/src/background.rs` for the analysis in the background. `crates/engine/tests/preview.rs`
for the preview player in the engine, and `crates/ui-egui/tests/panels.rs` for its controls.
