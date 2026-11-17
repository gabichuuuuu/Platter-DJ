//! The file selector against folders written by the test: listing, selection, loading and
//! the tree.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use platter_core::DeckId;
use platter_engine::Command;

use platter_ui_egui::command::{Action, AppCommand};
use platter_ui_egui::files::{FileBrowser, Root};
use platter_ui_egui::model::RowKind;

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

/// A folder of the test run with two tracks, a damaged file, a text file and a
/// subfolder with one track.
fn library(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("sets")).unwrap();
    std::fs::create_dir_all(root.join(".hidden")).unwrap();
    std::fs::write(root.join("b-side.wav"), wav()).unwrap();
    std::fs::write(root.join("A-side.WAV"), wav()).unwrap();
    std::fs::write(root.join("broken.flac"), b"not audio").unwrap();
    std::fs::write(root.join("notes.txt"), b"text").unwrap();
    std::fs::write(root.join("sets").join("opener.wav"), wav()).unwrap();
    root
}

fn browser(root: &Path) -> FileBrowser {
    FileBrowser::new(vec![Root {
        name: "Library".to_owned(),
        path: root.to_path_buf(),
    }])
}

/// Polls until `done` holds, as the application does frame by frame.
fn wait(browser: &mut FileBrowser, done: impl Fn(&FileBrowser) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(browser) {
        assert!(Instant::now() < deadline, "the folder was never read");
        browser.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn all_probed(browser: &FileBrowser) -> bool {
    let view = browser.view();
    !view.reading && view.rows.iter().all(|row| row.kind != RowKind::Pending)
}

#[test]
fn a_folder_lists_its_folders_then_its_audio_files_sorted_by_name() {
    let root = library("files-list");
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    assert!(browser.view().reading);
    wait(&mut browser, all_probed);
    let view = browser.view();
    let names: Vec<&str> = view.rows.iter().map(|row| row.name.as_str()).collect();
    assert_eq!(names, ["sets", "A-side.WAV", "b-side.wav", "broken.flac"]);
    assert_eq!(view.rows[0].kind, RowKind::Folder);
    let RowKind::Track(track) = &view.rows[1].kind else {
        panic!("{:?}", view.rows[1].kind);
    };
    assert_eq!((track.format.as_str(), track.sample_rate), ("WAV", 44_100));
    assert!((track.seconds.unwrap() - 0.1).abs() < 1e-6);
    assert_eq!(view.rows[3].kind, RowKind::Unreadable);
    assert_eq!(view.file_count(), 3);
    assert_eq!(view.selected, Some(0));
    assert_eq!(view.crumbs, ["Library"]);
    // The tree shows the root with its count; the hidden folder is not listed.
    assert_eq!(view.tree.len(), 1);
    assert_eq!(view.tree[0].files, Some(3));
    assert!(view.tree[0].current);
}

#[test]
fn the_selection_moves_inside_the_list_and_opens_folders() {
    let root = library("files-move");
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    wait(&mut browser, all_probed);
    browser.apply(&AppCommand::BrowserMove { delta: -3 });
    assert_eq!(browser.view().selected, Some(0));
    browser.apply(&AppCommand::BrowserMove { delta: 2 });
    assert_eq!(browser.view().selected, Some(2));
    browser.apply(&AppCommand::BrowserMove { delta: 99 });
    assert_eq!(browser.view().selected, Some(3));
    browser.apply(&AppCommand::BrowserSelect { index: 99 });
    assert_eq!(browser.view().selected, Some(3));
    // Activating a track does nothing; activating a folder shows it.
    browser.apply(&AppCommand::BrowserActivate);
    assert_eq!(browser.current(), Some(root.as_path()));
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    browser.apply(&AppCommand::BrowserActivate);
    wait(&mut browser, all_probed);
    let view = browser.view();
    assert_eq!(view.crumbs, ["Library", "sets"]);
    assert_eq!(view.rows.len(), 1);
    // The tree opened down to the folder shown.
    assert_eq!(view.tree.len(), 2);
    assert!(view.tree[0].open && view.tree[1].current);
    assert_eq!(view.tree[1].depth, 1);
    // Up returns to the root, and stops there.
    browser.apply(&AppCommand::BrowserUp);
    browser.apply(&AppCommand::BrowserUp);
    assert_eq!(browser.current(), Some(root.as_path()));
}

#[test]
fn loading_the_selection_asks_the_engine_for_that_file() {
    let root = library("files-load");
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    wait(&mut browser, all_probed);
    let load = AppCommand::BrowserLoadSelected { deck: DeckId::B };
    // A folder is not a track.
    assert_eq!(browser.apply(&load), None);
    browser.apply(&AppCommand::BrowserSelect { index: 2 });
    assert_eq!(
        browser.apply(&load),
        Some(Action::Engine(Command::DeckLoad {
            deck: DeckId::B,
            path: root.join("b-side.wav"),
        }))
    );
}

#[test]
fn a_folder_of_the_tree_opens_and_closes() {
    let root = library("files-tree");
    let mut browser = browser(&root);
    let toggle = AppCommand::BrowserToggle { path: root.clone() };
    browser.apply(&toggle);
    wait(&mut browser, |browser| browser.view().tree.len() == 2);
    let view = browser.view();
    assert_eq!(view.tree.len(), 2);
    assert_eq!(view.tree[1].name, "sets");
    // Opening a folder of the tree does not change what the list shows.
    assert!(view.rows.is_empty() && browser.current().is_none());
    browser.apply(&toggle);
    assert_eq!(browser.view().tree.len(), 1);
}

#[test]
fn a_folder_that_does_not_exist_is_empty() {
    let root = library("files-missing");
    let mut browser = browser(&root);
    browser.navigate(root.join("gone"));
    wait(&mut browser, |browser| !browser.view().reading);
    assert!(browser.view().rows.is_empty());
    assert_eq!(browser.view().selected, None);
}

#[test]
fn a_renamed_root_shows_its_new_name_in_the_tree_and_the_crumbs() {
    let root = library("renamed-root");
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    wait(&mut browser, all_probed);
    browser.rename_root(&root, "Biblioteca");
    let view = browser.view();
    assert_eq!(view.tree[0].name, "Biblioteca");
    assert_eq!(view.crumbs, ["Biblioteca"]);
}

/// The selected rows and the current one. Behaviour note: `docs/behavior/browser.md`,
/// *Selecting several tracks*.
fn selection(browser: &FileBrowser) -> (Vec<usize>, Option<usize>) {
    let view = browser.view();
    (view.marked, view.selected)
}

#[test]
fn several_rows_are_selected_with_control_shift_and_select_all() {
    let root = library("files-select");
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    wait(&mut browser, all_probed);
    // A folder, two tracks and a damaged file.
    assert_eq!(browser.view().rows.len(), 4);

    browser.apply(&AppCommand::BrowserSelect { index: 1 });
    browser.apply(&AppCommand::BrowserSelectToggle { index: 3 });
    assert_eq!(selection(&browser), (vec![1, 3], Some(3)));
    // Taking the current row out leaves another selected row current.
    browser.apply(&AppCommand::BrowserSelectToggle { index: 3 });
    assert_eq!(selection(&browser), (vec![1], Some(1)));
    // The last selected row stays.
    browser.apply(&AppCommand::BrowserSelectToggle { index: 1 });
    assert_eq!(selection(&browser), (vec![1], Some(1)));

    browser.apply(&AppCommand::BrowserSelectExtend { index: 3 });
    assert_eq!(selection(&browser), (vec![1, 2, 3], Some(1)));
    browser.apply(&AppCommand::BrowserSelectExtend { index: 0 });
    assert_eq!(selection(&browser), (vec![0, 1], Some(1)));
    // A row past the end changes nothing.
    browser.apply(&AppCommand::BrowserSelectExtend { index: 9 });
    browser.apply(&AppCommand::BrowserSelectToggle { index: 9 });
    assert_eq!(selection(&browser), (vec![0, 1], Some(1)));

    browser.apply(&AppCommand::BrowserSelectAll);
    assert_eq!(selection(&browser), (vec![0, 1, 2, 3], Some(1)));
    // An arrow leaves the current row alone, and so does a plain click.
    browser.apply(&AppCommand::BrowserMove { delta: 1 });
    assert_eq!(selection(&browser), (vec![2], Some(2)));
    browser.apply(&AppCommand::BrowserSelectAll);
    browser.apply(&AppCommand::BrowserSelect { index: 2 });
    assert_eq!(selection(&browser), (vec![2], Some(2)));
}

#[test]
fn a_change_of_the_list_ends_a_selection() {
    let root = library("files-select-change");
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    wait(&mut browser, all_probed);
    browser.apply(&AppCommand::BrowserSelectAll);
    browser.apply(&AppCommand::BrowserSearchSet {
        text: "side".to_owned(),
    });
    assert_eq!(selection(&browser), (vec![0], Some(0)));

    browser.apply(&AppCommand::BrowserSearchSet {
        text: String::new(),
    });
    browser.apply(&AppCommand::BrowserSelectAll);
    browser.navigate(root.join("sets"));
    wait(&mut browser, all_probed);
    assert_eq!(selection(&browser), (vec![0], Some(0)));
}

#[test]
fn an_empty_list_has_no_selection_to_extend() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("files-select-empty");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let mut browser = browser(&root);
    browser.navigate(root.clone());
    wait(&mut browser, all_probed);
    browser.apply(&AppCommand::BrowserSelectAll);
    browser.apply(&AppCommand::BrowserSelectExtend { index: 0 });
    assert_eq!(selection(&browser), (Vec::new(), None));
}
