# Writing tags

The tags of a track (title, artist, album and the rest) live in the track file. This note
covers how Platter writes changed tags back into the file, and when.

## What the public manual says

Sections of the public manual, read on 15 November 2026:

- *Managing Your Track Collection*, section *Editing Track Metadata in the Track List*
- *Managing Your Track Collection*, section *Editing Track Metadata using the Edit Dialog*
- *Managing Your Track Collection*, section *Editing Track Cover Artwork*
- *Preferences*, *File Management page*, *Tag writing mode*; *Analyze Options page*, *Musical
  key*, *Written to File Tags* (read on 16 November 2026)

A track file can carry properties such as the title, the name of the artist, the name of the
album and the cover artwork, and the documented application edits them in two ways: in place,
in a cell of the track list, confirmed with Enter and abandoned with Escape; or in the Edit
dialog, which reaches the properties the list does not show.

The Edit dialog edits one track or a selection of tracks. For a selection, a ticked box beside
a field means the tracks share that value, and whatever is written into a ticked field is
saved to the metadata tag of every selected track when the changes are applied. An unticked
box means the values differ. Apply or OK saves; Restore undoes the changes made in the dialog;
Cancel closes it without saving. Each field offers the values already stored in the
collection.

An imported cover is stored within the track files.

The sections name no format, field length or failure case.

A preference chooses what the application writes into the files: no tags at all, only tags of
its own, or every tag. The key is written in a notation the preferences choose.

## What Platter does

### When Platter writes

A setting of the Library page of the preferences, *Tag writing*, chooses what Platter writes
into the files (`library.tag_writing.set`):

| Mode | What is written |
| --- | --- |
| Everything, the default | The tags the details dialog saves; the tempo of the grid and the key the analysis found; Platter's own field |
| Platter's own field only | The beatgrid and the stored cues, in the field of Platter's own |
| Nothing | No file is changed |

- **Details.** Saving the details of a track writes its title, artist, album, genre and
  comment into the file as well as into the collection (`docs/behavior/browser.md`, *Track
  details*). The rating is kept in the collection only.
- **What a deck or the analysis learned.** Each time the collection keeps the grid and the
  cues of a track (`docs/behavior/library.md`, *Cues, grid and analysis*), Platter's own field
  takes the text of the track, and the tempo field the tempo of the grid; each time it keeps
  the key the analysis found, the key field takes it, as ID3 writes a key: the letter of the
  note, `b` for a flat and `m` for a minor key (`Dbm`), whatever notation the browser shows.
- The writing runs on the worker of the collection, or on the worker of the analysis in the
  background, never on the interface thread. A file that cannot be written, a read-only one or
  one of a format Platter does not write, keeps what it had; the collection keeps the change
  all the same, and the log says why the file did not.
- A file a deck holds can be written: the deck plays the audio it decoded into memory.

`platter-decode` writes with `write_tags(path, &TagEdit)`. A `TagEdit` holds one entry per
field; an entry left out (`None`) leaves the field of the file as it is.

| Field | Set with | Cleared with |
| --- | --- | --- |
| Title, artist, album, genre, comment | `Some(text)` | `Some(String::new())`, or text of spaces only |
| Musical key | `Some(text)` | the same |
| Tempo | `Some(Some(bpm))` | `Some(None)` |
| Platter's own data | `Some(Some(text))`, even empty | `Some(None)` |

- **Own code.** The writer is Platter's own code, one module per container under
  `crates/decode/src/tag_write/`, written from the public specifications cited below. It uses
  no tagging library: one was tried and left out because it brought an unmaintained crate.
- **Text.** Spaces at both ends are dropped. Control characters other than line feed and
  tab are dropped, because a NUL separates values in ID3v2 text frames. Text longer than
  1000 characters is cut there, the same bound the library keeps
  (`platter-library`, `MAX_TAG_CHARS`).
- **Platter's own data** (stored cues and the beatgrid, for the callers that come later) goes
  into a field of its own that other software ignores, a decision of the maintainer. It is
  opaque to the writer: UTF-8 written exactly as given, line feeds and every other character
  included, up to 64 KiB (`MAX_PLATTER_BYTES`). Longer data is refused with
  `PlatterDataTooLong` before the file is opened.
- **Tempo.** Accepted from 20 to 999 beats per minute, the range the reader accepts; any other
  value, and a value that is not a finite number, is refused before the file is opened. A
  Vorbis comment (`BPM`) keeps two decimals. ID3v2 (`TBPM`) holds a whole number by its
  specification and gets the value rounded. In M4A the tempo item `tmpo` gets the rounded
  value as a 16-bit big-endian integer of data type 21.
- **Rating.** Not written: it has no field shared by the formats (ID3v2 has the `POPM` frame
  with a scale of its own, the others none in their specifications), so the library keeps it.

### Where each field goes

| Field | ID3v2.3 / 2.4 (2.2) | Vorbis comment | M4A `ilst` | WAV `INFO` | AIFF chunk |
| --- | --- | --- | --- | --- | --- |
| Title | `TIT2` (`TT2`) | `TITLE` | `©nam` | `INAM` | `NAME` |
| Artist | `TPE1` (`TP1`) | `ARTIST` | `©ART` | `IART` | `AUTH` |
| Album | `TALB` (`TAL`) | `ALBUM` | `©alb` | `IPRD` | |
| Genre | `TCON` (`TCO`) | `GENRE` | `©gen` (a `gnre` item is removed) | `IGNR` | |
| Comment | `COMM` (`COM`) without description | `COMMENT` | `©cmt` | `ICMT` | `ANNO` |
| Key | `TKEY` (`TKE`) | `INITIALKEY` | `----` `com.apple.iTunes` / `initialkey` | | |
| Tempo | `TBPM` (`TBP`) | `BPM` | `tmpo` | | |
| Platter's data | `TXXX` (`TXX`) described `PLATTER` | `PLATTER` | `----` `org.platter-dj` / `PLATTER` | | |

An empty cell means the format has no place for the field; WAV and AIFF then use an ID3v2
tag in a chunk (below).

### Each format

- **MP3.** The ID3v2 tag at the start is edited, or created as ID3v2.4 when the file has none
  and the edit sets a field. A tag keeps the version it was found in (2.2, 2.3 or 2.4), so the
  frames the edit does not name pass through byte for byte, compressed and encrypted ones
  included; converting would rewrite their headers. New text is UTF-8 in 2.4, and ISO-8859-1,
  or UTF-16 with a byte order mark when the text needs it, in 2.2 and 2.3. A new frame takes
  the place of the first frame it replaces, or goes at the end. An ID3v1 tag at the end has
  its title, artist, album and comment rewritten: ISO-8859-1, cut at 30 bytes (28 for the
  comment of ID3v1.1, whose track number stays), other characters as `?`; its year and genre
  stay. Sources: <https://id3.org/id3v2-00>, <https://id3.org/id3v2.3.0>,
  <https://id3.org/id3v2.4.0-structure>, <https://id3.org/id3v2.4.0-frames>,
  <https://id3.org/ID3v1>.
- **Comments in ID3v2.** Only comment frames without a description are the comment of the
  track. A comment with a description is data of other software and stays, and so does a
  comment frame that is compressed or encrypted, whose description cannot be read. A new
  comment keeps the language of the one it replaces, or `und` (undetermined, ISO 639-2).
  Platter's data replaces only the user text frames described `PLATTER`.
- **ID3v2 structure.** Unsynchronisation is undone on reading (on the whole tag before 2.4,
  frame by frame in 2.4) and the tag is written plain, with the flags cleared. The extended
  header is not written back, because its CRC would no longer match the frames; nor is a 2.4
  footer, which forbids padding. When the new tag fits the space of the old one, the rest is
  zero padding and the audio does not move; otherwise the tag is followed by 1024 bytes of
  padding.
- **FLAC.** The `VORBIS_COMMENT` block is edited, or created after `STREAMINFO` when the edit
  sets a field; the other
  blocks (pictures, seek table, application data) stay as they are. The padding blocks are
  merged into one that keeps the metadata at its old size when the rest fits, so the audio
  frames do not move; when the metadata grows past it, it is followed by 1024 bytes of
  padding. An ID3v2 tag in front of the stream is kept byte for byte. An edit of the cover
  changes the pictures in both (*Covers*). Sources: RFC 9639,
  sections 8.1 to 8.6 (<https://www.rfc-editor.org/rfc/rfc9639>);
  <https://xiph.org/vorbis/doc/v-comment.html>.
- **Vorbis comments** (FLAC and Ogg). Field names compare without case. Every field with the
  name is removed and the new value takes the place of the first. Other fields, a picture in
  `METADATA_BLOCK_PICTURE` included unless the edit names the cover, stay byte for byte, and
  so does the vendor string; a new
  FLAC comment block has an empty one. The comment goes to `COMMENT`, the name most software
  reads; `DESCRIPTION`, the name the Vorbis comment document lists, is left as it is.
- **Ogg Vorbis.** The comment header, the second packet, is rebuilt, and the pages holding it
  and the setup header are cut again: at most 255 lacing values a page, a page that starts
  inside a packet flagged as continued, and the granule position zero, as appendix A.2 of the
  Vorbis I specification sets for pages of headers. When the number of those pages changes,
  every later page of the stream gets its sequence number moved and its CRC computed again.
  Only the first logical stream is edited; a chained stream after it keeps its comment.
  Sources: RFC 3533 (<https://www.rfc-editor.org/rfc/rfc3533>), Vorbis I sections 5.2 and A.2
  (<https://xiph.org/vorbis/doc/Vorbis_I_spec.html>).
- **M4A.** The items of `moov/udta/meta/ilst` are edited, and the boxes that lead there are
  created when missing (`meta` with a handler `mdir`); other items stay byte for byte. Text
  items hold a `data` box of type 1 (UTF-8). The key and Platter's data are freeform (`----`)
  items with a `mean` and a `name`. When `moov` changes size, a `free` box right after it
  takes up the change when what remains is empty or a whole box, and the file keeps its
  length; otherwise everything after `moov` moves and every chunk offset (`stco`, `co64`) at or
  past the old end of `moov` moves with it. Sources: ISO/IEC 14496-12, the box structure and
  the boxes `free`, `stco`, `co64`, `udta` and `meta`
  (<https://standards.iso.org/ittf/PubliclyAvailableStandards/>); the
  QuickTime data atom and its well-known types
  (<https://developer.apple.com/documentation/quicktime-file-format/data_atom>,
  <https://developer.apple.com/documentation/quicktime-file-format/well-known_types>).
- **WAV.** The `LIST` chunk of form `INFO` is edited, or created at the end of the file;
  values are UTF-8 ending with a zero. A field named by the edit is removed from any later
  `INFO` list. The RIFF size is written again and every chunk padded to an even length.
  Source: Multimedia Programming Interface and Data Specifications 1.0
  (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/WAVE/Docs/riffmci.pdf>).
- **AIFF.** The text chunks `NAME`, `AUTH` and `ANNO` are edited, or created at the end of the
  file. All `ANNO` chunks give way to one with the comment. The FORM size is written again.
  Source: AIFF 1.3 (<https://www.mmsp.ece.mcgill.ca/Documents/AudioFormats/AIFF/Docs/AIFF-1.3.pdf>).
- **ID3v2 in WAV and AIFF.** An `id3 ` or `ID3 ` chunk is edited like the tag of an MP3 file.
  When the file has none and the edit sets a field the native chunks have no place for (key,
  tempo and Platter's data; in AIFF also album and genre), a chunk is created: `id3 ` in WAV,
  `ID3 ` in AIFF, with an ID3v2.4 tag and no padding. It also carries the title, artist and
  the rest the native chunks held, because a reader that finds it may read nothing else:
  Platter's own reader shows only the ID3 chunk of an AIFF file once there is one.

### Covers

The edit can import a cover or delete the covers of a file (`TagEdit::cover`):
`Some(Some(picture))` imports one, `Some(None)` deletes them. A picture is built with
`CoverPicture::from_bytes`, which takes the bytes of a JPEG or PNG file and refuses them
unless Platter's own reader would show them (`docs/behavior/cover-art.md`): at most 8 MiB
(`MAX_COVER_IMPORT_BYTES`), a size it can decode, at most 4096 pixels on a side.

- **Importing** removes every picture of the file marked as the front cover, in every set of
  tags, and writes the new one as the front cover, first among the pictures of the main tag.
  Other pictures (a back cover, a picture of the artist) stay. The reader then shows the new
  picture, since a front cover wins over any other.
- **Deleting** removes every picture of the file, in every set of tags, so the reader finds
  none.
- **The 8 MiB bound.** A FLAC metadata block declares its length in 24 bits (RFC 9639,
  section 8.1), so a picture block holds at most 16 MiB less its header; 8 MiB leaves room
  for the header and for the base64 of the Ogg comment, and is far above a cover.

| Format | Where the cover goes |
| --- | --- |
| ID3v2.3 / 2.4 | `APIC` frame: encoding 0, the MIME type (`image/jpeg`, `image/png`), picture type 3 (front cover), empty description, the picture |
| ID3v2.2 | `PIC` frame: encoding 0, image format `JPG` or `PNG`, picture type 3, empty description, the picture |
| FLAC | `PICTURE` metadata block (type 6): picture type 3, MIME type, empty description, width, height, colour depth (24 for JPEG; PNG from its header), 0 colours, the picture |
| Ogg Vorbis | `METADATA_BLOCK_PICTURE` comment: the body of a FLAC `PICTURE` block, in base64 |
| M4A | `covr` item: one `data` box of type 13 (JPEG) or 14 (PNG) |
| WAV, AIFF | `APIC` frame of the ID3v2 tag of the `id3 ` or `ID3 ` chunk, created when missing |

- **Which pictures a format marks as the front cover.** ID3v2 and FLAC by the picture type
  (3); a `METADATA_BLOCK_PICTURE` comment by the type in its decoded body; every `data` box of
  a `covr` item, as the reader marks them all. `COVERART`, an older Vorbis comment without a
  picture type, is removed by both importing and deleting.
- **Sets of tags.** In MP3 the ID3v2 tag at the start; in FLAC the metadata blocks, the
  comment and an ID3v2 tag in front of the stream, which is edited for the cover only; in WAV
  and AIFF the ID3 chunk. An ID3v1 tag has no picture.
- **Base64** for the Ogg comment is Platter's own code, the alphabet and padding of RFC 4648,
  section 4 (<https://www.rfc-editor.org/rfc/rfc4648>).
- Sources: <https://id3.org/id3v2.4.0-frames> (section 4.14), <https://id3.org/id3v2.3.0>
  (section 4.15), <https://id3.org/id3v2-00> (section 4.15); RFC 9639, sections 8.1 and 8.8
  (<https://www.rfc-editor.org/rfc/rfc9639>). The Vorbis comment page of the Xiph wiki, which
  describes `METADATA_BLOCK_PICTURE` and `COVERART`, could not be read on 17 November 2026
  (its certificate had expired); the field names and their encoding were checked in the
  source of the decoding library (version 0.6.1), which reads both, and so were the `data`
  types 13 and 14 of a `covr` item, since the page of the QuickTime well-known types
  (<https://developer.apple.com/documentation/quicktime-file-format/well-known_types>)
  shows nothing without a script.

### Safe writing

- **Which files.** The file must open with Platter's own reader (`probe_file`) as one of the six
  formats, and the writer must find the structure of that format in it. A file that fails
  either is refused before anything is written.
- **No half-written file.** Platter writes a new file in the same folder (name starting with
  `.platter-tags-`, created only if no file has that name): the new metadata where the old
  was, every other byte copied from the original. It flushes it to the disk, reads it again
  with Platter's own reader and, when it is still the same format, renames it over the
  original. The new file must read as the same format with the same sample rate, channel
  count and declared length. Until the rename the original is untouched; on any error the
  new file is deleted.
  The rename replaces the file in one step on the same volume. The new file gets the
  permissions of the original before the rename.
- **Links.** A path that is a symbolic link is followed: the track it points to is written,
  and the link stays a link.
- **Read-only files.** A file marked read-only is refused, on every system: a rename would
  replace it on some systems even though the file says it must not change.
- **Bounds.** Every size read from the file is checked against what holds it before it is
  used. The metadata read into memory (an ID3v2 tag, the FLAC metadata, the Ogg header
  packets, the `moov` box) is bounded at 64 MiB; a file over it is refused. The number of
  frames, blocks, boxes and chunks read is bounded too.
- **Work.** Writing reads and copies the whole file. It is slow work for a worker thread.

## Differences

- The details dialog and the menu of the list write tags (`docs/behavior/browser.md`, *Track
  details*, and `docs/behavior/cover-art.md`, *Importing and deleting a cover*); there is no
  editing in the list itself. The collection does not learn the new values from this call;
  the caller updates it.
- The rating and the colour are not written.
- The public manual imports and deletes JPEG covers only; Platter imports PNG pictures too,
  since its reader shows them.
- Platter's own reader shows title, artist, album and tempo. It does not show genre, comment,
  key or Platter's data, and in WAV it shows no tag at all: the decoding library in use reads
  the `INFO` chunk only before the audio and does not return what it finds there, and does
  not read the `id3 ` chunk (read in its source, version 0.6.1). The values are in the file
  for other software and for a later reader.
- A tempo with decimals written into MP3, AIFF, WAV or M4A is read back by Platter rounded
  (126.5 becomes 127), although the reader keeps the decimals other software writes into
  `TBPM`. Writing the decimals there would go against the ID3v2 frame specification. An M4A
  freeform tempo item (`com.apple.iTunes` / `BPM`), which some software writes with decimals,
  is removed when the edit names the tempo, so no reader finds the old value there.
- `TKEY` holds at most three characters by its specification; Platter writes the key as given.
- ID3v2.2 and 2.3 define their Unicode encoding as UCS-2; Platter writes UTF-16, which is the
  same for every character but those beyond the Basic Multilingual Plane.
- A NUL in Platter's data would end the value of its ID3v2 user text frame for a reader that
  takes only the first value; the callers write text without one.
- An APE tag in an MP3 file is not edited: it stays as it is and keeps its old values.
- ID3v2 text and AIFF text chunks follow the specifications in their encodings, except that
  AIFF text, ASCII by its specification, and RIFF `INFO` text, whose character set the
  specification leaves to a `CSET` chunk, are written as UTF-8.
- The writer accepts what other software writes and the specifications forbid, as long as the
  structure can be followed: an ID3v1 tag with an empty year, an ID3v2 frame of size zero
  (passed through). It refuses what it cannot follow: an ID3v2 frame that runs past its tag
  or has an identifier with other characters than `A-Z` and `0-9`, a FLAC file with two comment
  blocks, a chunk or box that runs past what holds it.

## Edge cases

| Input | Result |
| --- | --- |
| Every field left out | The file is written again with its tags as they were; an MP3 without a tag gets none |
| Text of spaces only | The field is removed |
| Text over 1000 characters | Cut at 1000 characters |
| Text with control characters | Those characters dropped; line feed and tab kept |
| Platter's data over 64 KiB | Error "Platter's data ... is longer than ..."; the file is not opened |
| Tempo outside 20 to 999, or not finite | Error "the tempo is out of range"; the file is not opened |
| File with no tag | The main tag of the format is created when the edit sets a field |
| File that is not audio, or a format Platter does not play | Error; the file is unchanged |
| Damaged file or tag | Error; the file is unchanged |
| Metadata over 64 MiB | Error; the file is unchanged |
| Compressed or encrypted ID3v2 frame | Kept byte for byte |
| Unsynchronised ID3v2 tag or frame | Read, and written back plain |
| ID3v2 extended header or footer | Not written back |
| Ogg stream interleaved with another one in its headers, or audio on the page of the setup header | Error; the file is unchanged |
| Bytes that are not Ogg pages after the headers, when the header pages change in number | Error; the file is unchanged (later pages would keep their old numbers) |
| Fewer than eight bytes after the last chunk of WAV or AIFF, inside the container | Dropped, so the chunks added after them start on a chunk boundary |
| Fragmented M4A whose `moov` changes size without a `free` box to take it up | Error; the file is unchanged |
| Chunk offset of M4A that would pass 32 bits | Error; the file is unchanged |
| RF64 WAV, or a file the change would push past 4 GiB | Error; the file is unchanged |
| The written copy no longer reads as the same format, rate, channel count or length | Error; the copy is deleted |
| Read-only file | Error "the file is read-only"; the file is unchanged |
| Cover picture over 8 MiB, of another format than JPEG or PNG, damaged, or over 4096 pixels on a side | Refused by `CoverPicture::from_bytes`; no file is opened |
| Cover deleted from a file without pictures | The file is written again with its tags as they were |
| Missing file, or a folder | Error from the system |
| The folder does not let a file be created | Error from the system; the file is unchanged |

No input makes writing tags panic.

## Tests

When Platter writes: `crates/ui-egui/tests/collection.rs` (the details, what a deck learned,
each mode, a read-only file), the unit tests of `apps/platter/src/background.rs` (what the
analysis found) and `crates/ui-egui/tests/settings_mixer.rs` (the setting).

How each format is written: `crates/decode/tests/tag_write.rs`, against files the tests
synthesise in a folder under the build directory:

- WAV, AIFF and FLAC from `platter-testkit`, MP3 as silent frames, Ogg Vorbis as pages built
  in the test with an identification header and a setup packet that holds no codebooks, and
  M4A as boxes built in the test with one AAC sample entry and a few bytes of media. Neither
  of the last two holds audio that decodes; reading the header and the tags needs none.
- The fields are read back with Platter's reader where it shows them and otherwise with small
  parsers in the test file, written from the specifications apart from the writer. Each format
  has a test that compares with bytes written out by hand from its specification.
- Each format: every field, Platter's data included, is written and read back; a second edit
  changes two fields, clears two and leaves the others; clearing every field leaves none; the
  decoded audio and the header Platter reads stay the same.
- Platter's data is set, replaced, set empty and removed in each format, and the custom data
  of other software beside it stays.
- Fields the edit does not name survive: other ID3v2 frames in their order, a picture, a tempo
  with decimals, a comment frame with a description and a compressed frame in MP3; custom
  Vorbis comments and a picture block in FLAC; a custom item and the tempo item in M4A.
- Layout: the M4A media offset follows a larger `moov`, a `free` box after `moov` takes up the
  change both ways, a missing `udta` is created; FLAC padding takes up a growing comment; an
  Ogg comment that grows past a page renumbers the later pages with valid CRCs, and shrinking
  it again gives back the original bytes; an ID3v2.3 tag stays 2.3 with UTF-16 text, and an
  ID3v2.2 tag stays 2.2 with its comment language; an ID3v2 tag in front of a FLAC stream
  keeps its bytes; an ID3 chunk created in AIFF carries the title of its `NAME` chunk.
- An unsynchronised tag is written plain, frame by frame in 2.4 and whole in 2.3; an ID3v1
  tag, a RIFF INFO chunk and an AIFF `NAME` chunk already in the file are edited too; a file
  without a tag gets none from an edit that sets nothing; on Unix the file keeps its
  permissions (not run on Windows, where the development happens).
- Bounds of text, tempo and Platter's data, a non-audio file, damaged files, a read-only file,
  a missing file and a folder: each returns an error and leaves the bytes of the file and the
  folder as they were. A symbolic link stays a link (on Windows the test checks nothing when
  the account cannot create links).
- A property test cuts or changes one byte of a file of each format: the writer never panics;
  it either refuses the file and leaves it as it was, or writes it and the result reads as the
  same format.

- Sizes checked by hand: ID3v2 frames and tags over 127 bytes in 2.4 (synchsafe) and 2.3
  (plain frame size), and the sizes of `moov`, `udta`, `meta` and `ilst`. Also a `co64` file,
  a `moov` after the media (no offset moves), a fragmented file refused when `moov` must
  grow, stray bytes after the last WAV or AIFF chunk, and bytes that are not pages between
  Ogg pages that must be renumbered.

What no test covers: a failure in the middle of the rename, encrypted ID3v2 frames, a 2.4
extended header or footer, chained Ogg streams, a `free` box over 4 GiB, APE tags, and files
from real encoders, which the repository does not hold.
