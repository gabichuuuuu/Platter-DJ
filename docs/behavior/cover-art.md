# Cover art

A track file can carry pictures in its tags, and one of them is usually the front cover of the
record. This note covers how Platter finds that picture, turns it into pixels small enough
for the browser and shows it in the list of tracks.

## What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Managing Your Track Collection*, section *Editing Track Cover Artwork*
- *Overview*, section *Browser*

The browser has an area, Cover Artwork, that shows the cover of the track selected in the
track list.

A cover is imported for one track or a selection of tracks from a menu of the track list, by
choosing an image file; the section names JPEG files. The cover is then stored in the track
files themselves. Another entry of the same menu deletes the cover of the tracks selected. The
section states no size or resolution for the picture. Read again on 17 November 2026: the
section names only JPEG covers, for importing and for deleting, and does not say what happens
to a picture already in the file.

## What Platter does

`platter-decode` reads the cover of a file with `cover_file` (or `cover_bytes` for a file held
in memory) and returns it decoded, or nothing when the file carries no picture.

- **Where the picture comes from.** The tags the decoding library reads: the attached-picture
  frames of ID3v2 (in front of MP3 data, or in the ID3 chunk of an AIFF file), the picture
  blocks of FLAC, the picture comments of Vorbis comments (`METADATA_BLOCK_PICTURE`, in FLAC
  and in Ogg Vorbis) and the cover items of an M4A file. Every set of tags in the file is
  looked at, not only the last one.
- **Which picture.** A picture marked as the front cover wins. Without one, the first picture
  found is used. The decoding library marks every cover item of an M4A file as a front
  cover.
- **Image formats.** JPEG and PNG, recognised by their content; the type the tag declares is
  ignored. A PNG keeps its transparency.
- **Size.** The picture is scaled down to fit in 256 by 256 pixels, keeping its proportions; a
  smaller picture keeps its size. Each pixel of the result is the average of the pixels of the
  picture it covers, weighted by their opacity so that transparent pixels do not darken the
  edges. The result is 8-bit RGBA, rows from the top, alpha not premultiplied.
- **Bounds.** A picture larger than 16 MiB as stored, or that declares more than 4096 pixels on
  a side, is refused before its pixels are decoded.
- **Work.** Reading a cover opens the file and reads its tags; it decodes no audio. It is slow
  work for a worker thread, like reading the header of a file.

### In the list of tracks

- Every row of a track has a cell of 18 by 18 pixels after the letter of its deck, as the list
  of the design system has (`docs/design/screens/First-version.html`). It shows the cover of
  the file, or the icon of an audio file when the file has none, cannot be read or its cover
  is still being read.
- The covers are read on a thread of their own, one file at a time, for the first 256 rows of
  the list shown, in the order of the list, whether it shows a folder, the collection, a
  playlist or a history. A row further down shows the icon.
- The covers read are kept for the rows of the list, up to 1024 files; past that they are read
  again when shown. A file is read once while it stays kept, even if it is in several lists.

## Importing and deleting a cover

The menu of a track of the collection, a playlist or a history offers both, for the tracks
selected in the list (`docs/behavior/browser.md`, *Selecting several tracks*).

- **Choosing the picture.** *Import cover* opens in its place the pictures of the folder the
  explorer shows or last showed: its JPEG and PNG files (`.jpg`, `.jpeg`, `.png`, in any
  case), by name, at most 20, as many as a menu shows in the window; folders inside it are
  not looked at. File choices in Platter
  use the explorer, as for playlist files. Choosing one sends `browser.track.cover.import`
  with its path. Without a folder in the explorer, or without pictures there, the entry is
  not in the menu.
- **Deleting.** *Delete cover* (`browser.track.cover.delete`) removes every picture of the
  selected tracks.
- **Writing.** Both run on the worker of the library, through the writer of tags
  (`docs/behavior/tag-writing.md`, *Covers*). The picture file is read once, at most 8 MiB,
  checked with its size before it is read, and refused unless Platter's reader would show it;
  then each selected track is written, a track that is twice in a playlist once. A track
  whose file cannot be written keeps its cover, and the log says why, as for other tags.
- **The setting.** A cover lives only in the file. When the Library page of the preferences
  says to write nothing or only Platter's own field, neither entry is in the menu, and both
  commands, from a controller or the command line, change nothing.
- **Showing the change.** The covers kept for the rows of the changed tracks, and for the
  decks that hold them, are dropped and read again, so the list and the decks show the new
  picture, or the icon of an audio file.

## Differences

- The picture to import is chosen in the explorer of the browser, not in a dialog of the
  system.
- PNG pictures can be imported, not only JPEG: Platter shows them.
- Importing keeps the pictures that are not the front cover; deleting removes them all.
- A WAV file takes the cover into its ID3 chunk, which other software reads; Platter's own
  reader does not, so the list keeps the icon for it.
- The public manual shows the cover of the selected track in an area of the browser; Platter
  shows the cover of each track in its row, as its design system draws the list, and the cover
  of the track a deck holds in the header of the deck, 54 pixels on a side after its letter,
  or a frame with the icon of an audio file while it has none.
- PNG pictures are read as well as JPEG.
- The decoding library does not read the ID3 chunk of a WAV file, so a WAV file shows no
  cover, even with a picture in that chunk.
- Other image formats a tag may hold (GIF, BMP, WebP) are not decoded.

## Edge cases

| Input | Result |
| --- | --- |
| File without any picture | No cover, no error |
| Several pictures, the front cover not first | The front cover |
| Several pictures, none marked as front cover | The first one |
| Picture in a format other than JPEG or PNG | Error "unsupported picture" |
| Damaged picture | Error "the picture is damaged" |
| Picture over 16 MiB as stored | Error "the picture is too large"; it is not decoded |
| Picture that declares more than 4096 pixels on a side | Error "the picture is too large"; no pixels are decoded |
| Very narrow picture (4096 by 1) | Scaled to 256 by 1; no side goes below one pixel |
| Audio file that cannot be opened or read | The error of the audio file (`docs/behavior/formats.md`) |
| Picture to import over 8 MiB, damaged, or of another format | Refused before any track is written; the log says why |
| Picture file removed between the menu and the import | Refused the same way |
| Selection with tracks that are missing | Those tracks are named as not written; the others take the cover |
| An import or a delete while the setting keeps tags out of the files | Nothing is written and nothing is logged |
| No selected track of the library (the explorer shows a folder) | Nothing |
| The decoding library or an image decoder panics on the content | Error, as for a damaged file (`docs/decisions/0008-decoder-panic-boundary.md`) |

No input makes reading a cover panic.

## Tests

`crates/ui-egui/tests/covers.rs` for the reading of the covers of a list, against files the test
writes, for the pictures of the explorer, and for importing, refusing and deleting a cover
from the browser, after which the list and the deck read it again.
`crates/ui-egui/tests/playlist_menu.rs` for the entries of the menu and when they are absent.

`crates/decode/tests/tag_write.rs` for importing and deleting covers: after each, Platter's own
reader returns the new picture, or none, from FLAC with a picture block, with a
`METADATA_BLOCK_PICTURE` comment and with an ID3v2 tag in front, MP3 and AIFF.

`crates/decode/tests/cover.rs`. The pictures are built in the test: PNG files written by hand,
and baseline JPEG files from a small encoder of flat 8 by 8 blocks. They are embedded in FLAC
files synthesised in the test as a picture block and as a `METADATA_BLOCK_PICTURE` comment, and
in an ID3v2 tag placed in front of a FLAC file, in front of silent MP3 frames and inside an AIFF
file. Reading a cover decodes no audio, so frames without audio bits are enough for MP3. The
tests check the decoded size and pixels, the scaling, the choice of the front cover, damaged
and oversized pictures and a file without a picture.

M4A and Ogg Vorbis need an encoder to produce a valid file, and the project has none. For those
two the reading of the picture was checked in the source of the decoding library, not by a
test; Ogg Vorbis shares the Vorbis comment reader tested through FLAC.
