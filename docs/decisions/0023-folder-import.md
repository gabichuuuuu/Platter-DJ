# 0023. Folders are imported by the library, with a reader the caller gives

Status: accepted (M3).

## Context

Importing a folder walks the disk, reads the tags of each audio file and writes the tracks to
the library. The library (`platter-library`) and the decoder that reads tags
(`platter-decode`) are both in layer L1, and L1 crates do not depend on each other
(`docs/architecture.md`). What the import must do is in `docs/behavior/library.md`, section
*Importing folders*. Left open: where the walk lives, how a folder is named in the store, and
how the tracks are written.

## Options

1. The walk in the library, with the reading of each file behind a trait the caller gives.
2. The walk in the engine (L2), which sees both crates.
3. The walk in the interface, beside the file list of the browser.

For the writing: one transaction per track, as `Library::import` does, or batches.

## Choice

- Option 1. `import_folder` walks, skips what the collection holds and writes; a `TagReader`
  says what each file is. The library stays free of audio formats, the walk is tested with a
  reader that needs no audio, and the reader over the decoder comes with the browser, which is
  its user. The engine is about playing tracks, and the interface should not own disk work.
- The folder is canonicalised once; the paths below it are that path joined with the names
  read, and links are not followed, so they are canonical too. On Windows the prefix `\\?\`
  that canonicalising adds is dropped (`C:\Music`, `\\server\share`), so a path reads as the
  user knows it.
- `Library::import_all` writes several tracks in one transaction, all or none; the walk writes
  batches of 500.

## How it was measured

A throwaway test, not kept, built with `--release` on Windows x64, solid-state disk, a library
in a temporary directory: 1 000 calls of `import`, one transaction each, took 2.3 s; 10 000 tracks
through `import_all` in batches of 500 took 0.33 s. A collection of tens of thousands of tracks
would take minutes one by one and seconds in batches.

`cargo test -p platter-library --test folders` checks the order, the limits, a second pass and
another spelling of the path. It failed when the depth limit was moved one level down and when
the last batch was not written.

## Known limits

- The links test runs on Linux and macOS only: making a link on Windows needs a privilege. A
  junction back up the tree was checked by hand on Windows with a throwaway test: it was left
  out and counted. The links test, the test of names that are not Unicode and the Unix half of
  the batch test first run in CI.
- A folder with more than 10 000 entries gives the first ones the system lists, which is not
  an order by name; the rest are counted as left out.
- A mapped network drive canonicalises to its network path (`\\server\share`), not to its
  letter.
- The pass cannot be stopped half way yet; that comes with the browser that starts it.
