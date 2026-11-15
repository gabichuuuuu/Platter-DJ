# Library: collection, playlists and history

The library remembers the tracks a DJ has imported, the playlists made from them and what was
played in each session. This note covers the store behind it, `platter-library`: what it keeps,
the operations on it and the import of folders. The browser shows the collection and imports
folders into it (`docs/behavior/browser.md`, *The collection*); playlists and history have no
place in the window yet. How the store is kept on disk is in `docs/decisions/0022-library-store.md`,
and how folders are imported in `docs/decisions/0023-folder-import.md`.

## What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Managing your track collection*, sections *About the Track Collection*, *Adding Music from
  Other Folders*, *Checking Consistency of your Track Collection*, *Relocating Missing Tracks*
  and *Deleting Tracks from the Track Collection*
- *Working with Playlists*
- *Basic usage tutorials*, section *Performing a Text Search*
- *Overview*, section *Browser*
- *Preferences*, the *File Management* page

The collection holds the imported tracks with their tags and what the application adds to
them. Folders can be added to a list and their music imported, also at start. A track whose
file has moved or gone is shown as missing, and can be pointed at its new place; a consistency
check reports the missing tracks and those still to analyse. A track can be removed from the
collection, which removes it from every playlist.

Playlists are made in a playlists folder and can be grouped in folders of their own; they are
renamed and deleted from their menus. Tracks dragged onto a playlist go at its end, or at the
place they are dropped. A playlist keeps the order tracks were put in; it can be shown sorted by
a column, and reordered by hand only in its own order. A track can be in a playlist more than
once, and a command removes the repeats; another clears the playlist; tracks are removed from it
one by one or as a selection.

The history lists the tracks played in the session, with when each one started, how long it
played and on which deck. It is saved when the application closes and the next session starts
a new one; past sessions are kept, labelled by date and time. A played track is marked as
played, and its played state and play count can be reset. A previewed track does not count.

The search field filters the list shown, or the whole collection, by the tags of the tracks as
the user types.

## What Platter does

### Tracks

- A track is known by the path of its file: one record per path. Importing a path again
  updates its tags and keeps its playlists, history, play count and import date.
- A record keeps title, artist, album, genre, duration, sample rate and tempo, each optional,
  plus the date it was imported, how many times it was played, when it was last played and
  whether its file is missing. Key, beatgrid and cues come with the analysis that writes them,
  later in M3.
- A path must be valid Unicode; Platter cannot store one that is not and says so. Paths are
  stored as they are given: the importer gives absolute, canonical paths, so one file has one
  path.
- The consistency check looks for each file and marks the tracks whose file is gone as missing,
  and those whose file came back as present. A missing track can be relocated to another path,
  keeping everything the library knows about it; relocating onto the path of another track is
  refused.
- Removing a track removes it from every playlist. Its history entries stay, with its title
  and artist as they were, since the history tells what was played.

### Importing folders

- A folder is imported with the folders inside it, down to 32 levels. Each folder gives its
  files first, sorted by name with case ignored, then its folders in the same order; the tracks
  get the import date of the pass and enter the collection in that order.
- A file the collection already holds is left as it is: its tags are not read again, so a pass
  over a large folder costs little and keeps what the user changed. Only new files are read.
- Hidden entries, whose name starts with a dot, are not looked at. Links, junctions included,
  are not followed, so a link back up the tree cannot make the import go round. A folder gives
  at most its first 10 000 entries, as in the file list of the browser; the rest are counted as
  left out.
- The reader the application gives decides what each file is: a track with its tags, a file that
  cannot be read, or something that is not audio, which is left out without a word.
- The pass reports how many tracks it added, how many files were already there, how many could
  not be read and how many entries it left out: links, names that are not valid Unicode,
  folders too deep or that cannot be read, entries past the 10 000 of a folder.
- A folder is stored under one path however it was named: the path is made absolute and
  canonical, and on Windows it keeps the form the user knows, `C:\Music`, without the prefix
  `\\?\` of the system.
- Tracks are written in batches of 500, each in one transaction. When the library fails half
  way, the batches written before stay.

### Playlists and folders

- Playlists and folders form one tree under the root. A folder holds playlists and folders; a
  playlist holds tracks. Names are from 1 to 200 characters; two items in the same folder may
  have the same name.
- An item can be renamed, moved to another folder and deleted. Deleting a folder deletes what
  it holds. A folder cannot move into itself or into a folder inside it.
- A playlist is an ordered list of entries. Each entry points to a track; the same track can
  have several entries. Tracks are added at the end or at a position, entries are removed one
  by one or as a set, an entry moves to another position, the repeats of a track are removed
  keeping its first entry, and the playlist can be cleared.
- Sorting a playlist by a column is a way of showing it; its own order does not change.

### History

- A session is opened by the application when it starts and holds the tracks played in it, in
  order: the track, its title and artist at that moment, the deck, when it started and how
  long it played. Earlier sessions stay, each with the time it was opened.
- The application decides when a track counts as played; the store records it and adds one to
  the play count of the track and sets when it was last played. The rule comes with the engine
  wiring.
- Resetting the played state of a track sets its play count to zero and forgets when it was
  last played; its history entries stay.

### Search and sorting

- The search text is split into words. A track matches when every word is found, as part of a
  word or whole, in its title, artist, album or genre. Matching ignores case for every
  alphabet, not only for English letters. An empty search matches every track.
- Search runs over the collection or over one playlist, then sorts by one column, ascending or
  descending: title, artist, album, genre, tempo, duration, import date, play count or, in a
  playlist, its own order. Tracks without a value in the column come last. Ties keep the order
  of the path. Text sorts by its letters with case ignored, in the order of their code points:
  "é" comes after "z". Folders and playlists in the tree sort the same way.
- Search ignores case, not accents: "ecole" does not find "école".
- A search returns at most a page of results with how many matched in all.

### M3U files

The public manual (*Working with Playlists*, sections *Exporting Playlists*, *Importing
Playlists* and *Editing Smartlists*) exports a playlist with its tracks for another computer and
imports playlist files into the collection; M3U is one of the formats it names. Platter reads and
writes M3U, the plain list of paths most DJ and media software understands. This part is the
format alone; the commands that import a file as a playlist and export a playlist come with the
playlists of the browser.

- **Reading.** A file larger than 4 MiB is refused. The text is UTF-8 when it is valid UTF-8,
  with or without a byte order mark, and Latin-1 otherwise, the usual encoding of old `.m3u`
  files. Lines may end in LF, CR LF or CR. Blank lines and lines starting with `#` are not
  entries. An `#EXTINF:<seconds>,<display>` line describes the next entry line, even across
  blank and comment lines, and only that one; a second `#EXTINF` replaces the first. Seconds
  are kept when they are a finite number above zero (`-1` means unknown); attributes after
  the seconds are ignored. The display splits at its first " - " into artist and title, and is
  all title without one.
- **Paths.** An entry is trimmed. A relative path is put under the folder of the playlist file,
  with `/` and `\` both taken as separators on every system. An absolute path (`/...`,
  `\\server\...`, `C:\...` or `C:/...`) is kept as written. A `file://` URL is decoded,
  `%XX` escapes included; another host than `localhost` gives a network path. Lines with
  another URL scheme (`http://` and the like), a broken escape, a drive without a separator
  (`C:a.mp3`), a NUL character or nothing after trimming are skipped and counted. At most
  100 000 entries are read; the entry lines past them are counted as skipped. Whether each file
  exists is for the caller to check.
- **Writing.** The extended form in UTF-8 with LF line ends (the `.m3u8` convention): `#EXTM3U`,
  then for each entry an `#EXTINF` line, with the length in whole seconds or `-1` and
  "artist - title", and its absolute path as the system writes it. A line break inside a title,
  an artist or a path becomes a space, so it cannot split the file.

### The file

- The store is one file. A file that is not a library, is damaged, or was written by a newer
  version of Platter is refused with an error, and is not changed. A file that another program
  holds open for writing is an error after a short wait.
- Every change is written at once, in one transaction: a change is all there or not there.

## Differences

- The public manual does not say how several words are searched. Platter asks for all of
  them.
- The public manual sorts the collection into views by artist, release, label and genre.
  Platter gives the same through sorting and search; the views come with the browser.
- The public manual does not say what happens to the history when a track is removed.
  Platter keeps the entries, with the title and artist they were played with.
- The public manual imports music folders from a list in the preferences, also at start.
  Platter's import pass works on one folder; the list and the import at start come with the
  settings of the browser.
- The public manual includes the folders inside a folder in the import and updates the
  collection, without saying whether the tags of tracks already in it are read again. Platter
  leaves those tracks as they are; importing one file again updates its tags.
- The genre is not read yet: the decoder reads title, artist, album and tempo. It comes with the
  reader of tags.
- The public manual also imports and exports playlists in a format of its own. Platter does
  not read or write it: M3U is the format other software shares. Smart playlists are M4.
- An exported M3U holds absolute paths and no copy of the tracks; another computer finds them
  only at the same paths. A track with an artist and no title is written as its artist and
  reads back as a title.
- The public manual marks one playlist as a preparation list. That comes with the browser.

## Edge cases

| Input | Result |
| --- | --- |
| A search with `%`, `_`, quotes or SQL | Matched as text, nothing else |
| A search for "É" | Finds "é" and "É" |
| A tempo, a duration or a sample rate that is not a finite positive number | Stored as none |
| A tag longer than 1000 characters | Cut at 1000 characters |
| A track added to a playlist that does not exist, or a track that does not exist | An error; nothing changes |
| Moving an entry to a position past the end | It goes to the end |
| Removing the repeats of a playlist without repeats | Nothing changes |
| Deleting a folder that holds folders | Everything inside goes |
| Relocating a track onto the path of another one | An error; nothing changes |
| Importing a folder that does not exist, or a file | An error; nothing changes |
| Importing a folder twice, or by another spelling of its path | The second pass adds only new files |
| A link to a folder above, inside the folder imported | Left out; the import ends |
| A file that is not a library | An error; the file is left as it is |
| A library from a newer version of Platter | An error; the file is left as it is |
| An M3U file of more than 4 MiB | Refused |
| An M3U entry that is a web address | Skipped and counted |
| An `#EXTINF` line followed by no entry | Ignored |

## Tests

`crates/library/tests/store.rs` on libraries in temporary directories, with invented tracks:
the records, the consistency check and relocation, playlists and folders, history, search and
sorting, and damaged, foreign and newer files. `crates/library/tests/playlist_model.rs` checks
the operations on a playlist against a list in memory with random sequences.
`crates/library/tests/folders.rs` imports folders made in a temporary directory, with empty
files and a reader that takes the extension for the format: the order, hidden entries, a second
pass, another spelling of the path, a missing folder, the depth limit, a folder of 1 234 files
and a batch that fails; links on Linux and macOS, and names that are not Unicode on Linux.
`crates/library/tests/m3u.rs` reads and writes M3U text made in the test: paths, encodings,
`#EXTINF`, URLs, the limits and a round trip.
