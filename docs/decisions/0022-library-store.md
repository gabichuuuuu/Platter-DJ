# 0022. The library is one SQLite file with a versioned schema

Status: accepted (M3).

## Context

The roadmap puts the library in SQLite behind a project trait, and `docs/dependencies.md`
approves rusqlite for it. What the store must do is in `docs/behavior/library.md`. Left open:
which SQLite, how the file is told apart from others and from later versions of itself, and how
text is compared.

## Options

1. rusqlite with the SQLite of the system.
2. rusqlite with its `bundled` feature, which compiles the SQLite source it carries.

For the text: compare with what SQLite offers (`LIKE`, `NOCASE`, `lower`), which folds the case
of English letters only, or fold the text in Rust and compare the folded form.

## Choice

- rusqlite 0.42.0 with `bundled`, which carries SQLite 3.54.1 in libsqlite3-sys 0.40.0. Every
  platform gets the same SQLite, Windows has none to link against, and the build needs only the
  C compiler cargo already uses. rusqlite and libsqlite3-sys are MIT; what they bring is MIT,
  Apache-2.0 or Zlib. The crate `platter-library` alone uses it, and no type of rusqlite shows
  in its interface: `StoreError` keeps the cause behind its `source`.
- The file says it is a library with `PRAGMA application_id` set to "PLTR", and gives the
  version of its tables in `PRAGMA user_version`, now 6: version 2 added the preparation list,
  version 3 the favourites, version 4 the colour, rating and comment of a track, version 5 what
  a deck learned of it and version 6 the rules of the smart playlists, and an older
  file is brought up to date when opened. Opening reads both before writing
  anything: an empty file is given the tables, a file with another id or with tables of its own
  is refused, and a version above the one the code writes is refused. A later schema adds a
  migration from each version to the next, run in one transaction.
- Every change is one transaction. Foreign keys are on, so removing a track or a playlist takes
  its entries with it. A call waits up to 2 s for a file another program is writing.
- The library folds text with Rust's `to_lowercase` and keeps the folded tags beside the tags.
  Search compares the folded words with `LIKE ... ESCAPE`, the words bound as parameters and
  their wildcards escaped; sorting uses the folded form too. The column to sort by comes from an
  enum, never from text.

## How it was checked

`cargo test -p platter-library`, Windows x64: search finds "école" in "ÉCOLE" and takes `%` and
`_` as text; a text file and a foreign SQLite file are refused byte for byte unchanged, and so
is a library whose version was set to 2; the operations on a playlist were checked against a
list in memory over 64 random sequences, and that check failed when the removal of repeats was
made to keep the last entry instead of the first.

## Known limits

- Folding is not a collation: names and tags sort by the code points of their folded form, so
  "é" comes after "z". A collation for the language of the user is left for later.
- Search ignores case but not accents: "ecole" does not find "école".
- Paths are compared as text, as the importer gives them.
