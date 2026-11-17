//! Importing folders, on folders made in a temporary directory, with a reader that takes the
//! extension for the format and the name for the title.
//! Behaviour note: `docs/behavior/library.md`, sections *Importing folders* and *Importing
//! one file*.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use platter_library::{
    Library, LibraryError, MAX_DEPTH, Query, Reading, SqliteLibrary, TagReader, Timestamp,
    TrackTags, import_file, import_folder,
};

const AT: Timestamp = Timestamp(1_700_000_000);

fn directory(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("folders-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// Files with the extension `wav` are tracks titled by `prefix` and their name; `bad`
/// cannot be read; the rest is not audio. Keeps what it was asked.
#[derive(Default)]
struct Reader {
    asked: Vec<PathBuf>,
    prefix: &'static str,
}

impl TagReader for Reader {
    fn read(&mut self, path: &Path) -> Reading {
        self.asked.push(path.to_owned());
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("wav") => Reading::Tags(TrackTags {
                title: path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(|stem| format!("{}{stem}", self.prefix)),
                ..TrackTags::default()
            }),
            Some("bad") => Reading::Unreadable,
            _ => Reading::NotAudio,
        }
    }
}

fn touch(root: &Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"").unwrap();
}

/// The tracks of the collection in the order they were added: their path relative to
/// `root`, with `/` between folders, and their title.
fn collection(library: &SqliteLibrary, root: &Path) -> Vec<(String, String)> {
    let root = std::fs::canonicalize(root).unwrap();
    let mut hits = library.search(&Query::default()).unwrap().hits;
    hits.sort_by_key(|hit| hit.track.id);
    hits.into_iter()
        .map(|hit| {
            let path = std::fs::canonicalize(&hit.track.path).unwrap();
            let relative = path.strip_prefix(&root).unwrap().to_string_lossy();
            (
                relative.replace('\\', "/"),
                hit.track.tags.title.unwrap_or_default(),
            )
        })
        .collect()
}

fn paths(tracks: &[(String, String)]) -> Vec<&str> {
    tracks.iter().map(|(path, _)| path.as_str()).collect()
}

#[test]
fn a_folder_and_the_folders_inside_it_are_imported_in_order() {
    let root = directory("order");
    for file in [
        "b.wav",
        "A.wav",
        "cover.jpg",
        "broken.bad",
        ".hidden.wav",
        ".hidden/c.wav",
        "sub2/deeper/e.wav",
        "Sub/d.wav",
    ] {
        touch(&root, file);
    }
    let mut library = SqliteLibrary::open(&directory("order-db").join("library.db")).unwrap();
    let mut reader = Reader::default();
    let report = import_folder(&mut library, &root, &mut reader, AT).unwrap();
    assert_eq!(report.added.len(), 4);
    assert_eq!((report.known, report.unreadable, report.skipped), (0, 1, 0));
    let tracks = collection(&library, &root);
    assert_eq!(
        paths(&tracks),
        ["A.wav", "b.wav", "Sub/d.wav", "sub2/deeper/e.wav"]
    );
    assert_eq!(tracks[0].1, "A");
    // Hidden entries are not even looked at.
    let canonical = std::fs::canonicalize(&root).unwrap();
    assert!(reader.asked.iter().all(|path| {
        !std::fs::canonicalize(path)
            .unwrap()
            .strip_prefix(&canonical)
            .unwrap()
            .components()
            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    }));
    let track = library.track(report.added[0]).unwrap().unwrap();
    assert_eq!(track.imported, AT);
    assert!(track.path.is_absolute());
}

#[test]
fn importing_again_adds_only_the_new_files_and_keeps_the_rest() {
    let root = directory("again");
    touch(&root, "one.wav");
    touch(&root, "inner/two.wav");
    let mut library = SqliteLibrary::open(&directory("again-db").join("library.db")).unwrap();
    import_folder(&mut library, &root, &mut Reader::default(), AT).unwrap();
    touch(&root, "inner/three.wav");
    let mut reader = Reader {
        prefix: "changed ",
        ..Reader::default()
    };
    let report = import_folder(&mut library, &root, &mut reader, Timestamp(AT.0 + 60)).unwrap();
    assert_eq!((report.added.len(), report.known), (1, 2));
    assert_eq!(
        reader.asked.len(),
        1,
        "files already held are not read again"
    );
    let tracks = collection(&library, &root);
    assert_eq!(
        tracks,
        [
            ("one.wav".to_owned(), "one".to_owned()),
            ("inner/two.wav".to_owned(), "two".to_owned()),
            ("inner/three.wav".to_owned(), "changed three".to_owned()),
        ]
    );
}

#[test]
fn one_file_has_one_record_however_its_folder_is_named() {
    let root = directory("spelling");
    touch(&root, "inner/track.wav");
    let mut library = SqliteLibrary::open(&directory("spelling-db").join("library.db")).unwrap();
    let roundabout = root.join("inner").join("..").join(".");
    import_folder(&mut library, &roundabout, &mut Reader::default(), AT).unwrap();
    let report = import_folder(
        &mut library,
        &root.join("inner"),
        &mut Reader::default(),
        AT,
    )
    .unwrap();
    assert_eq!((report.added.len(), report.known), (0, 1));
    assert_eq!(library.search(&Query::default()).unwrap().total, 1);
    let stored = library.search(&Query::default()).unwrap().hits[0]
        .track
        .path
        .clone();
    // The path reads as the user knows it, without the prefix of a canonical Windows path.
    assert!(!stored.to_string_lossy().starts_with(r"\\?\"));
    assert!(library.track_at(&stored).unwrap().is_some());
}

#[test]
fn a_folder_that_cannot_be_imported_is_an_error_and_changes_nothing() {
    let root = directory("missing");
    touch(&root, "file.wav");
    let mut library = SqliteLibrary::open(&directory("missing-db").join("library.db")).unwrap();
    for folder in [root.join("nowhere"), root.join("file.wav")] {
        let result = import_folder(&mut library, &folder, &mut Reader::default(), AT);
        assert!(
            matches!(result, Err(LibraryError::NotAFolder(_))),
            "{result:?}"
        );
    }
    assert_eq!(library.search(&Query::default()).unwrap().total, 0);
}

#[test]
fn folders_deeper_than_the_limit_are_left_out() {
    let root = directory("deep");
    let mut deepest = String::new();
    for level in 1..=MAX_DEPTH + 1 {
        deepest.push_str("d/");
        touch(&root, &format!("{deepest}{level}.wav"));
    }
    let mut library = SqliteLibrary::open(&directory("deep-db").join("library.db")).unwrap();
    let report = import_folder(&mut library, &root, &mut Reader::default(), AT).unwrap();
    assert_eq!(report.added.len(), MAX_DEPTH);
    assert_eq!(report.skipped, 1);
}

#[test]
fn a_large_folder_is_written_in_batches_and_all_of_it_arrives() {
    let root = directory("large");
    let count = 1_234;
    for index in 0..count {
        touch(&root, &format!("{index:04}.wav"));
    }
    let mut library = SqliteLibrary::open(&directory("large-db").join("library.db")).unwrap();
    let report = import_folder(&mut library, &root, &mut Reader::default(), AT).unwrap();
    assert_eq!(report.added.len(), count);
    assert_eq!(library.search(&Query::default()).unwrap().total, count);
    let title = |index: usize| {
        library
            .track(report.added[index])
            .unwrap()
            .unwrap()
            .tags
            .title
    };
    assert_eq!(title(0).as_deref(), Some("0000"));
    assert_eq!(title(count - 1), Some(format!("{:04}", count - 1)));
}

#[test]
fn a_batch_is_written_whole_or_not_at_all() {
    let mut library = SqliteLibrary::open(&directory("batch").join("library.db")).unwrap();
    let tags = TrackTags::default();
    let good = PathBuf::from("/music/good.wav");
    #[cfg(unix)]
    let bad = {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(b"/music/\xff.wav"))
    };
    #[cfg(windows)]
    let bad = {
        use std::os::windows::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_wide(&[0xD800]))
    };
    let result = library.import_all(&[(good.clone(), tags.clone()), (bad, tags)], AT);
    assert!(
        matches!(result, Err(LibraryError::NotUnicode)),
        "{result:?}"
    );
    assert_eq!(library.track_at(&good).unwrap(), None);
}

/// Links are not followed, so a link back up the tree does not make the import go round.
/// Making links needs a privilege on Windows, so this runs on the other systems.
#[cfg(unix)]
#[test]
fn links_are_not_followed() {
    let root = directory("links");
    touch(&root, "inner/track.wav");
    std::os::unix::fs::symlink(&root, root.join("inner/up")).unwrap();
    std::os::unix::fs::symlink(root.join("inner/track.wav"), root.join("alias.wav")).unwrap();
    let mut library = SqliteLibrary::open(&directory("links-db").join("library.db")).unwrap();
    let report = import_folder(&mut library, &root, &mut Reader::default(), AT).unwrap();
    assert_eq!((report.added.len(), report.skipped), (1, 2));
}

/// File systems of macOS refuse such names, and Windows names are UTF-16, so this runs on
/// Linux.
#[cfg(target_os = "linux")]
#[test]
fn names_that_are_not_unicode_are_left_out() {
    use std::os::unix::ffi::OsStrExt;
    let root = directory("names");
    touch(&root, "fine.wav");
    std::fs::write(root.join(std::ffi::OsStr::from_bytes(b"\xff.wav")), b"").unwrap();
    let mut library = SqliteLibrary::open(&directory("names-db").join("library.db")).unwrap();
    let report = import_folder(&mut library, &root, &mut Reader::default(), AT).unwrap();
    assert_eq!((report.added.len(), report.skipped), (1, 1));
}

#[test]
fn one_file_enters_the_collection_once_whichever_way_its_path_is_written() {
    let root = directory("one-file");
    touch(&root, "sets/opener.wav");
    let mut library = SqliteLibrary::open(&directory("one-file-db").join("library.db")).unwrap();
    let mut reader = Reader::default();
    let path = root.join("sets").join("opener.wav");
    let id = import_file(&mut library, &path, &mut reader, AT)
        .unwrap()
        .unwrap();
    assert_eq!(
        collection(&library, &root),
        [("sets/opener.wav".to_owned(), "opener".to_owned())]
    );
    // The same file named another way is the same track, and is not read again.
    let other = root.join("sets").join("..").join("sets").join("opener.wav");
    assert_eq!(
        import_file(&mut library, &other, &mut reader, AT).unwrap(),
        Some(id)
    );
    assert_eq!(reader.asked.len(), 1);
}

#[test]
fn a_file_without_music_or_without_a_file_gives_no_track() {
    let root = directory("one-file-none");
    touch(&root, "notes.txt");
    touch(&root, "broken.bad");
    let mut library =
        SqliteLibrary::open(&directory("one-file-none-db").join("library.db")).unwrap();
    let mut reader = Reader::default();
    for name in ["notes.txt", "broken.bad", "gone.wav"] {
        assert_eq!(
            import_file(&mut library, &root.join(name), &mut reader, AT).unwrap(),
            None,
            "{name}"
        );
    }
    // A folder is not a file.
    assert_eq!(
        import_file(&mut library, &root, &mut reader, AT).unwrap(),
        None
    );
    assert!(collection(&library, &root).is_empty());
}

#[test]
fn a_track_whose_file_is_gone_is_still_found_by_its_path() {
    let root = directory("one-file-missing");
    touch(&root, "kept.wav");
    let mut library =
        SqliteLibrary::open(&directory("one-file-missing-db").join("library.db")).unwrap();
    let mut reader = Reader::default();
    let id = import_file(&mut library, &root.join("kept.wav"), &mut reader, AT)
        .unwrap()
        .unwrap();
    let stored = library.track(id).unwrap().unwrap().path;
    std::fs::remove_file(&stored).unwrap();
    assert_eq!(
        import_file(&mut library, &stored, &mut reader, AT).unwrap(),
        Some(id)
    );
}

#[test]
fn missing_tracks_are_relocated_by_a_name_found_once_and_wanted_once() {
    let root = directory("relocate");
    let old = root.join("old");
    for file in [
        "a.wav",
        "b.wav",
        "one/x.wav",
        "two/x.wav",
        "c.wav",
        "keep.wav",
    ] {
        touch(&old, file);
    }
    let mut library = SqliteLibrary::open(&root.join("library.db")).unwrap();
    import_folder(&mut library, &old, &mut Reader::default(), AT).unwrap();
    for file in ["a.wav", "b.wav", "one/x.wav", "two/x.wav", "c.wav"] {
        std::fs::remove_file(old.join(file)).unwrap();
    }
    // The a-side under another case, b twice, x once for two missing tracks, c nowhere.
    let new = root.join("new");
    for file in ["deep/A.wav", "b.wav", "copy/b.wav", "x.wav"] {
        touch(&new, file);
    }
    assert_eq!(library.check_files().unwrap().missing.len(), 5);
    assert_eq!(library.missing().unwrap().len(), 5);

    let report = platter_library::relocate_missing(&mut library, &new).unwrap();
    assert_eq!(report.relocated.len(), 1);
    assert_eq!(report.ambiguous, 3);
    assert_eq!(report.not_found, 1);
    let relocated = library.track(report.relocated[0]).unwrap().unwrap();
    assert!(!relocated.missing);
    assert_eq!(relocated.tags.title.as_deref(), Some("a"));
    assert_eq!(library.missing().unwrap().len(), 4);

    // The new path is kept as an import keeps it: importing the folder finds the track known.
    let imported = import_folder(&mut library, &new, &mut Reader::default(), AT).unwrap();
    assert_eq!(imported.known, 1);
    assert_eq!(imported.added.len(), 3);

    assert!(matches!(
        platter_library::relocate_missing(&mut library, &root.join("nowhere")),
        Err(LibraryError::NotAFolder(_))
    ));
}

/// A reader that stops the import once it has looked at `after` files.
struct Stopping {
    inner: Reader,
    after: usize,
    told: Vec<usize>,
}

impl TagReader for Stopping {
    fn read(&mut self, path: &Path) -> Reading {
        self.inner.read(path)
    }

    fn keep_going(&mut self, looked: usize) -> bool {
        self.told.push(looked);
        looked < self.after
    }
}

#[test]
fn a_reader_stops_the_import_and_what_was_read_stays() {
    let root = directory("stopped");
    for file in ["a.wav", "b.wav", "c.wav", "inner/d.wav"] {
        touch(&root, file);
    }
    let mut library = SqliteLibrary::open(&directory("stopped-db").join("library.db")).unwrap();
    let mut reader = Stopping {
        inner: Reader::default(),
        after: 2,
        told: Vec::new(),
    };
    let report = import_folder(&mut library, &root, &mut reader, AT).unwrap();
    assert!(report.stopped);
    assert_eq!(report.added.len(), 2);
    assert_eq!(reader.told, [0, 1, 2]);
    assert_eq!(paths(&collection(&library, &root)), ["a.wav", "b.wav"]);

    // Again to the end: the files read before are known, the rest are added.
    let report = import_folder(&mut library, &root, &mut Reader::default(), AT).unwrap();
    assert!(!report.stopped);
    assert_eq!((report.added.len(), report.known), (2, 2));
}
