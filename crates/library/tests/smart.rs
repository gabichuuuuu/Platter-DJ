//! Smart playlists: `docs/behavior/library.md`, *Smart playlists*. Libraries in temporary
//! directories, with invented tracks.

// The helpers that build the libraries are not tests themselves.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use platter_core::DeckId;
use platter_library::{
    Column, Library, LibraryError, MAX_RULES, NodeId, NodeKind, Query, RangeField, Rule, Source,
    SqliteLibrary, TagField, Timestamp, TrackId, TrackTags,
};

/// A fresh directory for one test; names are unique among the tests, which run in parallel.
fn directory(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("smart-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn add(
    library: &mut SqliteLibrary,
    path: &str,
    title: &str,
    genre: &str,
    bpm: Option<f64>,
) -> TrackId {
    let tags = TrackTags {
        title: Some(title.to_owned()),
        genre: Some(genre.to_owned()),
        bpm,
        ..TrackTags::default()
    };
    library
        .import(Path::new(path), &tags, Timestamp(1000))
        .unwrap()
}

fn titles(library: &SqliteLibrary, smart: NodeId) -> Vec<String> {
    titles_of(
        library,
        &Query {
            source: Source::Playlist(smart),
            column: Column::Title,
            ..Query::default()
        },
    )
}

fn titles_of(library: &SqliteLibrary, query: &Query) -> Vec<String> {
    library
        .search(query)
        .unwrap()
        .hits
        .into_iter()
        .filter_map(|hit| hit.track.tags.title)
        .collect()
}

fn smart(library: &mut SqliteLibrary, rules: &[Rule]) -> NodeId {
    let id = library.create(None, NodeKind::Smart, "Smart").unwrap();
    library.set_rules(id, rules).unwrap();
    id
}

fn tempo(low: Option<f64>, high: Option<f64>) -> Rule {
    Rule::Range {
        field: RangeField::Tempo,
        low,
        high,
    }
}

#[test]
fn a_track_must_pass_every_rule_and_follows_its_tags() {
    let mut library = SqliteLibrary::open(&directory("and").join("library.db")).unwrap();
    add(&mut library, "/a", "Alpha", "Deep House", Some(122.0));
    add(&mut library, "/b", "Bravo", "House", Some(128.0));
    let c = add(&mut library, "/c", "Charlie", "Techno", Some(124.0));
    let list = smart(
        &mut library,
        &[
            Rule::Words {
                field: TagField::Genre,
                words: "HOUSE".into(),
            },
            tempo(Some(120.0), Some(126.0)),
        ],
    );
    assert_eq!(titles(&library, list), ["Alpha"]);

    // Retagging a track moves it in at once, with no change to the smart playlist.
    library
        .import(
            Path::new("/c"),
            &TrackTags {
                title: Some("Charlie".into()),
                genre: Some("Tech House".into()),
                bpm: Some(124.0),
                ..TrackTags::default()
            },
            Timestamp(2000),
        )
        .unwrap();
    assert_eq!(titles(&library, list), ["Alpha", "Charlie"]);
    assert!(library.track(c).unwrap().is_some());
}

#[test]
fn ranges_include_their_bounds_and_leave_out_tracks_without_a_value() {
    let mut library = SqliteLibrary::open(&directory("ranges").join("library.db")).unwrap();
    add(&mut library, "/a", "Low", "x", Some(120.0));
    add(&mut library, "/b", "High", "x", Some(126.0));
    add(&mut library, "/c", "Out", "x", Some(126.01));
    add(&mut library, "/d", "None", "x", None);
    let list = smart(&mut library, &[tempo(Some(120.0), Some(126.0))]);
    assert_eq!(titles(&library, list), ["High", "Low"]);
    library
        .set_rules(list, &[tempo(None, Some(121.0))])
        .unwrap();
    assert_eq!(titles(&library, list), ["Low"]);
    library
        .set_rules(list, &[tempo(Some(126.0), None)])
        .unwrap();
    assert_eq!(titles(&library, list), ["High", "Out"]);
}

#[test]
fn plays_colours_keys_and_analysis_are_rules() {
    let mut library = SqliteLibrary::open(&directory("values").join("library.db")).unwrap();
    let a = add(&mut library, "/a", "Alpha", "x", None);
    let b = add(&mut library, "/b", "Bravo", "x", None);
    add(&mut library, "/c", "Charlie", "x", None);
    let session = library.open_session(Timestamp(5000)).unwrap();
    library
        .record_played(session, a, DeckId::A, Timestamp(5100), 60.0)
        .unwrap();
    library
        .record_played(session, a, DeckId::B, Timestamp(5200), 60.0)
        .unwrap();
    library
        .record_played(session, b, DeckId::A, Timestamp(5300), 60.0)
        .unwrap();
    library.set_colour(b, 4).unwrap();
    library.set_analysis(a, Some(21), None).unwrap();

    let list = smart(
        &mut library,
        &[Rule::Range {
            field: RangeField::PlayCount,
            low: Some(2.0),
            high: None,
        }],
    );
    assert_eq!(titles(&library, list), ["Alpha"]);
    let since = Rule::Range {
        field: RangeField::LastPlayed,
        low: Some(5250.0),
        high: None,
    };
    library.set_rules(list, &[since]).unwrap();
    assert_eq!(titles(&library, list), ["Bravo"]);
    library.set_rules(list, &[Rule::Colour(4)]).unwrap();
    assert_eq!(titles(&library, list), ["Bravo"]);
    library.set_rules(list, &[Rule::Key(21)]).unwrap();
    assert_eq!(titles(&library, list), ["Alpha"]);
    library.set_rules(list, &[Rule::Analysed(false)]).unwrap();
    assert_eq!(titles(&library, list), ["Bravo", "Charlie"]);
    // None of the invented files exists.
    library.check_files().unwrap();
    library.set_rules(list, &[Rule::Missing(false)]).unwrap();
    assert!(titles(&library, list).is_empty());
    library.set_rules(list, &[Rule::Missing(true)]).unwrap();
    assert_eq!(titles(&library, list).len(), 3);
}

#[test]
fn without_rules_it_holds_the_collection_and_searches_and_sorts_as_a_playlist() {
    let mut library = SqliteLibrary::open(&directory("search").join("library.db")).unwrap();
    add(&mut library, "/b", "Bravo", "Disco", Some(118.0));
    add(&mut library, "/a", "Alpha", "Disco", Some(124.0));
    add(&mut library, "/c", "Charlie", "Funk", Some(110.0));
    let list = smart(&mut library, &[]);
    let query = |text: &str, column, descending| Query {
        text: text.into(),
        source: Source::Playlist(list),
        column,
        descending,
        ..Query::default()
    };
    assert_eq!(
        titles_of(&library, &query("", Column::Order, false)),
        ["Alpha", "Bravo", "Charlie"]
    );
    assert_eq!(
        titles_of(&library, &query("disco", Column::Tempo, true)),
        ["Alpha", "Bravo"]
    );
    let page = library.search(&query("", Column::Order, false)).unwrap();
    assert_eq!(page.total, 3);
    assert!(page.hits.iter().all(|hit| hit.entry.is_none()));
}

#[test]
fn a_smart_playlist_takes_no_entries_and_is_no_preparation_list_or_favourite() {
    let mut library = SqliteLibrary::open(&directory("entries").join("library.db")).unwrap();
    let a = add(&mut library, "/a", "Alpha", "x", None);
    let list = smart(&mut library, &[]);
    let wrong = |result: Result<(), LibraryError>| {
        assert!(matches!(result, Err(LibraryError::WrongKind)), "{result:?}");
    };
    wrong(library.add(list, &[a], None));
    wrong(library.clear(list));
    wrong(library.remove_repeats(list));
    wrong(library.set_preparation(list));
    wrong(library.set_favourite(0, list));
    assert!(matches!(
        library.entries(list),
        Err(LibraryError::WrongKind)
    ));
    let playlist = library.create(None, NodeKind::Playlist, "Set").unwrap();
    assert!(matches!(
        library.rules(playlist),
        Err(LibraryError::WrongKind)
    ));
    wrong(library.set_rules(playlist, &[]));
}

#[test]
fn invalid_rules_change_nothing() {
    let mut library = SqliteLibrary::open(&directory("invalid").join("library.db")).unwrap();
    let kept = [Rule::Colour(1)];
    let list = smart(&mut library, &kept);
    let too_many = vec![Rule::Analysed(true); MAX_RULES + 1];
    for rules in [
        too_many.as_slice(),
        &[Rule::Colour(1), tempo(None, None)],
        &[tempo(Some(130.0), Some(120.0))],
        &[tempo(Some(f64::INFINITY), None)],
        &[Rule::Words {
            field: TagField::Title,
            words: " ".into(),
        }],
        &[Rule::Colour(9)],
    ] {
        assert!(matches!(
            library.set_rules(list, rules),
            Err(LibraryError::InvalidRule)
        ));
        assert_eq!(library.rules(list).unwrap(), kept);
    }
    library
        .set_rules(list, &vec![Rule::Analysed(true); MAX_RULES])
        .unwrap();
}

#[test]
fn rules_are_kept_in_order_and_go_with_their_folder() {
    let file = directory("kept").join("library.db");
    let rules = [
        Rule::Words {
            field: TagField::Artist,
            words: "Ñandú".into(),
        },
        tempo(Some(90.5), None),
        Rule::Key(0),
    ];
    let (folder, list) = {
        let mut library = SqliteLibrary::open(&file).unwrap();
        let folder = library.create(None, NodeKind::Folder, "Crate").unwrap();
        let list = library
            .create(Some(folder), NodeKind::Smart, "Slow")
            .unwrap();
        library.set_rules(list, &rules).unwrap();
        (folder, list)
    };
    let mut library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.rules(list).unwrap(), rules);
    let children = library.children(Some(folder)).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].kind, NodeKind::Smart);
    library.rename(list, "Slower").unwrap();
    library.delete_node(folder).unwrap();
    assert!(matches!(library.rules(list), Err(LibraryError::NoSuchNode)));
    drop(library);
    let rows: i64 = rusqlite::Connection::open(&file)
        .unwrap()
        .query_row("SELECT count(*) FROM rule", [], |row| row.get(0))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn a_rule_this_version_cannot_read_is_left_out() {
    let file = directory("unknown").join("library.db");
    let list = {
        let mut library = SqliteLibrary::open(&file).unwrap();
        add(&mut library, "/a", "Alpha", "x", None);
        smart(&mut library, &[Rule::Colour(2)])
    };
    {
        let connection = rusqlite::Connection::open(&file).unwrap();
        connection
            .execute_batch(
                "INSERT INTO rule (smart, position, field, low) \
                 SELECT smart, 1, 99, 1.0 FROM rule; \
                 INSERT INTO rule (smart, position, field, low) \
                 SELECT smart, 2, 20, 42.0 FROM rule WHERE field = 20;",
            )
            .unwrap();
    }
    let library = SqliteLibrary::open(&file).unwrap();
    assert_eq!(library.rules(list).unwrap(), [Rule::Colour(2)]);
    assert!(titles(&library, list).is_empty());
}
