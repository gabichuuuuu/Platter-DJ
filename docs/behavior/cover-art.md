# Cover art

A track file can carry pictures in its tags, and one of them is usually the front cover of the
record. This note covers how Platter finds that picture and turns it into pixels small enough
for the browser. Nothing in the window shows it yet: the panel that does comes later in M3.

## What the public manual says

Sections of the reference manual, read on 15 November 2026:

- *Managing Your Track Collection*, section *Editing Track Cover Artwork*
- *Overview*, section *Browser*

The browser has an area, Cover Artwork, that shows the cover of the track selected in the
track list. The header of a deck shows the cover of the track loaded in it.

A cover is imported for one track or a selection of tracks from a menu of the track list, by
choosing an image file; the section names JPEG files. The cover is then stored in the track
files themselves. Another entry of the same menu deletes the cover of the tracks selected. The
section states no size or resolution for the picture.

## What Platter does

`platter-decode` reads the cover of a file with `cover_file` (or `cover_bytes` for a file held
in memory) and returns it decoded, or nothing when the file carries no picture.

- **Where the picture comes from.** The tags the decoding library reads: the attached-picture
  frames of ID3v2 (in front of MP3 data, or in the ID3 chunk of an AIFF file), the picture
  blocks of FLAC, the picture comments of Vorbis comments (`METADATA_BLOCK_PICTURE`, in FLAC
  and in Ogg Vorbis) and the cover items of an M4A file. Every set of tags in the file is
  looked at, not only the last one.
- **Which picture.** A picture marked as the front cover wins. Without one, the first picture
  found is used. An M4A cover item is always taken as a front cover: the format has no other
  kind.
- **Image formats.** JPEG and PNG, recognised by their content; the type the tag declares is
  ignored. A PNG keeps its transparency.
- **Size.** The picture is scaled down to fit in 256 by 256 pixels, keeping its proportions; a
  smaller picture keeps its size. Each pixel of the result is the average of the pixels of the
  picture it covers, weighted by their opacity so that transparent pixels do not darken the
  edges. The result is 8-bit RGBA, rows from the top, alpha not premultiplied.
- **Bounds.** A picture larger than 16 MiB as stored, or that declares more than 8192 pixels on
  a side, is refused before its pixels are decoded.
- **Work.** Reading a cover opens the file and reads its tags; it decodes no audio. It is slow
  work for a worker thread, like reading the header of a file.

## Differences

- Platter only reads covers. Importing a cover into a file and deleting it are not
  implemented; they belong to the editing of tags, planned for later in M3.
- PNG pictures are read as well as JPEG.
- The decoding library reads no tags from WAV files, so a WAV file shows no cover, even with a
  picture in an ID3 chunk.
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
| Picture that declares more than 8192 pixels on a side | Error "the picture is too large"; no pixels are decoded |
| Very narrow picture (8192 by 1) | Scaled to 256 by 1; no side goes below one pixel |
| Audio file that cannot be opened or read | The error of the audio file (`docs/behavior/formats.md`) |
| The decoding library or an image decoder panics on the content | Error, as for a damaged file (`docs/decisions/0008-decoder-panic-boundary.md`) |

No input makes reading a cover panic.

## Tests

`crates/decode/tests/cover.rs`. The pictures are built in the test: PNG files written by hand,
and baseline JPEG files from a small encoder of flat 8 by 8 blocks. They are embedded in FLAC
files synthesised in the test as a picture block and as a `METADATA_BLOCK_PICTURE` comment, and
in an ID3v2 tag placed in front of a FLAC file and inside an AIFF file. The tests check the
decoded size and pixels, the scaling, the choice of the front cover, damaged and oversized
pictures and a file without a picture.

MP3, M4A and Ogg Vorbis need an encoder to produce a valid file, and the project has none. For
those three the reading of the picture was checked in the source of the decoding library, not
by a test: MP3 shares the ID3v2 reader tested through AIFF and FLAC, and Ogg Vorbis the Vorbis
comment reader tested through FLAC.
