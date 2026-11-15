//! The library store on files in a temporary directory, with invented tracks. Behaviour note:
//! `docs/behavior/library.md`.

// The helpers that build the libraries are not tests themselves.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use platter_core::DeckId;
use platter_library::{
    Column, Library, LibraryError, MAX_TAG_CHARS, NodeKind, Query, SqliteLibrary, Timestamp,
    TrackId, TrackTags,
};

/// A fresh directory for one test; names are unique among the tests, which run in parallel.
fn directory(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("library-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn library(name: &str) -> SqliteLibrary {
    SqliteLibrary::open(&directory(name).join("library.db")).unwrap()
}

fn tags(title: &str, artist: &str) -> TrackTags {
    TrackTags {
        title: Some(title.to_owned()),
        artist: Some(artist.to_owned()),
        ..TrackTags::default()
    }
}

fn add(library: &mut SqliteLibrary, path: &str, title: &str, artist: &str) -> TrackId {
    library
        .import(Path::new(path), &tags(title, artist), Timestamp(1000))
        .unwrap()
}

fn titles(library: &SqliteLibrary, query: &Query) -> Vec<String> {
    library
        .search(query)
        .unwrap()
        .hits
        .into_iter()
        .map(|hit| hit.track.tags.title.unwrap_or_default())
        .collect()
}

fn text(words: &str) -> Query {
    Query {
        text: words.to_owned(),
        column: Column::Title,
        ..Query::default()
    }
}

#[test]
fn a_track_is_kept_by_its_path_and_importing_it_again_updates_its_tags() {
    let mut library = library("import");
    let path = Path::new("/music/one.flac");
    let id = library
        .import(
            path,
            &TrackTags {
                title: Some("One".into()),
                bpm: Some(f64::NAN),
                seconds: Some(-3.0),
                sample_rate: Some(44_100),
                genre: Some("x".repeat(MAX_TAG_CHARS + 50)),
                ..TrackTags::default()
            },
            Timestamp(1000),
        )
        .unwrap();
    let track = library.track(id).unwrap().unwrap();
    assert_eq!(track.path, path);
    assert_eq!(track.tags.bpm, None);
    assert_eq!(track.tags.seconds, None);
    assert_eq!(track.tags.sample_rate, Some(44_100));
    assert_eq!(track.tags.genre.unwrap().chars().count(), MAX_TAG_CHARS);
    assert_eq!(track.imported, Timestamp(1000));

    let session = library.open_session(Timestamp(1500)).unwrap();
    library
        .record_played(session, id, DeckId::A, Timestamp(1600), 200.0)
        .unwrap();
    let again = library
        .import(path, &tags("One (edit)", "Someone"), Timestamp(2000))
        .unwrap();
    assert_eq!(again, id);
    let track = library.track_at(path).unwrap().unwrap();
    assert_eq!(track.tags.title.as_deref(), Some("One (edit)"));
    assert_eq!(track.imported, Timestamp(1000));
    assert_eq!(track.play_count, 1);
}

#[test]
fn the_consistency_check_marks_missing_files_and_a_track_can_be_relocated() {
    let directory = directory("check");
    let mut library = SqliteLibrary::open(&directory.join("library.db")).unwrap();
    let file = directory.join("track.wav");
    let moved = directory.join("moved.wav");
    std::fs::write(&file, b"audio").unwrap();
    let id = library
        .import(&file, &tags("T", "A"), Timestamp(1))
        .unwrap();
    let other = library
        .import(&directory.join("other.wav"), &tags("O", "A"), Timestamp(1))
        .unwrap();

    let report = library.check_files().unwrap();
    assert_eq!(report.missing, vec![other]);
    assert!(report.found.is_empty());
    std::fs::rename(&file, &moved).unwrap();
    let report = library.check_files().unwrap();
    assert_eq!(report.missing, vec![id]);
    assert!(library.track(id).unwrap().unwrap().missing);

    assert!(matches!(
        library.relocate(id, &directory.join("other.wav")),
        Err(LibraryError::PathTaken)
    ));
    library.relocate(id, &moved).unwrap();
    let track = library.track(id).unwrap().unwrap();
    assert_eq!(track.path, moved);
    assert!(!track.missing);
    assert_eq!(track.tags.title.as_deref(), Some("T"));
}

#[test]
fn removing_a_track_takes_it_out_of_every_playlist_and_keeps_its_history() {
    let mut library = library("remove");
    let a = add(&mut library, "/a", "A", "X");
    let b = add(&mut library, "/b", "B", "Y");
    let list = library.create(None, NodeKind::Playlist, "Set").unwrap();
    library.add(list, &[a, b, a, b], None).unwrap();
    let session = library.open_session(Timestamp(10)).unwrap();
    library
        .record_played(session, a, DeckId::B, Timestamp(20), 61.5)
        .unwrap();

    library.remove_track(a).unwrap();
    let left: Vec<TrackId> = library
        .entries(list)
        .unwrap()
        .iter()
        .map(|e| e.track)
        .collect();
    assert_eq!(left, vec![b, b]);
    let history = library.history(session).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].track, None);
    assert_eq!(history[0].title.as_deref(), Some("A"));
    assert_eq!(history[0].deck, DeckId::B);
    assert!(matches!(
        library.remove_track(a),
        Err(LibraryError::NoSuchTrack)
    ));
}

#[test]
fn playlists_and_folders_form_a_tree() {
    let mut library = library("tree");
    let crates = library.create(None, NodeKind::Folder, "Crates").unwrap();
    let house = library
        .create(Some(crates), NodeKind::Folder, "house")
        .unwrap();
    let list = library
        .create(Some(house), NodeKind::Playlist, "Ébauche")
        .unwrap();
    library
        .create(Some(house), NodeKind::Playlist, "zeta")
        .unwrap();
    library
        .create(Some(house), NodeKind::Playlist, "Alpha")
        .unwrap();

    let names: Vec<String> = library
        .children(Some(house))
        .unwrap()
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(names, ["Alpha", "zeta", "Ébauche"]);

    assert!(matches!(
        library.create(Some(list), NodeKind::Playlist, "In"),
        Err(LibraryError::WrongKind)
    ));
    assert!(matches!(
        library.create(None, NodeKind::Playlist, ""),
        Err(LibraryError::InvalidName)
    ));
    assert!(matches!(
        library.rename(list, &"n".repeat(201)),
        Err(LibraryError::InvalidName)
    ));
    assert!(matches!(
        library.move_node(crates, Some(house)),
        Err(LibraryError::Cycle)
    ));
    assert!(matches!(
        library.move_node(crates, Some(crates)),
        Err(LibraryError::Cycle)
    ));

    library.move_node(list, None).unwrap();
    library.rename(list, "Draft").unwrap();
    let root: Vec<String> = library
        .children(None)
        .unwrap()
        .into_iter()
        .map(|node| node.name)
        .collect();
    assert_eq!(root, ["Crates", "Draft"]);

    library.delete_node(crates).unwrap();
    assert!(matches!(
        library.children(Some(house)),
        Err(LibraryError::NoSuchNode)
    ));
    assert_eq!(library.children(None).unwrap().len(), 1);
}

#[test]
fn a_playlist_keeps_its_order_and_its_repeats() {
    let mut library = library("playlist");
    let [a, b, c] = ["/a", "/b", "/c"].map(|path| add(&mut library, path, path, "X"));
    let list = library.create(None, NodeKind::Playlist, "Set").unwrap();
    let folder = library.create(None, NodeKind::Folder, "Folder").unwrap();
    library.add(list, &[a, b], None).unwrap();
    library.add(list, &[c, a], Some(1)).unwrap();
    let order = |library: &SqliteLibrary| -> Vec<TrackId> {
        library
            .entries(list)
            .unwrap()
            .iter()
            .map(|e| e.track)
            .collect()
    };
    assert_eq!(order(&library), [a, c, a, b]);

    let first = library.entries(list).unwrap()[0].id;
    library.move_entry(list, first, 99).unwrap();
    assert_eq!(order(&library), [c, a, b, a]);
    library.remove_repeats(list).unwrap();
    assert_eq!(order(&library), [c, a, b]);

    let middle = library.entries(list).unwrap()[1].id;
    library.remove_entries(list, &[middle]).unwrap();
    assert_eq!(order(&library), [c, b]);
    assert!(matches!(
        library.move_entry(list, middle, 0),
        Err(LibraryError::NoSuchEntry)
    ));
    assert!(matches!(
        library.add(folder, &[a], None),
        Err(LibraryError::WrongKind)
    ));
    let gone = add(&mut library, "/gone", "Gone", "X");
    library.remove_track(gone).unwrap();
    assert!(matches!(
        library.add(list, &[a, gone], None),
        Err(LibraryError::NoSuchTrack)
    ));
    assert_eq!(order(&library), [c, b]);
    library.clear(list).unwrap();
    assert!(order(&library).is_empty());
}

#[test]
fn the_history_counts_plays_per_session() {
    let mut library = library("history");
    let a = add(&mut library, "/a", "A", "X");
    let first = library.open_session(Timestamp(100)).unwrap();
    library
        .record_played(first, a, DeckId::A, Timestamp(110), 300.0)
        .unwrap();
    let second = library.open_session(Timestamp(200)).unwrap();
    library
        .record_played(second, a, DeckId::B, Timestamp(220), f64::NAN)
        .unwrap();

    let sessions: Vec<_> = library.sessions().unwrap().iter().map(|s| s.id).collect();
    assert_eq!(sessions, [second, first]);
    let track = library.track(a).unwrap().unwrap();
    assert_eq!(track.play_count, 2);
    assert_eq!(track.last_played, Some(Timestamp(220)));
    assert_eq!(library.history(second).unwrap()[0].seconds, 0.0);

    library.reset_played(a).unwrap();
    let track = library.track(a).unwrap().unwrap();
    assert_eq!((track.play_count, track.last_played), (0, None));
    assert_eq!(library.history(first).unwrap().len(), 1);
}

#[test]
fn search_finds_every_word_in_any_alphabet_and_takes_wildcards_as_text() {
    let mut library = library("search");
    add(&mut library, "/1", "École de nuit", "Artiste");
    add(&mut library, "/2", "ÉCOLE remix", "Other");
    add(&mut library, "/3", "100% Pure_Drive", "Producer");
    add(&mut library, "/4", "100 Pure Drive", "Producer");

    assert_eq!(
        titles(&library, &text("école")),
        ["École de nuit", "ÉCOLE remix"]
    );
    assert_eq!(titles(&library, &text("éCOLE artiste")), ["École de nuit"]);
    assert_eq!(titles(&library, &text("100%")), ["100% Pure_Drive"]);
    assert_eq!(titles(&library, &text("pure_")), ["100% Pure_Drive"]);
    assert_eq!(
        titles(&library, &text("'; DROP TABLE track; --")),
        Vec::<String>::new()
    );
    assert_eq!(library.search(&text("")).unwrap().total, 4);
}

#[test]
fn search_sorts_with_missing_values_last_and_pages() {
    let mut library = library("sort");
    for (path, title, bpm) in [
        ("/a", "Slow", Some(90.0)),
        ("/b", "None", None),
        ("/c", "Fast", Some(140.0)),
    ] {
        library
            .import(
                Path::new(path),
                &TrackTags {
                    title: Some(title.into()),
                    bpm,
                    ..TrackTags::default()
                },
                Timestamp(1),
            )
            .unwrap();
    }
    let by_tempo = |descending| Query {
        column: Column::Tempo,
        descending,
        ..Query::default()
    };
    assert_eq!(titles(&library, &by_tempo(false)), ["Slow", "Fast", "None"]);
    assert_eq!(titles(&library, &by_tempo(true)), ["Fast", "Slow", "None"]);

    let page = library
        .search(&Query {
            column: Column::Title,
            offset: 1,
            limit: 1,
            ..Query::default()
        })
        .unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].track.tags.title.as_deref(), Some("None"));
}

#[test]
fn search_in_a_playlist_gives_its_entries_in_its_order() {
    let mut library = library("search-playlist");
    let a = add(&mut library, "/a", "Alpha", "X");
    let b = add(&mut library, "/b", "Beta", "X");
    add(&mut library, "/c", "Gamma", "X");
    let list = library.create(None, NodeKind::Playlist, "Set").unwrap();
    library.add(list, &[b, a, b], None).unwrap();
    let query = Query {
        playlist: Some(list),
        ..Query::default()
    };
    let page = library.search(&query).unwrap();
    let found: Vec<_> = page.hits.iter().map(|hit| hit.track.id).collect();
    assert_eq!(found, [b, a, b]);
    let entries: Vec<_> = library
        .entries(list)
        .unwrap()
        .iter()
        .map(|e| Some(e.id))
        .collect();
    let hit_entries: Vec<_> = page.hits.iter().map(|hit| hit.entry).collect();
    assert_eq!(hit_entries, entries);
    assert_eq!(
        titles(
            &library,
            &Query {
                text: "alp".into(),
                ..query
            }
        ),
        ["Alpha"]
    );
}

#[test]
fn the_library_survives_reopening() {
    let directory = directory("reopen");
    let file = directory.join("library.db");
    let id = {
        let mut library = SqliteLibrary::open(&file).unwrap();
        add(&mut library, "/kept", "Kept", "X")
    };
    let library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(
        library.track(id).unwrap().unwrap().tags.title.as_deref(),
        Some("Kept")
    );
}

#[test]
fn a_file_that_is_not_a_library_is_refused_unchanged() {
    let directory = directory("foreign");
    let text = directory.join("notes.txt");
    let contents = b"these are not the tables you are looking for".repeat(20);
    std::fs::write(&text, &contents).unwrap();
    assert!(matches!(
        SqliteLibrary::open(&text),
        Err(LibraryError::Store(_))
    ));
    assert_eq!(std::fs::read(&text).unwrap(), contents);

    let other = directory.join("other.db");
    {
        let connection = rusqlite::Connection::open(&other).unwrap();
        connection
            .execute_batch("CREATE TABLE something (x INTEGER);")
            .unwrap();
    }
    let before = std::fs::read(&other).unwrap();
    assert!(matches!(
        SqliteLibrary::open(&other),
        Err(LibraryError::NotALibrary)
    ));
    assert_eq!(std::fs::read(&other).unwrap(), before);
}

#[test]
fn a_library_from_a_newer_version_is_refused_unchanged() {
    let directory = directory("newer");
    let file = directory.join("library.db");
    drop(SqliteLibrary::open(&file).unwrap());
    {
        let connection = rusqlite::Connection::open(&file).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
    }
    let before = std::fs::read(&file).unwrap();
    assert!(matches!(
        SqliteLibrary::open(&file),
        Err(LibraryError::Newer { version: 2 })
    ));
    assert_eq!(std::fs::read(&file).unwrap(), before);
}
