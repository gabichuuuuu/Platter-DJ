//! The library store on files in a temporary directory, with invented tracks. Behaviour note:
//! `docs/behavior/library.md`.

// The helpers that build the libraries are not tests themselves.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use platter_core::{DeckId, Stem, TrackMemory};
use platter_library::{
    Column, Library, LibraryError, MAX_KEY, MAX_TAG_CHARS, NodeId, NodeKind, Query, SessionId,
    Source, SqliteLibrary, StemFolderError, Timestamp, TrackId, TrackTags, find_stems,
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
fn a_history_is_searched_in_the_order_of_play_without_the_tracks_removed_since() {
    let mut library = library("history-search");
    let a = add(&mut library, "/a", "Alpha", "X");
    let b = add(&mut library, "/b", "Beta", "Y");
    let c = add(&mut library, "/c", "Gamma", "X");
    let first = library.open_session(Timestamp(100)).unwrap();
    let empty = library.open_session(Timestamp(200)).unwrap();
    let second = library.open_session(Timestamp(300)).unwrap();
    // Recorded out of the order they started in; Beta twice, and twice in the same second.
    for (track, started) in [(b, 320), (a, 310), (b, 320), (c, 330)] {
        library
            .record_played(second, track, DeckId::A, Timestamp(started), 40.0)
            .unwrap();
    }
    library
        .record_played(first, c, DeckId::B, Timestamp(110), 40.0)
        .unwrap();
    library.remove_track(c).unwrap();

    let history = |session, column, descending| Query {
        source: Source::History(session),
        column,
        descending,
        ..Query::default()
    };
    let order = history(second, Column::Order, false);
    assert_eq!(titles(&library, &order), ["Alpha", "Beta", "Beta"]);
    let page = library.search(&order).unwrap();
    assert_eq!(page.total, 3);
    assert!(page.hits.iter().all(|hit| hit.entry.is_none()));
    assert_eq!(
        titles(&library, &history(second, Column::Order, true)),
        ["Beta", "Beta", "Alpha"]
    );
    assert_eq!(
        titles(&library, &history(second, Column::Title, true)),
        ["Beta", "Beta", "Alpha"]
    );
    assert_eq!(
        titles(
            &library,
            &Query {
                text: "y".into(),
                ..order.clone()
            }
        ),
        ["Beta", "Beta"]
    );
    assert!(titles(&library, &history(first, Column::Order, false)).is_empty());
    assert!(
        titles(
            &library,
            &history(SessionId::new(999), Column::Order, false)
        )
        .is_empty()
    );

    let counts: Vec<_> = library
        .sessions()
        .unwrap()
        .iter()
        .map(|session| (session.id, session.played))
        .collect();
    assert_eq!(counts, [(second, 3), (empty, 0), (first, 0)]);
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
        source: Source::Playlist(list),
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
        connection.pragma_update(None, "user_version", 8).unwrap();
    }
    let before = std::fs::read(&file).unwrap();
    assert!(matches!(
        SqliteLibrary::open(&file),
        Err(LibraryError::Newer { version: 8 })
    ));
    assert_eq!(std::fs::read(&file).unwrap(), before);
}

#[test]
fn a_library_of_the_first_version_is_brought_up_to_date_with_what_it_held() {
    let directory = directory("upgrade");
    let file = directory.join("library.db");
    let list = {
        let mut library = SqliteLibrary::open(&file).unwrap();
        let a = add(&mut library, "/a", "Alpha", "X");
        let list = library.create(None, NodeKind::Playlist, "Set").unwrap();
        library.add(list, &[a], None).unwrap();
        list
    };
    // The file as the first version wrote it: without the preparation list, the favourites
    // and the colour, rating and comment of a track.
    {
        let connection = rusqlite::Connection::open(&file).unwrap();
        connection
            .execute_batch(
                "DROP TABLE stem; DROP TABLE rule; DROP TABLE preparation; DROP TABLE favourite; ALTER TABLE track DROP COLUMN colour; ALTER TABLE track DROP COLUMN rating; ALTER TABLE track DROP COLUMN comment; ALTER TABLE track DROP COLUMN memory; ALTER TABLE track DROP COLUMN key; ALTER TABLE track DROP COLUMN loudness; ALTER TABLE track DROP COLUMN analysed; PRAGMA user_version = 1;",
            )
            .unwrap();
    }
    let mut library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.entries(list).unwrap().len(), 1);
    assert_eq!(library.preparation().unwrap(), None);
    assert!(library.favourites().unwrap().is_empty());
    library.set_preparation(list).unwrap();
    library.set_favourite(4, list).unwrap();
    drop(library);
    let library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.preparation().unwrap(), Some(list));
    assert_eq!(library.favourites().unwrap(), [(4, list)]);
    let version = |file: &Path| -> i64 {
        rusqlite::Connection::open(file)
            .unwrap()
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap()
    };
    assert_eq!(version(&file), 7);

    // A file of the second version keeps its preparation list and gains the favourites.
    {
        let connection = rusqlite::Connection::open(&file).unwrap();
        connection
            .execute_batch(
                "DROP TABLE stem; DROP TABLE rule; DROP TABLE favourite; ALTER TABLE track DROP COLUMN colour; ALTER TABLE track DROP COLUMN rating; ALTER TABLE track DROP COLUMN comment; ALTER TABLE track DROP COLUMN memory; ALTER TABLE track DROP COLUMN key; ALTER TABLE track DROP COLUMN loudness; ALTER TABLE track DROP COLUMN analysed; PRAGMA user_version = 2;",
            )
            .unwrap();
    }
    let library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.preparation().unwrap(), Some(list));
    assert!(library.favourites().unwrap().is_empty());
    assert_eq!(version(&file), 7);

    // A file of the fourth version keeps its colours and gains what a deck learns.
    let track = library.track_at(Path::new("/a")).unwrap().unwrap().id;
    drop(library);
    {
        let connection = rusqlite::Connection::open(&file).unwrap();
        connection
            .execute_batch(
                "DROP TABLE stem; DROP TABLE rule; UPDATE track SET colour = 3; ALTER TABLE track DROP COLUMN memory; ALTER TABLE track DROP COLUMN key; ALTER TABLE track DROP COLUMN loudness; ALTER TABLE track DROP COLUMN analysed; PRAGMA user_version = 4;",
            )
            .unwrap();
    }
    let mut library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(version(&file), 7);
    let kept = library.track_at(Path::new("/a")).unwrap().unwrap();
    assert_eq!((kept.colour, kept.key, kept.loudness), (3, None, None));
    library.set_analysis(track, Some(21), Some(-8.5)).unwrap();
    assert_eq!(library.memory_at(Path::new("/a")).unwrap(), None);
}

#[test]
fn a_playlist_is_a_favourite_in_one_place_at_most() {
    let mut library = library("favourites");
    let folder = library.create(None, NodeKind::Folder, "Crate").unwrap();
    let first = library.create(None, NodeKind::Playlist, "First").unwrap();
    let second = library.create(None, NodeKind::Playlist, "Second").unwrap();
    library.set_favourite(4, first).unwrap();
    library.set_favourite(5, second).unwrap();
    assert_eq!(library.favourites().unwrap(), [(4, first), (5, second)]);
    // Into another place: it leaves the first one.
    library.set_favourite(6, first).unwrap();
    assert_eq!(library.favourites().unwrap(), [(5, second), (6, first)]);
    // Into a taken place: the one there goes out.
    library.set_favourite(5, first).unwrap();
    assert_eq!(library.favourites().unwrap(), [(5, first)]);
    assert!(matches!(
        library.set_favourite(7, folder),
        Err(LibraryError::WrongKind)
    ));
    library.clear_favourite(5).unwrap();
    library.clear_favourite(8).unwrap();
    assert!(library.favourites().unwrap().is_empty());
    library.set_favourite(4, second).unwrap();
    library.delete_node(second).unwrap();
    assert!(library.favourites().unwrap().is_empty());
}

#[test]
fn one_playlist_at_most_is_the_preparation_list() {
    let mut library = library("preparation");
    let folder = library.create(None, NodeKind::Folder, "Crate").unwrap();
    let first = library.create(None, NodeKind::Playlist, "First").unwrap();
    let second = library.create(None, NodeKind::Playlist, "Second").unwrap();
    assert_eq!(library.preparation().unwrap(), None);
    library.set_preparation(first).unwrap();
    library.set_preparation(second).unwrap();
    assert_eq!(library.preparation().unwrap(), Some(second));
    assert!(matches!(
        library.set_preparation(folder),
        Err(LibraryError::WrongKind)
    ));
    assert!(matches!(
        library.set_preparation(NodeId::new(999)),
        Err(LibraryError::NoSuchNode)
    ));
    assert_eq!(library.preparation().unwrap(), Some(second));
    library.delete_node(second).unwrap();
    assert_eq!(library.preparation().unwrap(), None);
}

#[test]
fn a_track_keeps_its_colour_rating_and_comment_through_a_new_import() {
    let mut library = library("colour-rating");
    let a = add(&mut library, "/a", "Alpha", "X");
    let fresh = library.track(a).unwrap().unwrap();
    assert_eq!((fresh.colour, fresh.rating, fresh.comment), (0, 0, None));
    library.set_colour(a, 8).unwrap();
    library.set_rating(a, 5).unwrap();
    library.set_comment(a, Some("long intro")).unwrap();
    assert!(matches!(
        library.set_colour(a, 9),
        Err(LibraryError::OutOfRange)
    ));
    assert!(matches!(
        library.set_rating(a, 6),
        Err(LibraryError::OutOfRange)
    ));
    let gone = add(&mut library, "/gone", "Gone", "X");
    library.remove_track(gone).unwrap();
    assert!(matches!(
        library.set_rating(gone, 1),
        Err(LibraryError::NoSuchTrack)
    ));
    // Importing the file again updates its tags and keeps what the DJ gave it.
    add(&mut library, "/a", "Alpha 2", "X");
    let track = library.track(a).unwrap().unwrap();
    assert_eq!(track.tags.title.as_deref(), Some("Alpha 2"));
    assert_eq!(
        (track.colour, track.rating, track.comment.as_deref()),
        (8, 5, Some("long intro"))
    );
    library.set_colour(a, 0).unwrap();
    library.set_comment(a, Some("")).unwrap();
    let track = library.track(a).unwrap().unwrap();
    assert_eq!((track.colour, track.comment), (0, None));
}

fn memory() -> TrackMemory {
    TrackMemory::parse(
        "platter-track 1\nbpm 124\nmarker 0.25\ncue 1 cue 12.5 0 Intro\ncue 3 loop 40 7.5 Break\n",
    )
    .unwrap()
}

#[test]
fn the_grid_and_the_cues_of_a_track_are_kept_and_give_it_their_tempo() {
    let mut library = library("memory");
    let a = add(&mut library, "/a", "Alpha", "X");
    let b = add(&mut library, "/b", "Beta", "Y");
    assert_eq!(library.memory_at(Path::new("/a")).unwrap(), None);
    library.set_memory(a, &memory()).unwrap();
    assert_eq!(library.memory_at(Path::new("/a")).unwrap(), Some(memory()));
    assert_eq!(library.memory_at(Path::new("/b")).unwrap(), None);
    assert_eq!(library.memory_at(Path::new("/elsewhere")).unwrap(), None);
    let track = library.track_at(Path::new("/a")).unwrap().unwrap();
    assert_eq!(track.tags.bpm, Some(124.0));

    // A grid without a tempo keeps the tempo of the record as it is.
    let mut no_tempo = memory();
    no_tempo.grid = platter_core::BeatGrid::default();
    library.set_memory(a, &no_tempo).unwrap();
    assert_eq!(library.memory_at(Path::new("/a")).unwrap(), Some(no_tempo));
    assert_eq!(
        library.track_at(Path::new("/a")).unwrap().unwrap().tags.bpm,
        Some(124.0)
    );

    // Importing the file again keeps what was kept.
    library.set_memory(b, &memory()).unwrap();
    add(&mut library, "/b", "Beta again", "Y");
    assert_eq!(library.memory_at(Path::new("/b")).unwrap(), Some(memory()));
    library.remove_track(a).unwrap();
    assert!(matches!(
        library.set_memory(a, &memory()),
        Err(LibraryError::NoSuchTrack)
    ));
}

#[test]
fn a_kept_text_this_version_cannot_read_is_left_as_it_is() {
    let directory = directory("memory-newer");
    let file = directory.join("library.db");
    let mut library = SqliteLibrary::open(&file).unwrap();
    add(&mut library, "/a", "Alpha", "X");
    drop(library);
    let written = "platter-track 2\nwhatever comes next\n";
    {
        let connection = rusqlite::Connection::open(&file).unwrap();
        connection
            .execute("UPDATE track SET memory = ?1", [written])
            .unwrap();
    }
    let library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.memory_at(Path::new("/a")).unwrap(), None);
    drop(library);
    let connection = rusqlite::Connection::open(&file).unwrap();
    let still: String = connection
        .query_row("SELECT memory FROM track", [], |row| row.get(0))
        .unwrap();
    assert_eq!(still, written);
}

#[test]
fn the_key_and_the_loudness_of_a_track_are_kept_within_their_range() {
    let mut library = library("analysis");
    let a = add(&mut library, "/a", "Alpha", "X");
    let track = library.track_at(Path::new("/a")).unwrap().unwrap();
    assert_eq!((track.key, track.loudness), (None, None));
    library.set_analysis(a, Some(MAX_KEY), Some(-9.25)).unwrap();
    let track = library.track_at(Path::new("/a")).unwrap().unwrap();
    assert_eq!((track.key, track.loudness), (Some(23), Some(-9.25)));
    for (key, loudness) in [
        (Some(24), None),
        (None, Some(f64::NAN)),
        (None, Some(f64::INFINITY)),
    ] {
        assert!(matches!(
            library.set_analysis(a, key, loudness),
            Err(LibraryError::OutOfRange)
        ));
    }
    library.set_analysis(a, None, None).unwrap();
    let track = library.track_at(Path::new("/a")).unwrap().unwrap();
    assert_eq!((track.key, track.loudness), (None, None));
}

#[test]
fn the_tracks_to_analyse_come_newest_first_until_they_are_analysed() {
    let directory = directory("unanalysed");
    let mut library = SqliteLibrary::open(&directory.join("library.db")).unwrap();
    let tags = TrackTags::default();
    let (old_path, new_path) = (directory.join("old.wav"), directory.join("new.wav"));
    std::fs::write(&old_path, b"x").unwrap();
    std::fs::write(&new_path, b"x").unwrap();
    let gone_path = directory.join("gone.wav");
    let old = library.import(&old_path, &tags, Timestamp(10)).unwrap();
    let new = library.import(&new_path, &tags, Timestamp(20)).unwrap();
    library.import(&gone_path, &tags, Timestamp(30)).unwrap();
    let paths = |library: &SqliteLibrary| -> Vec<PathBuf> {
        library
            .unanalysed(10)
            .unwrap()
            .into_iter()
            .map(|track| track.path)
            .collect()
    };
    assert_eq!(library.count_unanalysed().unwrap(), 3);
    assert_eq!(paths(&library)[0], gone_path);
    // A track whose file is gone waits until it comes back.
    library.check_files().unwrap();
    assert_eq!(paths(&library), [new_path.clone(), old_path.clone()]);
    assert_eq!(library.unanalysed(1).unwrap().len(), 1);
    // Nothing found still counts as analysed.
    library.set_analysis(new, None, None).unwrap();
    library.set_analysis(old, Some(3), Some(-10.0)).unwrap();
    assert!(paths(&library).is_empty());
    assert_eq!(library.count_unanalysed().unwrap(), 0);
}

#[test]
fn keys_sort_round_the_circle_of_fifths_major_before_minor() {
    let mut library = library("key-order");
    // C major is 1d, A minor 1m, G major 2d, F major 12d; one track has no key.
    for (path, title, key) in [
        ("/g", "G major", Some(7)),
        ("/none", "None", None),
        ("/a", "A minor", Some(21)),
        ("/f", "F major", Some(5)),
        ("/c", "C major", Some(0)),
    ] {
        let id = library
            .import(
                Path::new(path),
                &TrackTags {
                    title: Some(title.into()),
                    ..TrackTags::default()
                },
                Timestamp(1),
            )
            .unwrap();
        library.set_analysis(id, key, None).unwrap();
    }
    let by_key = Query {
        column: Column::Key,
        ..Query::default()
    };
    assert_eq!(
        titles(&library, &by_key),
        ["C major", "A minor", "G major", "F major", "None"]
    );
}

#[test]
fn ratings_and_colours_sort_from_the_first_with_none_last_either_way() {
    let mut library = library("rating-colour-order");
    for (path, title, rating, colour) in [
        ("/a", "Three, blue", 3, 6),
        ("/b", "None", 0, 0),
        ("/c", "Five, red", 5, 1),
        ("/d", "One, green", 1, 4),
    ] {
        let id = add(&mut library, path, title, "Artist");
        library.set_rating(id, rating).unwrap();
        library.set_colour(id, colour).unwrap();
    }
    let by = |column, descending| Query {
        column,
        descending,
        ..Query::default()
    };
    assert_eq!(
        titles(&library, &by(Column::Rating, false)),
        ["One, green", "Three, blue", "Five, red", "None"]
    );
    assert_eq!(
        titles(&library, &by(Column::Rating, true)),
        ["Five, red", "Three, blue", "One, green", "None"]
    );
    assert_eq!(
        titles(&library, &by(Column::Colour, false)),
        ["Five, red", "One, green", "Three, blue", "None"]
    );
    assert_eq!(
        titles(&library, &by(Column::Colour, true)),
        ["Three, blue", "One, green", "Five, red", "None"]
    );
}

#[test]
fn deleting_a_session_forgets_its_history_and_keeps_the_tracks_and_their_counts() {
    let mut library = library("delete-session");
    let a = add(&mut library, "/a", "A", "X");
    let first = library.open_session(Timestamp(100)).unwrap();
    library
        .record_played(first, a, DeckId::A, Timestamp(110), 300.0)
        .unwrap();
    let second = library.open_session(Timestamp(200)).unwrap();
    library
        .record_played(second, a, DeckId::B, Timestamp(220), 60.0)
        .unwrap();

    library.delete_session(first).unwrap();
    let sessions: Vec<_> = library.sessions().unwrap().iter().map(|s| s.id).collect();
    assert_eq!(sessions, [second]);
    assert!(library.history(first).unwrap().is_empty());
    assert_eq!(library.history(second).unwrap().len(), 1);
    assert_eq!(library.track(a).unwrap().unwrap().play_count, 2);
    // A session the library does not have: nothing changes.
    library.delete_session(first).unwrap();
    assert_eq!(library.sessions().unwrap().len(), 1);
}

fn stem_paths(folder: &str) -> [PathBuf; Stem::COUNT] {
    Stem::ALL.map(|stem| PathBuf::from(format!("/{folder}/{stem}.wav")))
}

#[test]
fn a_track_keeps_its_stems_until_they_are_forgotten_or_it_is_removed() {
    let directory = directory("stems");
    let file = directory.join("library.db");
    let mut library = SqliteLibrary::open(&file).unwrap();
    let a = add(&mut library, "/a", "A", "X");
    let b = add(&mut library, "/b", "B", "X");
    assert_eq!(library.stems(a).unwrap(), None);
    assert!(!library.track(a).unwrap().unwrap().stems);

    library.set_stems(a, Some(&stem_paths("one"))).unwrap();
    library.set_stems(a, Some(&stem_paths("two"))).unwrap();
    drop(library);
    let mut library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.stems(a).unwrap(), Some(stem_paths("two")));
    assert!(library.track(a).unwrap().unwrap().stems);
    assert!(library.search(&text("A")).unwrap().hits[0].track.stems);
    assert!(!library.track(b).unwrap().unwrap().stems);

    library.set_stems(a, None).unwrap();
    assert_eq!(library.stems(a).unwrap(), None);
    library.set_stems(b, Some(&stem_paths("b"))).unwrap();
    library.remove_track(b).unwrap();
    assert_eq!(library.stems(b).unwrap(), None);
    assert!(matches!(
        library.set_stems(b, Some(&stem_paths("b"))),
        Err(LibraryError::NoSuchTrack)
    ));
}

#[test]
fn a_folder_gives_the_four_audio_files_named_after_the_stems() {
    let folder = directory("stem-folder");
    for name in [
        "Drums.wav",
        "bass.flac",
        "other.wav",
        "VOCALS.mp3",
        "drums.png",
        "mix.wav",
    ] {
        std::fs::write(folder.join(name), b"").unwrap();
    }
    std::fs::create_dir_all(folder.join("other")).unwrap();
    let audio = |path: &Path| path.extension().is_some_and(|extension| extension != "png");
    let found = find_stems(&folder, audio).unwrap();
    let names = found.map(|path| path.file_name().unwrap().to_string_lossy().into_owned());
    assert_eq!(names, ["Drums.wav", "bass.flac", "other.wav", "VOCALS.mp3"]);

    std::fs::write(folder.join("Bass.wav"), b"").unwrap();
    assert!(matches!(
        find_stems(&folder, audio),
        Err(StemFolderError::Twice(Stem::Bass))
    ));
    std::fs::remove_file(folder.join("Bass.wav")).unwrap();
    std::fs::remove_file(folder.join("other.wav")).unwrap();
    assert!(matches!(
        find_stems(&folder, audio),
        Err(StemFolderError::Missing(Stem::Other))
    ));
    assert!(matches!(
        find_stems(&folder.join("absent"), audio),
        Err(StemFolderError::Unreadable(_))
    ));
}
