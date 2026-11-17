//! The collection in the browser, against a library and folders written by the test.
//!
//! Behaviour notes: `docs/behavior/browser.md`, *The collection*, *Playlists*, *Played state*,
//! *History*, *Preparation list*, *Favourites*, *Deleting tracks*, *Collection maintenance*,
//! *Playlist files*, *Colour and rating* and *Track details*.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use platter_core::{DeckId, TrackMemory};
use platter_engine::Command;
use platter_library::{Library, NodeId, NodeKind, Query, SqliteLibrary, Timestamp, TrackTags};
use platter_ui_egui::collection::{Collection, Kept, TagWriting};
use platter_ui_egui::command::{
    Action, AppCommand, BrowserNode, DetailFields, DetailsThen, NewItem, SortColumn, TrackDetails,
};
use platter_ui_egui::files::{FileBrowser, Root};
use platter_ui_egui::model::{
    CollectionView, ImportOutcome, MaintenanceOutcome, PlaylistFileOutcome, RowKind,
};

/// A WAV file of a tenth of a second of silence.
fn wav() -> Vec<u8> {
    let frames = 4_410u32;
    let data = frames * 4;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&44_100u32.to_le_bytes());
    bytes.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data.to_le_bytes());
    bytes.resize(bytes.len() + data as usize, 0);
    bytes
}

/// A folder of the test run with music in `music` (two tracks, a damaged file, a text
/// file and a subfolder with one track) and room for the library file.
fn place(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    let music = root.join("music");
    std::fs::create_dir_all(music.join("sets")).unwrap();
    std::fs::write(music.join("b-side.wav"), wav()).unwrap();
    std::fs::write(music.join("a-side.wav"), wav()).unwrap();
    std::fs::write(music.join("broken.flac"), b"not audio").unwrap();
    std::fs::write(music.join("notes.txt"), b"text").unwrap();
    std::fs::write(music.join("sets").join("opener.wav"), wav()).unwrap();
    root
}

fn browser(root: &Path, library: PathBuf) -> FileBrowser {
    let mut browser = FileBrowser::new(vec![Root {
        name: "Music".to_owned(),
        path: root.join("music"),
    }]);
    browser.attach(Collection::start(move || {
        Ok(Box::new(SqliteLibrary::open(&library)?) as Box<dyn Library>)
    }));
    browser
}

fn collection(browser: &FileBrowser) -> CollectionView {
    browser.view().collection.unwrap()
}

/// Polls until `done` holds, as the application does frame by frame.
fn wait(browser: &mut FileBrowser, done: impl Fn(&FileBrowser) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(browser) {
        assert!(Instant::now() < deadline, "the library never answered");
        browser.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn import(browser: &mut FileBrowser, folder: PathBuf) -> ImportOutcome {
    browser.apply(&AppCommand::LibraryImportFolder { path: folder });
    assert!(collection(browser).importing);
    wait(browser, |browser| !browser.collection_busy());
    collection(browser).imported.unwrap()
}

fn show_collection(browser: &mut FileBrowser) {
    browser.apply(&AppCommand::BrowserNodeSelect {
        node: BrowserNode::Collection,
    });
}

#[test]
fn an_imported_folder_fills_the_collection_shown_in_the_list() {
    let root = place("collection-import");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    let opened = collection(&browser);
    assert_eq!(opened.available, Some(true));
    assert_eq!(opened.tracks, Some(0));

    let outcome = import(&mut browser, root.join("music"));
    assert_eq!(
        outcome,
        ImportOutcome::Done {
            added: 3,
            known: 0,
            unreadable: 1,
            stopped: false,
        }
    );
    assert_eq!(collection(&browser).tracks, Some(3));

    show_collection(&mut browser);
    let view = browser.view();
    assert!(view.collection.as_ref().unwrap().current);
    assert_eq!(view.folder, None);
    // Sorted by path: the files of the folder, then those of the folder inside it.
    let names: Vec<_> = view.rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["a-side.wav", "b-side.wav", "opener.wav"]);
    let RowKind::Track(track) = &view.rows[0].kind else {
        panic!("{:?}", view.rows[0]);
    };
    assert_eq!(track.sample_rate, 44_100);
    assert!(track.format.is_empty() && !track.missing);
    assert_eq!(view.selected, Some(0));

    // Importing again keeps what the collection knows.
    let again = import(&mut browser, root.join("music"));
    assert_eq!(
        again,
        ImportOutcome::Done {
            added: 0,
            known: 3,
            unreadable: 1,
            stopped: false,
        }
    );
}

#[test]
fn a_track_of_the_collection_loads_and_a_folder_leaves_the_collection() {
    let root = place("collection-load");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    import(&mut browser, root.join("music"));
    show_collection(&mut browser);
    browser.apply(&AppCommand::BrowserMove { delta: 1 });
    let load = browser.apply(&AppCommand::BrowserLoadSelected { deck: DeckId::B });
    let Some(Action::Engine(Command::DeckLoad { deck, path })) = load else {
        panic!("{load:?}");
    };
    assert_eq!(deck, DeckId::B);
    assert_eq!(path.file_name().unwrap(), "b-side.wav");
    // Backspace has no folder above the collection.
    browser.apply(&AppCommand::BrowserUp);
    assert!(collection(&browser).current);

    browser.navigate(root.join("music").join("sets"));
    wait(&mut browser, |browser| !browser.view().reading);
    let view = browser.view();
    assert!(!view.collection.unwrap().current);
    assert_eq!(view.folder, Some(root.join("music").join("sets")));
    assert_eq!(view.rows.len(), 1);
}

#[test]
fn a_library_that_cannot_be_opened_leaves_the_explorer_working() {
    let root = place("collection-unavailable");
    // A folder where the library file should be: it cannot be opened as one.
    let library = root.join("library.sqlite");
    std::fs::create_dir_all(&library).unwrap();
    let mut browser = browser(&root, library);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(collection(&browser).available, Some(false));
    assert_eq!(collection(&browser).tracks, None);

    browser.apply(&AppCommand::LibraryImportFolder {
        path: root.join("music"),
    });
    assert!(!collection(&browser).importing);
    show_collection(&mut browser);
    assert!(browser.view().rows.is_empty());

    browser.navigate(root.join("music"));
    wait(&mut browser, |browser| !browser.view().reading);
    assert!(!browser.view().rows.is_empty());
}

#[test]
fn a_browser_without_a_library_has_no_collection_node() {
    let root = place("collection-none");
    let mut browser = FileBrowser::new(vec![Root {
        name: "Music".to_owned(),
        path: root.join("music"),
    }]);
    assert!(browser.view().collection.is_none());
    show_collection(&mut browser);
    browser.navigate(root.join("music"));
    wait(&mut browser, |browser| !browser.view().reading);
    assert_eq!(browser.view().rows.len(), 4);
}

fn search(browser: &mut FileBrowser, text: &str) {
    browser.apply(&AppCommand::BrowserSearchSet {
        text: text.to_owned(),
    });
    wait(browser, |browser| !browser.collection_busy());
}

fn names(browser: &FileBrowser) -> Vec<String> {
    browser
        .view()
        .rows
        .iter()
        .map(|row| row.name.clone())
        .collect()
}

#[test]
fn the_search_keeps_the_tracks_of_the_collection_that_match_every_word() {
    let root = place("collection-search");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    import(&mut browser, root.join("music"));
    show_collection(&mut browser);

    // The tracks have no title tag: the library finds nothing by the words of a file name.
    search(&mut browser, "  ");
    assert_eq!(names(&browser).len(), 3);
    search(&mut browser, "nothing like it");
    assert!(names(&browser).is_empty());
    let view = browser.view();
    assert_eq!(view.collection.as_ref().unwrap().matched, Some(0));
    assert_eq!(view.collection.unwrap().tracks, Some(3));
    assert_eq!(view.selected, None);
    search(&mut browser, "");
    assert_eq!(names(&browser).len(), 3);
    assert_eq!(browser.view().selected, Some(0));
}

#[test]
fn only_the_answer_to_the_last_text_reaches_the_list() {
    let root = place("collection-search-order");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    import(&mut browser, root.join("music"));
    show_collection(&mut browser);
    // Typed faster than the library answers: the earlier texts are skipped or dropped.
    for text in ["n", "no", "non", "none of it", ""] {
        browser.apply(&AppCommand::BrowserSearchSet {
            text: text.to_owned(),
        });
    }
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser).len(), 3);
    assert_eq!(browser.view().search, "");
}

#[test]
fn the_search_filters_the_rows_of_a_folder_by_name_and_follows_the_list() {
    let root = place("collection-search-folder");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    browser.navigate(root.join("music"));
    wait(&mut browser, |browser| {
        let view = browser.view();
        !view.reading && view.rows.iter().all(|row| row.kind != RowKind::Pending)
    });
    search(&mut browser, "SIDE wav");
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav"]);
    assert_eq!(browser.view().selected, Some(0));
    // The folder is kept when its name matches.
    search(&mut browser, "set");
    assert_eq!(names(&browser), ["sets"]);
    search(&mut browser, "zzz");
    assert!(names(&browser).is_empty());
    assert_eq!(browser.view().selected, None);
    assert_eq!(
        browser.apply(&AppCommand::BrowserLoadSelected { deck: DeckId::A }),
        None
    );

    // The same text applies to the collection when it is shown.
    import(&mut browser, root.join("music"));
    search(&mut browser, "");
    show_collection(&mut browser);
    assert_eq!(names(&browser).len(), 3);
}

#[test]
fn a_column_name_sorts_a_folder_and_again_turns_the_order_round() {
    let root = place("collection-sort-folder");
    let mut browser = browser(&root, root.join("library.sqlite"));
    browser.navigate(root.join("music"));
    wait(&mut browser, |browser| {
        let view = browser.view();
        !view.reading && view.rows.iter().all(|row| row.kind != RowKind::Pending)
    });
    browser.apply(&AppCommand::BrowserMove { delta: 1 });
    assert_eq!(names(&browser)[1], "a-side.wav");

    let sort = AppCommand::BrowserSort {
        column: SortColumn::Title,
    };
    browser.apply(&sort);
    // The folder first; files without a title by their name; the damaged one has no value.
    assert_eq!(
        names(&browser),
        ["sets", "a-side.wav", "b-side.wav", "broken.flac"]
    );
    browser.apply(&sort);
    assert_eq!(browser.view().sort, Some((SortColumn::Title, true)));
    assert_eq!(
        names(&browser),
        ["sets", "b-side.wav", "a-side.wav", "broken.flac"]
    );
    // The selection stayed on its row.
    assert_eq!(browser.view().selected, Some(2));
}

#[test]
fn the_library_sorts_the_whole_collection_by_the_column_chosen() {
    let root = place("collection-sort");
    let library = root.join("library.sqlite");
    {
        let mut store = SqliteLibrary::open(&library).unwrap();
        let music = std::fs::canonicalize(root.join("music")).unwrap();
        for (file, title, seconds) in [
            ("a-side.wav", Some("Zebra"), 200.0),
            ("b-side.wav", None, 100.0),
            ("sets/opener.wav", Some("apple"), 300.0),
        ] {
            let tags = platter_library::TrackTags {
                title: title.map(str::to_owned),
                seconds: Some(seconds),
                ..platter_library::TrackTags::default()
            };
            store
                .import(&music.join(file), &tags, platter_library::Timestamp(0))
                .unwrap();
        }
    }
    let mut browser = browser(&root, library);
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav", "opener.wav"]);

    let by = |column| AppCommand::BrowserSort { column };
    browser.apply(&by(SortColumn::Title));
    wait(&mut browser, |browser| !browser.collection_busy());
    // Case is ignored, and the track without a title comes last.
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav", "b-side.wav"]);
    browser.apply(&by(SortColumn::Time));
    browser.apply(&by(SortColumn::Time));
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav", "b-side.wav"]);
    // The order stays with a search typed.
    search(&mut browser, "e");
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav"]);
}

/// The playlists the tests show: a folder with an empty playlist in it, an empty playlist and
/// one that holds two tracks twice each, four entries against three tracks in the collection.
struct Seeded {
    archive: NodeId,
    friday: NodeId,
}

/// A library with the three tracks of `music`, titled as the sorting test titles them, and
/// the playlists of [`Seeded`]; `only_first` imports the first track alone.
fn seed(root: &Path, only_first: bool) -> Seeded {
    let mut store = SqliteLibrary::open(&root.join("library.sqlite")).unwrap();
    // The folder as the importer keeps it: without the prefix Windows gives a canonical path.
    let canonical = std::fs::canonicalize(root.join("music")).unwrap();
    let music = canonical
        .to_str()
        .and_then(|path| path.strip_prefix(r"\\?\"))
        .map_or_else(|| canonical.clone(), PathBuf::from);
    let mut tracks = Vec::new();
    for (file, title, seconds) in [
        ("a-side.wav", Some("Zebra"), 200.0),
        ("b-side.wav", None, 100.0),
        ("sets/opener.wav", Some("apple"), 300.0),
    ] {
        if only_first && !tracks.is_empty() {
            break;
        }
        let tags = TrackTags {
            title: title.map(str::to_owned),
            seconds: Some(seconds),
            ..TrackTags::default()
        };
        tracks.push(
            store
                .import(&music.join(file), &tags, Timestamp(0))
                .unwrap(),
        );
    }
    let archive = store.create(None, NodeKind::Folder, "Archive").unwrap();
    store
        .create(Some(archive), NodeKind::Playlist, "Old")
        .unwrap();
    store.create(None, NodeKind::Playlist, "Empty").unwrap();
    let friday = store.create(None, NodeKind::Playlist, "Friday").unwrap();
    let first = tracks[0];
    let last = *tracks.last().unwrap();
    store
        .add(friday, &[last, first, last, first], None)
        .unwrap();
    Seeded { archive, friday }
}

fn show_playlist(browser: &mut FileBrowser, playlist: NodeId) {
    browser.apply(&AppCommand::BrowserPlaylistSelect { playlist });
    wait(browser, |browser| !browser.collection_busy());
}

#[test]
fn the_tree_lists_the_folders_and_playlists_of_the_library_by_name() {
    let root = place("playlists-tree");
    seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    let tree: Vec<_> = collection(&browser)
        .playlists
        .iter()
        .map(|row| (row.name.clone(), row.depth, row.folder, row.entries))
        .collect();
    assert_eq!(
        tree,
        [
            ("Archive".to_owned(), 0, true, None),
            ("Old".to_owned(), 1, false, Some(0)),
            ("Empty".to_owned(), 0, false, Some(0)),
            ("Friday".to_owned(), 0, false, Some(4)),
        ]
    );
}

#[test]
fn a_playlist_shows_its_entries_in_its_own_order_with_the_search_and_the_sorting() {
    let root = place("playlists-show");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_playlist(&mut browser, seeded.friday);
    // A track in the playlist twice is listed twice.
    assert_eq!(
        names(&browser),
        ["opener.wav", "a-side.wav", "opener.wav", "a-side.wav"]
    );
    let shown = collection(&browser);
    assert_eq!(shown.playlist, Some(seeded.friday));
    assert!(!shown.current);
    assert!(
        shown
            .playlists
            .iter()
            .any(|row| row.current && row.name == "Friday")
    );
    // The collection keeps its own count.
    assert_eq!(shown.tracks, Some(3));
    assert_eq!(shown.matched, Some(4));

    search(&mut browser, "apple");
    assert_eq!(names(&browser), ["opener.wav", "opener.wav"]);
    assert_eq!(collection(&browser).matched, Some(2));
    search(&mut browser, "");
    browser.apply(&AppCommand::BrowserSort {
        column: SortColumn::Title,
    });
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(
        names(&browser),
        ["opener.wav", "opener.wav", "a-side.wav", "a-side.wav"]
    );

    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav", "b-side.wav"]);
    let shown = collection(&browser);
    assert!(shown.current && shown.playlist.is_none());
    assert!(shown.playlists.iter().all(|row| !row.current));
}

#[test]
fn an_import_while_a_playlist_is_shown_counts_the_whole_collection() {
    let root = place("playlists-import");
    let seeded = seed(&root, true);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(collection(&browser).tracks, Some(1));
    show_playlist(&mut browser, seeded.friday);
    assert_eq!(names(&browser), ["a-side.wav"; 4]);
    assert_eq!(collection(&browser).tracks, Some(1));

    import(&mut browser, root.join("music"));
    assert_eq!(collection(&browser).tracks, Some(3));
    assert_eq!(names(&browser), ["a-side.wav"; 4]);
}

#[test]
fn a_playlist_that_is_gone_or_a_folder_shows_an_empty_list() {
    let root = place("playlists-gone");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    for playlist in [NodeId::new(9_999), seeded.archive] {
        show_playlist(&mut browser, seeded.friday);
        assert_eq!(names(&browser).len(), 4);
        show_playlist(&mut browser, playlist);
        assert!(names(&browser).is_empty());
        let shown = collection(&browser);
        assert_eq!(shown.available, Some(true));
        assert_eq!(shown.matched, Some(0));
        assert_eq!(shown.playlists.len(), 4);
    }
    // The library still answers.
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser).len(), 3);
}

/// The names of the tree with the entries of each playlist.
fn tree(browser: &FileBrowser) -> Vec<(String, Option<usize>)> {
    collection(browser)
        .playlists
        .iter()
        .map(|row| (row.name.clone(), row.entries))
        .collect()
}

fn change(browser: &mut FileBrowser, command: AppCommand) {
    browser.apply(&command);
    wait(browser, |browser| !browser.collection_busy());
}

#[test]
fn a_deleted_playlist_leaves_the_tree_and_the_list_shows_the_collection_instead() {
    let root = place("playlists-delete");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_playlist(&mut browser, seeded.friday);
    browser.apply(&AppCommand::BrowserSelect { index: 3 });

    change(
        &mut browser,
        AppCommand::BrowserPlaylistDelete {
            playlist: seeded.friday,
        },
    );
    assert_eq!(
        tree(&browser),
        [
            ("Archive".to_owned(), None),
            ("Old".to_owned(), Some(0)),
            ("Empty".to_owned(), Some(0)),
        ]
    );
    let shown = collection(&browser);
    assert!(shown.current && shown.playlist.is_none());
    assert_eq!(shown.tracks, Some(3));
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav", "opener.wav"]);
    assert_eq!(browser.view().selected, Some(0));

    // A playlist that is not shown goes from the tree; the list stays as it was.
    let empty = shown.playlists[2].id;
    change(
        &mut browser,
        AppCommand::BrowserPlaylistDelete { playlist: empty },
    );
    assert_eq!(tree(&browser).len(), 2);
    assert!(collection(&browser).current);
    assert_eq!(names(&browser).len(), 3);
}

#[test]
fn the_command_that_deletes_a_playlist_leaves_a_folder_and_what_it_holds() {
    let root = place("playlists-delete-folder");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_playlist(&mut browser, seeded.friday);
    change(
        &mut browser,
        AppCommand::BrowserPlaylistDelete {
            playlist: seeded.archive,
        },
    );
    assert_eq!(
        tree(&browser),
        [
            ("Archive".to_owned(), None),
            ("Old".to_owned(), Some(0)),
            ("Empty".to_owned(), Some(0)),
            ("Friday".to_owned(), Some(4)),
        ]
    );
    assert_eq!(collection(&browser).available, Some(true));
    assert_eq!(names(&browser).len(), 4);
}

#[test]
fn clearing_a_playlist_takes_out_every_entry_and_keeps_the_collection() {
    let root = place("playlists-clear");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_playlist(&mut browser, seeded.friday);
    change(
        &mut browser,
        AppCommand::BrowserPlaylistClear {
            playlist: seeded.friday,
        },
    );
    assert!(names(&browser).is_empty());
    let shown = collection(&browser);
    assert_eq!(shown.playlist, Some(seeded.friday));
    assert_eq!(shown.matched, Some(0));
    assert_eq!(shown.tracks, Some(3));
    assert_eq!(tree(&browser)[3], ("Friday".to_owned(), Some(0)));
    assert_eq!(browser.view().selected, None);
}

#[test]
fn removing_the_repeats_keeps_the_first_entry_of_each_track() {
    let root = place("playlists-repeats");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_playlist(&mut browser, seeded.friday);
    change(
        &mut browser,
        AppCommand::BrowserPlaylistRemoveRepeats {
            playlist: seeded.friday,
        },
    );
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav"]);
    assert_eq!(tree(&browser)[3], ("Friday".to_owned(), Some(2)));
    assert_eq!(collection(&browser).tracks, Some(3));
}

#[test]
fn removing_the_selected_entry_takes_out_that_entry_alone() {
    let root = place("playlists-remove");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    // In the collection there is no entry to take out.
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    change(&mut browser, AppCommand::BrowserPlaylistRemove);
    assert_eq!(names(&browser).len(), 3);

    show_playlist(&mut browser, seeded.friday);
    // The second entry of the opener; taking out the first entry of its track instead would
    // leave the a-side first.
    browser.apply(&AppCommand::BrowserSelect { index: 2 });
    change(&mut browser, AppCommand::BrowserPlaylistRemove);
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav", "a-side.wav"]);
    assert_eq!(browser.view().selected, Some(2));
    assert_eq!(tree(&browser)[3], ("Friday".to_owned(), Some(3)));
    assert_eq!(collection(&browser).tracks, Some(3));

    // The selection stays on its place, or on the last row once that place is gone.
    change(&mut browser, AppCommand::BrowserPlaylistRemove);
    assert_eq!(names(&browser), ["opener.wav", "a-side.wav"]);
    assert_eq!(browser.view().selected, Some(1));
}

#[test]
fn a_change_to_a_playlist_that_does_not_exist_leaves_the_library_available() {
    let root = place("playlists-change-gone");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    // With a playlist shown, a library taken for unavailable would not get its count of
    // tracks back from the page that follows.
    show_playlist(&mut browser, seeded.friday);
    let playlist = NodeId::new(9_999);
    for command in [
        AppCommand::BrowserPlaylistDelete { playlist },
        AppCommand::BrowserPlaylistClear { playlist },
        AppCommand::BrowserPlaylistRemoveRepeats { playlist },
    ] {
        change(&mut browser, command);
        let shown = collection(&browser);
        assert_eq!(shown.available, Some(true));
        assert_eq!(shown.tracks, Some(3));
        assert_eq!(shown.playlists.len(), 4);
        assert_eq!(names(&browser).len(), 4);
    }
}

fn depths(browser: &FileBrowser) -> Vec<(String, usize, bool)> {
    collection(browser)
        .playlists
        .iter()
        .map(|row| (row.name.clone(), row.depth, row.folder))
        .collect()
}

#[test]
fn a_new_playlist_goes_where_it_was_asked_for_and_is_shown() {
    let root = place("playlists-create");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    change(
        &mut browser,
        AppCommand::BrowserPlaylistCreate {
            folder: Some(seeded.archive),
            item: NewItem::Playlist,
            name: "Afterhours".to_owned(),
        },
    );
    let shown = collection(&browser);
    let created = shown
        .playlists
        .iter()
        .find(|row| row.name == "Afterhours")
        .unwrap();
    assert_eq!((created.depth, created.entries), (1, Some(0)));
    // The new playlist is shown, empty, and the collection keeps its count.
    assert_eq!(shown.playlist, Some(created.id));
    assert!(names(&browser).is_empty());
    assert_eq!(shown.tracks, Some(3));

    change(
        &mut browser,
        AppCommand::BrowserPlaylistCreate {
            folder: None,
            item: NewItem::Folder,
            name: "Gigs".to_owned(),
        },
    );
    // A folder is not shown: the list stays on the playlist.
    assert_eq!(collection(&browser).playlist, Some(created.id));
    assert_eq!(
        depths(&browser),
        [
            ("Archive".to_owned(), 0, true),
            ("Afterhours".to_owned(), 1, false),
            ("Old".to_owned(), 1, false),
            ("Empty".to_owned(), 0, false),
            ("Friday".to_owned(), 0, false),
            ("Gigs".to_owned(), 0, true),
        ]
    );
}

#[test]
fn a_playlist_or_a_folder_takes_another_name_and_an_empty_one_changes_nothing() {
    let root = place("playlists-rename");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    change(
        &mut browser,
        AppCommand::BrowserPlaylistRename {
            item: seeded.friday,
            name: "Monday".to_owned(),
        },
    );
    change(
        &mut browser,
        AppCommand::BrowserPlaylistRename {
            item: seeded.archive,
            name: "Attic".to_owned(),
        },
    );
    assert_eq!(
        depths(&browser),
        [
            ("Attic".to_owned(), 0, true),
            ("Old".to_owned(), 1, false),
            ("Empty".to_owned(), 0, false),
            ("Monday".to_owned(), 0, false),
        ]
    );
    for name in ["", &"x".repeat(201)] {
        change(
            &mut browser,
            AppCommand::BrowserPlaylistRename {
                item: seeded.friday,
                name: name.to_owned(),
            },
        );
        change(
            &mut browser,
            AppCommand::BrowserPlaylistCreate {
                folder: None,
                item: NewItem::Playlist,
                name: name.to_owned(),
            },
        );
    }
    assert_eq!(depths(&browser).len(), 4);
    assert!(depths(&browser).iter().any(|row| row.0 == "Monday"));
    assert_eq!(collection(&browser).available, Some(true));
}

#[test]
fn a_track_added_to_a_playlist_goes_at_its_end_and_a_new_file_enters_the_collection() {
    let root = place("playlists-add");
    let seeded = seed(&root, true);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_playlist(&mut browser, seeded.friday);
    let add = |path: PathBuf| AppCommand::BrowserPlaylistAdd {
        playlist: seeded.friday,
        path,
    };
    // A file of the explorer that the collection does not hold yet.
    change(&mut browser, add(root.join("music").join("b-side.wav")));
    assert_eq!(
        names(&browser),
        [
            "a-side.wav",
            "a-side.wav",
            "a-side.wav",
            "a-side.wav",
            "b-side.wav"
        ]
    );
    assert_eq!(collection(&browser).tracks, Some(2));
    // The same file again is the same track, once more in the playlist.
    change(&mut browser, add(root.join("music").join("b-side.wav")));
    assert_eq!(names(&browser).len(), 6);
    assert_eq!(collection(&browser).tracks, Some(2));
    // A file that is not music, a damaged one and a folder add nothing.
    for path in ["notes.txt", "broken.flac", "sets"] {
        change(&mut browser, add(root.join("music").join(path)));
    }
    assert_eq!(names(&browser).len(), 6);
    assert_eq!(collection(&browser).tracks, Some(2));
    // A folder of playlists takes no track, and the file is not taken into the collection.
    change(
        &mut browser,
        AppCommand::BrowserPlaylistAdd {
            playlist: seeded.archive,
            path: root.join("music").join("sets").join("opener.wav"),
        },
    );
    assert_eq!(collection(&browser).tracks, Some(2));
    assert_eq!(collection(&browser).available, Some(true));
}

#[test]
fn a_playlist_moves_into_a_folder_and_a_folder_not_into_itself() {
    let root = place("playlists-move");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    change(
        &mut browser,
        AppCommand::BrowserPlaylistMove {
            item: seeded.friday,
            folder: Some(seeded.archive),
        },
    );
    assert_eq!(
        depths(&browser),
        [
            ("Archive".to_owned(), 0, true),
            ("Friday".to_owned(), 1, false),
            ("Old".to_owned(), 1, false),
            ("Empty".to_owned(), 0, false),
        ]
    );
    // A folder into itself changes nothing, and the library stays available.
    change(
        &mut browser,
        AppCommand::BrowserPlaylistMove {
            item: seeded.archive,
            folder: Some(seeded.archive),
        },
    );
    assert_eq!(depths(&browser).len(), 4);
    assert_eq!(collection(&browser).available, Some(true));
    change(
        &mut browser,
        AppCommand::BrowserPlaylistMove {
            item: seeded.friday,
            folder: None,
        },
    );
    assert_eq!(depths(&browser)[3], ("Friday".to_owned(), 0, false));
}

#[test]
fn a_track_played_in_this_session_is_marked_until_its_played_state_is_reset() {
    let root = place("played");
    seed(&root, true);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    let marked = |browser: &FileBrowser| -> Vec<bool> {
        browser
            .view()
            .rows
            .iter()
            .map(|row| matches!(&row.kind, RowKind::Track(track) if track.played))
            .collect()
    };
    assert_eq!(marked(&browser), [false]);
    // Started now, after the session opened, as a deck starts a track.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let played = |path: PathBuf| platter_ui_egui::played::Played {
        deck: DeckId::A,
        path,
        started: now,
        seconds: 30.0,
    };
    // A track of the collection, and a file of the explorer that enters the collection.
    let a_side = browser.view().rows[0].path.clone();
    browser.played(played(a_side));
    wait(&mut browser, |browser| !browser.collection_busy());
    browser.played(played(root.join("music").join("b-side.wav")));
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav"]);
    assert_eq!(marked(&browser), [true, true]);
    assert_eq!(collection(&browser).tracks, Some(2));

    browser.apply(&AppCommand::BrowserSelect { index: 1 });
    change(&mut browser, AppCommand::BrowserPlayedReset);
    assert_eq!(marked(&browser), [true, false]);
}

#[test]
fn the_history_lists_what_was_played_in_this_session_and_in_earlier_ones() {
    let root = place("history");
    seed(&root, false);
    // An earlier session where two tracks were played, and one where nothing was.
    let earlier = {
        let mut store = SqliteLibrary::open(&root.join("library.sqlite")).unwrap();
        let tracks: Vec<_> = store
            .search(&Query::default())
            .unwrap()
            .hits
            .into_iter()
            .map(|hit| hit.track.id)
            .collect();
        let earlier = store.open_session(Timestamp(100)).unwrap();
        for (track, started) in [(tracks[2], 120), (tracks[0], 110)] {
            store
                .record_played(earlier, track, DeckId::B, Timestamp(started), 60.0)
                .unwrap();
        }
        store.open_session(Timestamp(200)).unwrap();
        earlier
    };
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    let opened = collection(&browser);
    assert_eq!(opened.session.as_ref().map(|row| row.tracks), Some(0));
    let listed: Vec<_> = opened
        .sessions
        .iter()
        .map(|row| (row.id, row.label.as_str(), row.tracks))
        .collect();
    assert_eq!(listed, [(earlier, "1970-01-01 00:01", 2)]);

    // An earlier session, in the order its tracks started playing.
    change(
        &mut browser,
        AppCommand::BrowserHistorySelect { session: earlier },
    );
    assert_eq!(names(&browser), ["a-side.wav", "opener.wav"]);
    let shown = collection(&browser);
    assert_eq!(shown.history, Some(earlier));
    assert!(shown.sessions[0].current && !shown.current);
    assert_eq!(shown.tracks, Some(3));
    search(&mut browser, "apple");
    assert_eq!(names(&browser), ["opener.wav"]);
    search(&mut browser, "");

    // The history of this run fills as tracks count as played.
    change(
        &mut browser,
        AppCommand::BrowserNodeSelect {
            node: BrowserNode::History,
        },
    );
    assert!(names(&browser).is_empty());
    assert!(collection(&browser).session.unwrap().current);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    browser.played(platter_ui_egui::played::Played {
        deck: DeckId::A,
        path: root.join("music").join("b-side.wav"),
        started: now,
        seconds: 30.0,
    });
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser), ["b-side.wav"]);
    let view = collection(&browser);
    assert_eq!(view.session.map(|row| row.tracks), Some(1));
    // A history page is not the whole collection: the count of the collection stays.
    assert_eq!(view.tracks, Some(3));

    // A session the library does not have shows nothing.
    change(
        &mut browser,
        AppCommand::BrowserHistorySelect {
            session: platter_library::SessionId::new(999),
        },
    );
    assert!(names(&browser).is_empty());
    assert_eq!(collection(&browser).available, Some(true));
}

#[test]
fn tracks_go_into_the_preparation_list_at_its_end_or_after_the_track_loaded_last() {
    let root = place("preparation");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    let prepared = |browser: &FileBrowser| -> Vec<bool> {
        browser
            .view()
            .rows
            .iter()
            .map(|row| matches!(&row.kind, RowKind::Track(track) if track.prepared))
            .collect()
    };
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav", "opener.wav"]);
    assert_eq!(prepared(&browser), [false, false, false]);
    // With no preparation list, adding to it does nothing.
    browser.apply(&AppCommand::BrowserSelect { index: 1 });
    change(&mut browser, AppCommand::BrowserPreparationAppend);
    let opener = browser.view().rows[2].path.clone();

    // Friday holds the opener and the a-side, each twice.
    change(
        &mut browser,
        AppCommand::BrowserPreparationSelect {
            playlist: seeded.friday,
        },
    );
    assert!(
        collection(&browser)
            .playlists
            .iter()
            .all(|row| row.preparation == (row.id == seeded.friday))
    );
    assert_eq!(prepared(&browser), [true, false, true]);

    browser.apply(&AppCommand::BrowserSelect { index: 1 });
    change(&mut browser, AppCommand::BrowserPreparationAppend);
    assert_eq!(prepared(&browser), [true, true, true]);
    // No track loaded yet: as next means at the start.
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    change(&mut browser, AppCommand::BrowserPreparationNext);
    // After the last entry of the track loaded last.
    browser.loaded(opener);
    browser.apply(&AppCommand::BrowserSelect { index: 1 });
    change(&mut browser, AppCommand::BrowserPreparationNext);

    // The Preparation node shows it.
    change(
        &mut browser,
        AppCommand::BrowserNodeSelect {
            node: BrowserNode::Preparation,
        },
    );
    assert_eq!(collection(&browser).playlist, Some(seeded.friday));
    assert_eq!(
        names(&browser),
        [
            "a-side.wav",
            "opener.wav",
            "a-side.wav",
            "opener.wav",
            "b-side.wav",
            "a-side.wav",
            "b-side.wav",
        ]
    );
}

#[test]
fn a_favourite_keeps_one_playlist_until_it_is_freed_or_the_playlist_deleted() {
    let root = place("favourites");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    let favourites = |browser: &FileBrowser| -> Vec<(String, usize)> {
        collection(browser)
            .playlists
            .iter()
            .filter_map(|row| row.favourite.map(|place| (row.name.clone(), place)))
            .collect()
    };
    assert!(favourites(&browser).is_empty());
    change(
        &mut browser,
        AppCommand::BrowserFavouriteSet {
            place: 4,
            playlist: seeded.friday,
        },
    );
    assert_eq!(favourites(&browser), [("Friday".to_owned(), 4)]);
    // A folder cannot be one; the library keeps what it had and stays available.
    change(
        &mut browser,
        AppCommand::BrowserFavouriteSet {
            place: 5,
            playlist: seeded.archive,
        },
    );
    assert_eq!(favourites(&browser), [("Friday".to_owned(), 4)]);
    assert_eq!(collection(&browser).available, Some(true));
    change(&mut browser, AppCommand::BrowserFavouriteClear { place: 4 });
    assert!(favourites(&browser).is_empty());
    change(
        &mut browser,
        AppCommand::BrowserFavouriteSet {
            place: 8,
            playlist: seeded.friday,
        },
    );
    assert_eq!(favourites(&browser), [("Friday".to_owned(), 8)]);
    change(
        &mut browser,
        AppCommand::BrowserPlaylistDelete {
            playlist: seeded.friday,
        },
    );
    assert!(favourites(&browser).is_empty());
}

#[test]
fn a_deleted_track_leaves_the_collection_and_its_playlists_and_a_second_delete_changes_nothing() {
    let root = place("delete-track");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav", "opener.wav"]);
    let opener = browser.view().rows[2].path.clone();

    browser.apply(&AppCommand::BrowserSelect { index: 2 });
    change(&mut browser, AppCommand::BrowserTrackDelete);
    assert_eq!(names(&browser), ["a-side.wav", "b-side.wav"]);
    assert_eq!(collection(&browser).tracks, Some(2));
    // The file stays where it was.
    assert!(opener.is_file());
    // Friday held the opener twice; the a-side twice remains.
    let entries = |browser: &FileBrowser| {
        collection(browser)
            .playlists
            .iter()
            .find(|row| row.id == seeded.friday)
            .and_then(|row| row.entries)
    };
    assert_eq!(entries(&browser), Some(2));

    // The same row twice before the library answers: the second finds the track gone, which
    // changes nothing and leaves the library available.
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    browser.apply(&AppCommand::BrowserTrackDelete);
    browser.apply(&AppCommand::BrowserTrackDelete);
    // Not even for one frame does the list say the library is gone.
    wait(&mut browser, |browser| {
        assert_ne!(collection(browser).available, Some(false));
        !browser.collection_busy()
    });
    assert_eq!(names(&browser), ["b-side.wav"]);
    assert_eq!(collection(&browser).tracks, Some(1));
    assert_eq!(entries(&browser), Some(0));
}

#[test]
fn the_maintenance_checks_relocates_and_removes_the_missing_tracks() {
    let root = place("maintenance");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    import(&mut browser, root.join("music"));
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    browser.apply(&AppCommand::LibraryMaintenanceOpen);
    wait(&mut browser, |browser| {
        collection(browser).missing_total.is_some()
    });
    assert_eq!(collection(&browser).missing_total, Some(0));

    // Two files go away: one moved to another folder, one gone for good.
    let moved = root.join("moved");
    std::fs::create_dir_all(&moved).unwrap();
    std::fs::rename(
        root.join("music").join("a-side.wav"),
        moved.join("a-side.wav"),
    )
    .unwrap();
    std::fs::remove_file(root.join("music").join("b-side.wav")).unwrap();
    change(&mut browser, AppCommand::LibraryCheck);
    let view = collection(&browser);
    assert_eq!(
        view.maintained,
        Some(MaintenanceOutcome::Checked {
            missing: 2,
            found: 0
        })
    );
    assert_eq!(view.missing_total, Some(2));
    assert!(view.missing.iter().all(|path| path.ends_with("side.wav")));

    change(&mut browser, AppCommand::LibraryRelocate { path: moved });
    let view = collection(&browser);
    assert_eq!(
        view.maintained,
        Some(MaintenanceOutcome::Relocated {
            relocated: 1,
            not_found: 1,
            ambiguous: 0
        })
    );
    assert_eq!(view.missing_total, Some(1));

    change(&mut browser, AppCommand::LibraryMissingRemove);
    let view = collection(&browser);
    assert_eq!(view.maintained, Some(MaintenanceOutcome::Removed(1)));
    assert_eq!(view.missing_total, Some(0));
    assert_eq!(view.tracks, Some(2));
    assert_eq!(names(&browser), ["a-side.wav", "opener.wav"]);
}

#[test]
fn a_playlist_is_exported_without_replacing_a_file_and_imported_back() {
    let root = place("playlist-files");
    let seeded = seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    let exports = root.join("exports");
    std::fs::create_dir_all(&exports).unwrap();

    for _ in 0..2 {
        change(
            &mut browser,
            AppCommand::BrowserPlaylistExport {
                playlist: seeded.friday,
                path: exports.clone(),
            },
        );
    }
    assert_eq!(
        collection(&browser).exchanged,
        Some(PlaylistFileOutcome::Exported {
            file: "Friday (2).m3u8".to_owned(),
            entries: 4,
        })
    );
    let written = std::fs::read_to_string(exports.join("Friday.m3u8")).unwrap();
    assert!(written.starts_with("#EXTM3U\n"));
    assert_eq!(
        written
            .lines()
            .filter(|line| !line.starts_with('#'))
            .count(),
        4
    );
    // A third file names a track that is not there and a web address.
    std::fs::write(
        exports.join("Other.m3u"),
        "#EXTM3U\nnowhere/gone.wav\nhttp://example.invalid/a.mp3\n",
    )
    .unwrap();

    change(
        &mut browser,
        AppCommand::BrowserPlaylistImport {
            folder: Some(seeded.archive),
            path: exports,
        },
    );
    assert_eq!(
        collection(&browser).exchanged,
        Some(PlaylistFileOutcome::Imported {
            playlists: 3,
            tracks: 8,
            skipped: 2,
        })
    );
    let imported: Vec<_> = collection(&browser)
        .playlists
        .iter()
        .filter(|row| row.depth == 1 && !row.folder && row.name != "Old")
        .map(|row| (row.name.clone(), row.entries))
        .collect();
    assert_eq!(
        imported,
        [
            ("Friday".to_owned(), Some(4)),
            ("Friday (2)".to_owned(), Some(4)),
            ("Other".to_owned(), Some(0)),
        ]
    );
    // The tracks were in the collection already: none is added twice.
    assert_eq!(collection(&browser).tracks, Some(3));
}

#[test]
fn the_colour_and_the_rating_of_the_selected_track_show_in_its_row() {
    let root = place("colour-rating");
    seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    let tags = |browser: &FileBrowser| -> Vec<(u8, Option<u8>)> {
        browser
            .view()
            .rows
            .iter()
            .map(|row| match &row.kind {
                RowKind::Track(track) => (track.colour, track.rating),
                _ => (0, None),
            })
            .collect()
    };
    assert_eq!(tags(&browser), [(0, Some(0)); 3]);
    browser.apply(&AppCommand::BrowserSelect { index: 1 });
    change(
        &mut browser,
        AppCommand::BrowserTrackColourSet { colour: 6 },
    );
    change(
        &mut browser,
        AppCommand::BrowserTrackRatingSet { rating: 4 },
    );
    assert_eq!(tags(&browser), [(0, Some(0)), (6, Some(4)), (0, Some(0))]);
    // A value out of range changes nothing and leaves the library available.
    change(
        &mut browser,
        AppCommand::BrowserTrackRatingSet { rating: 9 },
    );
    assert_eq!(tags(&browser)[1], (6, Some(4)));
    assert_eq!(collection(&browser).available, Some(true));
}

#[test]
fn the_details_saved_for_the_selected_track_reach_its_row() {
    let root = place("details");
    seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    let details = TrackDetails {
        title: "Zebra Crossing".to_owned(),
        artist: "The Stripes".to_owned(),
        album: String::new(),
        genre: "Techno".to_owned(),
        comment: "Peak time".to_owned(),
        rating: 4,
    };
    change(
        &mut browser,
        AppCommand::BrowserTrackDetailsSave {
            details: details.clone(),
            fields: DetailFields::ALL,
            position: 0,
            then: DetailsThen::Close,
        },
    );
    let view = browser.view();
    let RowKind::Track(track) = &view.rows[0].kind else {
        panic!("{:?}", view.rows[0]);
    };
    assert_eq!(track.title.as_deref(), Some("Zebra Crossing"));
    assert_eq!(track.artist.as_deref(), Some("The Stripes"));
    assert_eq!(track.album, None);
    assert_eq!(track.genre.as_deref(), Some("Techno"));
    assert_eq!(track.comment.as_deref(), Some("Peak time"));
    assert_eq!(track.rating, Some(4));
    // The duration the file gave stays.
    assert_eq!(track.seconds, Some(200.0));

    // Saved for a track deleted meanwhile: nothing changes and the library stays available.
    browser.apply(&AppCommand::BrowserTrackDelete);
    browser.apply(&AppCommand::BrowserTrackDetailsSave {
        details,
        fields: DetailFields::ALL,
        position: 0,
        then: DetailsThen::Close,
    });
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(collection(&browser).available, Some(true));
    assert_eq!(collection(&browser).tracks, Some(2));
}

/// The file of a row, its title, its genre and its rating.
type Fields = (PathBuf, Option<String>, Option<String>, Option<u8>);

/// Behaviour note: `docs/behavior/browser.md`, *Track details*.
#[test]
fn details_saved_for_several_tracks_change_only_the_fields_named() {
    let root = place("details-several");
    seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    browser.apply(&AppCommand::BrowserSelectToggle { index: 1 });
    let titles = |browser: &FileBrowser| -> Vec<Fields> {
        browser
            .view()
            .rows
            .iter()
            .map(|row| {
                let RowKind::Track(track) = &row.kind else {
                    panic!("not a track");
                };
                (
                    row.path.clone(),
                    track.title.clone(),
                    track.genre.clone(),
                    track.rating,
                )
            })
            .collect()
    };
    let before = titles(&browser);
    let details = TrackDetails {
        title: "Not Written".to_owned(),
        artist: String::new(),
        album: String::new(),
        genre: "Electro".to_owned(),
        comment: String::new(),
        rating: 2,
    };
    // All the selected tracks take the genre and the rating, and keep their titles.
    change(
        &mut browser,
        AppCommand::BrowserTrackDetailsSave {
            details: details.clone(),
            fields: DetailFields::parse("genre,rating").unwrap(),
            position: 0,
            then: DetailsThen::Next,
        },
    );
    let selected: Vec<PathBuf> = before[..2].iter().map(|row| row.0.clone()).collect();
    let after = titles(&browser);
    assert_eq!(after.len(), before.len());
    for (path, title, genre, rating) in &after {
        let old = before.iter().find(|row| &row.0 == path).unwrap();
        assert_eq!(title, &old.1);
        if selected.contains(path) {
            assert_eq!(genre.as_deref(), Some("Electro"));
            assert_eq!(*rating, Some(2));
        } else {
            // The track that is not selected keeps everything.
            assert_eq!((genre, rating), (&old.2, &old.3));
        }
    }

    // From position 1 a save is for the first selected track alone, in the order of the list.
    let first = after
        .iter()
        .find(|row| selected.contains(&row.0))
        .unwrap()
        .0
        .clone();
    change(
        &mut browser,
        AppCommand::BrowserTrackDetailsSave {
            details: TrackDetails {
                genre: "House".to_owned(),
                ..details.clone()
            },
            fields: DetailFields::parse("genre").unwrap(),
            position: 1,
            then: DetailsThen::Close,
        },
    );
    for (path, _, genre, _) in titles(&browser)
        .into_iter()
        .filter(|row| selected.contains(&row.0))
    {
        let expected = if path == first { "House" } else { "Electro" };
        assert_eq!(genre.as_deref(), Some(expected), "{}", path.display());
    }

    // Nothing to write, or a position past the selection, changes nothing.
    let kept = titles(&browser);
    for (fields, position) in [(DetailFields::NONE, 0), (DetailFields::ALL, 3)] {
        browser.apply(&AppCommand::BrowserTrackDetailsSave {
            details: details.clone(),
            fields,
            position,
            then: DetailsThen::Close,
        });
        wait(&mut browser, |browser| !browser.collection_busy());
        assert_eq!(titles(&browser), kept);
    }
}

/// Behaviour note: `docs/behavior/browser.md`, *Track details*, edge cases.
#[test]
fn a_position_of_the_details_names_the_same_track_when_a_save_sorts_the_list_again() {
    let root = place("details-sorted");
    seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    change(
        &mut browser,
        AppCommand::BrowserSort {
            column: SortColumn::Title,
        },
    );
    let title_of = |browser: &FileBrowser, file: &str| {
        let view = browser.view();
        let row = view
            .rows
            .iter()
            .find(|row| row.path.ends_with(file))
            .unwrap()
            .clone();
        let RowKind::Track(track) = row.kind else {
            panic!("not a track");
        };
        track.title
    };
    // By title: apple (sets/opener.wav), Zebra (a-side.wav), then b-side.wav without one.
    let names: Vec<String> = browser
        .view()
        .rows
        .iter()
        .map(|row| row.name.clone())
        .collect();
    assert_eq!(names, ["opener.wav", "a-side.wav", "b-side.wav"]);
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    browser.apply(&AppCommand::BrowserSelectToggle { index: 1 });
    let save = |title: &str, position| AppCommand::BrowserTrackDetailsSave {
        details: TrackDetails {
            title: title.to_owned(),
            ..TrackDetails::default()
        },
        fields: DetailFields::parse("title").unwrap(),
        position,
        then: DetailsThen::Next,
    };
    // Position 1 is the first file, a-side.wav; its new title takes it to the top of the list.
    change(&mut browser, save("Aardvark", 1));
    assert_eq!(
        title_of(&browser, "a-side.wav").as_deref(),
        Some("Aardvark")
    );
    assert_eq!(browser.view().rows[0].name, "a-side.wav");
    // Position 2 is still sets/opener.wav, wherever the list now has it.
    change(&mut browser, save("Mango", 2));
    assert_eq!(title_of(&browser, "opener.wav").as_deref(), Some("Mango"));
    assert_eq!(
        title_of(&browser, "a-side.wav").as_deref(),
        Some("Aardvark")
    );
    assert_eq!(title_of(&browser, "b-side.wav"), None);
}

#[test]
fn what_a_deck_learned_is_kept_and_takes_a_track_into_the_collection() {
    let root = place("keep");
    let library = root.join("library.sqlite");
    let mut browser = browser(&root, library.clone());
    wait(&mut browser, |browser| !browser.collection_busy());
    let memory =
        TrackMemory::parse("platter-track 1\nbpm 126\nmarker 0.1\ncue 2 cue 3 0 Drop\n").unwrap();
    // A file of the explorer the collection does not hold yet.
    let canonical = std::fs::canonicalize(root.join("music").join("a-side.wav")).unwrap();
    // As the importer keeps it: without the prefix Windows gives a canonical path.
    let path = canonical
        .to_str()
        .and_then(|path| path.strip_prefix(r"\\?\"))
        .map_or_else(|| canonical.clone(), PathBuf::from);
    browser.keep(
        path.clone(),
        Kept {
            memory: Some(memory),
            analysis: Some((Some(7), Some(-11.0))),
        },
    );
    wait(&mut browser, |browser| !browser.collection_busy());
    // Only the key and the loudness come later: the grid and the cues stay.
    browser.keep(
        path.clone(),
        Kept {
            memory: None,
            analysis: Some((None, Some(-12.0))),
        },
    );
    wait(&mut browser, |browser| !browser.collection_busy());
    drop(browser);
    let store = SqliteLibrary::open(&library).unwrap();
    let track = store.track_at(&path).unwrap().unwrap();
    assert_eq!(store.memory_at(&path).unwrap(), Some(memory));
    assert_eq!(track.tags.bpm, Some(126.0));
    assert_eq!((track.key, track.loudness), (None, Some(-12.0)));
}

/// Whether the bytes of the file hold `text`. Platter's reader shows no tags of a WAV file
/// (`docs/behavior/tag-writing.md`), so the tests look at the bytes.
fn holds(path: &Path, text: &str) -> bool {
    let bytes = std::fs::read(path).unwrap();
    bytes
        .windows(text.len())
        .any(|window| window == text.as_bytes())
}

/// Whether the file holds Platter's own field with a text of a track.
fn holds_platter_text(path: &Path) -> bool {
    holds(path, "platter-track 1")
}

#[test]
fn saved_details_reach_the_file_unless_the_setting_says_nothing() {
    let root = place("details-file");
    seed(&root, false);
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    show_collection(&mut browser);
    wait(&mut browser, |browser| !browser.collection_busy());
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    let path = browser.view().rows[0].path.clone();
    let details = |title: &str| TrackDetails {
        title: title.to_owned(),
        artist: "The Stripes".to_owned(),
        album: String::new(),
        genre: String::new(),
        comment: String::new(),
        rating: 0,
    };
    change(
        &mut browser,
        AppCommand::BrowserTrackDetailsSave {
            details: details("Zebra Crossing"),
            fields: DetailFields::ALL,
            position: 0,
            then: DetailsThen::Close,
        },
    );
    assert!(holds(&path, "Zebra Crossing"));
    assert!(holds(&path, "The Stripes"));
    assert!(browser.take_unwritten().is_empty());

    browser.set_tag_writing(TagWriting::Nothing);
    change(
        &mut browser,
        AppCommand::BrowserTrackDetailsSave {
            details: details("Only In The Library"),
            fields: DetailFields::ALL,
            position: 0,
            then: DetailsThen::Close,
        },
    );
    assert!(holds(&path, "Zebra Crossing"));
    assert!(!holds(&path, "Only In The Library"));
    let RowKind::Track(track) = &browser.view().rows[0].kind else {
        panic!("not a track");
    };
    assert_eq!(track.title.as_deref(), Some("Only In The Library"));
}

#[test]
fn what_a_deck_learned_reaches_the_file_as_the_setting_lets_it() {
    let root = place("keep-file");
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());
    let memory =
        TrackMemory::parse("platter-track 1\nbpm 126\nmarker 0.1\ncue 2 cue 3 0 Drop\n").unwrap();
    let kept = Kept {
        memory: Some(memory),
        analysis: Some((Some(14), Some(-9.0))),
    };
    // Platter's own field alone: no tempo.
    let own = root.join("music").join("b-side.wav");
    browser.set_tag_writing(TagWriting::PlatterOnly);
    browser.keep(own.clone(), kept);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert!(holds_platter_text(&own));
    // ID3 writes the tempo in a frame of its own.
    assert!(!holds(&own, "TBPM"));

    // Everything: the tempo of the grid too.
    let all = root.join("music").join("a-side.wav");
    browser.set_tag_writing(TagWriting::Everything);
    browser.keep(all.clone(), kept);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert!(holds_platter_text(&all));
    assert!(holds(&all, "TBPM"));
    // D minor, as ID3 writes a key.
    assert!(holds(&all, "TKEY"));
    assert!(holds(&all, "Dm"));

    // Nothing: the file stays as it was.
    let untouched = root.join("music").join("sets").join("opener.wav");
    let before = std::fs::read(&untouched).unwrap();
    browser.set_tag_writing(TagWriting::Nothing);
    browser.keep(untouched.clone(), kept);
    wait(&mut browser, |browser| !browser.collection_busy());
    assert_eq!(std::fs::read(&untouched).unwrap(), before);

    // A file that cannot take the tags says why, and the collection keeps the change.
    let locked = root.join("music").join("locked.wav");
    std::fs::write(&locked, wav()).unwrap();
    let mut permissions = std::fs::metadata(&locked).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&locked, permissions).unwrap();
    browser.set_tag_writing(TagWriting::Everything);
    browser.keep(locked.clone(), kept);
    wait(&mut browser, |browser| !browser.collection_busy());
    let unwritten = browser.take_unwritten();
    assert_eq!(unwritten.len(), 1, "{unwritten:?}");
    assert!(!holds_platter_text(&locked));
    assert!(browser.take_unwritten().is_empty());
    drop(browser);
    let store = SqliteLibrary::open(&root.join("library.sqlite")).unwrap();
    let canonical = std::fs::canonicalize(&locked).unwrap();
    let stored = canonical
        .to_str()
        .and_then(|path| path.strip_prefix(r"\\?\"))
        .map_or_else(|| canonical.clone(), PathBuf::from);
    assert_eq!(store.memory_at(&stored).unwrap(), Some(memory));
}

#[test]
fn a_history_is_saved_as_a_playlist_exported_and_deleted() {
    let root = place("history-menu");
    seed(&root, false);
    // An earlier session where a track was played twice.
    let earlier = {
        let mut store = SqliteLibrary::open(&root.join("library.sqlite")).unwrap();
        let tracks: Vec<_> = store
            .search(&Query::default())
            .unwrap()
            .hits
            .into_iter()
            .map(|hit| hit.track.id)
            .collect();
        let earlier = store.open_session(Timestamp(100)).unwrap();
        for (track, started) in [(tracks[0], 110), (tracks[2], 120), (tracks[0], 130)] {
            store
                .record_played(earlier, track, DeckId::B, Timestamp(started), 60.0)
                .unwrap();
        }
        earlier
    };
    let mut browser = browser(&root, root.join("library.sqlite"));
    wait(&mut browser, |browser| !browser.collection_busy());

    change(
        &mut browser,
        AppCommand::BrowserHistoryToPlaylist { session: earlier },
    );
    let saved = collection(&browser)
        .playlists
        .into_iter()
        .find(|row| row.name == "1970-01-01 00:01")
        .unwrap();
    assert_eq!((saved.depth, saved.entries), (0, Some(3)));

    let exports = root.join("exports");
    std::fs::create_dir_all(&exports).unwrap();
    change(
        &mut browser,
        AppCommand::BrowserHistoryExport {
            session: earlier,
            path: exports.clone(),
        },
    );
    assert_eq!(
        collection(&browser).exchanged,
        Some(PlaylistFileOutcome::Exported {
            file: "1970-01-01 00_01.m3u8".to_owned(),
            entries: 3,
        })
    );

    // The history shown goes; the list turns to the collection, which keeps every track.
    change(
        &mut browser,
        AppCommand::BrowserHistorySelect { session: earlier },
    );
    assert_eq!(collection(&browser).history, Some(earlier));
    change(
        &mut browser,
        AppCommand::BrowserHistoryDelete { session: earlier },
    );
    let after = collection(&browser);
    assert!(after.sessions.is_empty(), "{:?}", after.sessions);
    assert_eq!(after.history, None);
    assert!(after.current);
    assert_eq!(after.tracks, Some(3));

    // The history of this run is not deleted.
    let this_run = after.session.unwrap().id;
    change(
        &mut browser,
        AppCommand::BrowserHistoryDelete { session: this_run },
    );
    assert_eq!(
        collection(&browser).session.map(|row| row.id),
        Some(this_run)
    );
}
