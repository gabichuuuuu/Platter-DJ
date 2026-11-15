//! The collection in the browser, against a library and folders written by the test.
//!
//! Behaviour note: `docs/behavior/browser.md`, *The collection*.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use platter_core::DeckId;
use platter_engine::Command;
use platter_library::{Library, SqliteLibrary};
use platter_ui_egui::collection::Collection;
use platter_ui_egui::command::{Action, AppCommand, BrowserNode};
use platter_ui_egui::files::{FileBrowser, Root};
use platter_ui_egui::model::{CollectionView, ImportOutcome, RowKind};

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
