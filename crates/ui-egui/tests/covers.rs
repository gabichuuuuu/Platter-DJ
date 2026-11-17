//! The covers of the tracks of the list, read from files the test writes.
//!
//! Behaviour note: `docs/behavior/cover-art.md`, *In the list of tracks* and *Importing and
//! deleting a cover*.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use platter_library::{Library, SqliteLibrary};
use platter_testkit::files::{flac_pcm16, wav_pcm16};
use platter_testkit::pictures::{FRONT_COVER, flac_with_pictures, rgba_png};
use platter_ui_egui::collection::{Collection, TagWriting};
use platter_ui_egui::command::{AppCommand, BrowserNode};
use platter_ui_egui::covers::Covers;
use platter_ui_egui::files::{FileBrowser, MAX_PICTURES, Root};
use platter_ui_egui::model::{CoverImage, ImportOutcome};

/// A folder with a FLAC file whose front cover is a red square, a WAV file without a cover
/// and a FLAC file without one.
fn folder(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("covers-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let red = rgba_png(4, 4, |_, _| [255, 0, 0, 255]);
    std::fs::write(
        root.join("covered.flac"),
        flac_with_pictures(&[(FRONT_COVER, &red)]),
    )
    .unwrap();
    std::fs::write(root.join("plain.wav"), wav_pcm16(44_100, 2, &[0; 2_000])).unwrap();
    std::fs::write(root.join("bare.flac"), flac_with_pictures(&[])).unwrap();
    root
}

fn until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "the covers were never read");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_covers_are_read_on_their_thread_and_a_file_without_one_has_none() {
    let root = folder("loader");
    let mut covers = Covers::start();
    let paths = [
        root.join("covered.flac"),
        root.join("plain.wav"),
        root.join("bare.flac"),
        root.join("gone.flac"),
    ];
    covers.want(paths.iter().map(PathBuf::as_path));
    // Asking again reads nothing again.
    covers.want(paths.iter().map(PathBuf::as_path));
    until(|| {
        covers.poll();
        covers.idle()
    });
    let cover = covers.get(&paths[0]).unwrap();
    assert_eq!((cover.width, cover.height), (4, 4));
    assert_eq!(&cover.rgba[..4], [255, 0, 0, 255]);
    for path in &paths[1..] {
        assert!(covers.get(path).is_none(), "{}", path.display());
    }
}

#[test]
fn the_list_of_a_folder_carries_the_covers_of_its_tracks() {
    let root = folder("list");
    let mut browser = FileBrowser::new(vec![Root {
        name: "Music".to_owned(),
        path: root.clone(),
    }]);
    browser.navigate(root.clone());
    let covered = |browser: &FileBrowser, name: &str| {
        browser
            .view()
            .covers
            .keys()
            .any(|path| path.file_name() == Path::new(name).file_name())
    };
    until(|| {
        browser.poll();
        covered(&browser, "covered.flac")
    });
    let view = browser.view();
    assert_eq!(view.covers.len(), 1);
    assert!(!covered(&browser, "plain.wav"));
}

#[test]
fn the_cover_of_a_track_a_deck_holds_reaches_its_header() {
    let root = folder("deck");
    let mut browser = FileBrowser::new(vec![Root {
        name: "Music".to_owned(),
        path: root.clone(),
    }]);
    // Not listed: the deck holds it all the same.
    browser.set_deck_tracks([None, Some(root.join("covered.flac")), None, None]);
    until(|| {
        browser.poll();
        browser.view().deck_covers[1].is_some()
    });
    let view = browser.view();
    assert!(view.deck_covers[0].is_none());
    browser.set_deck_tracks([Some(root.join("plain.wav")), None, None, None]);
    browser.poll();
    assert!(browser.view().deck_covers[1].is_none());
}

#[test]
fn the_pictures_of_the_folder_of_the_explorer_are_offered_by_name() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("covers-pictures");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("inner.png")).unwrap();
    // The listing reads names only: the contents do not matter here.
    for index in 0..MAX_PICTURES {
        std::fs::write(root.join(format!("p{index:02}.png")), b"").unwrap();
    }
    for name in ["A.JPEG", "b.Jpg", "c.gif", "notes.txt", ".hidden.png"] {
        std::fs::write(root.join(name), b"").unwrap();
    }
    let mut browser = FileBrowser::new(vec![Root {
        name: "Pictures".to_owned(),
        path: root.clone(),
    }]);
    browser.navigate(root.clone());
    until(|| {
        browser.poll();
        !browser.view().reading
    });
    let names: Vec<String> = browser
        .view()
        .pictures
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), MAX_PICTURES);
    // By name without regard to case; a folder, another format and a hidden file are not.
    assert_eq!(names[..3], ["A.JPEG", "b.Jpg", "p00.png"]);
    let last = format!("p{:02}.png", MAX_PICTURES - 3);
    assert_eq!(names.last(), Some(&last));
}

/// Whether `cover` is a square of four pixels a side of the colour `rgba`.
fn coloured(cover: Option<&CoverImage>, rgba: [u8; 4]) -> bool {
    cover.is_some_and(|cover| (cover.width, cover.height) == (4, 4) && cover.rgba[..4] == rgba)
}

#[test]
fn an_imported_cover_reaches_the_rows_and_the_deck_and_a_deleted_one_leaves_them() {
    let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("covers-import");
    let _ = std::fs::remove_dir_all(&root);
    let music = root.join("music");
    std::fs::create_dir_all(&music).unwrap();
    for name in ["one.flac", "three.flac", "two.flac"] {
        std::fs::write(music.join(name), flac_pcm16(44_100, 2, &[0; 2_000])).unwrap();
    }
    let green = [0, 255, 0, 255];
    let blue = [0, 0, 255, 255];
    std::fs::write(music.join("Green.PNG"), rgba_png(4, 4, |_, _| green)).unwrap();
    std::fs::write(music.join("blue.png"), rgba_png(4, 4, |_, _| blue)).unwrap();
    std::fs::write(music.join("broken.png"), b"not a picture").unwrap();
    let library = root.join("library.sqlite");
    let mut browser = FileBrowser::new(vec![Root {
        name: "Music".to_owned(),
        path: music.clone(),
    }]);
    browser.attach(Collection::start(move || {
        Ok(Box::new(SqliteLibrary::open(&library)?) as Box<dyn Library>)
    }));
    browser.navigate(music.clone());
    // The library opens first; then the folder goes into it.
    until(|| {
        browser.poll();
        !browser.view().reading && !browser.collection_busy()
    });
    browser.apply(&AppCommand::LibraryImportFolder {
        path: music.clone(),
    });
    until(|| {
        browser.poll();
        !browser.collection_busy()
    });
    assert!(matches!(
        browser.view().collection.unwrap().imported,
        Some(ImportOutcome::Done { added: 3, .. })
    ));
    browser.apply(&AppCommand::BrowserNodeSelect {
        node: BrowserNode::Collection,
    });
    until(|| {
        browser.poll();
        !browser.collection_busy() && browser.view().rows.len() == 3
    });
    // The explorer still shows the folder: its pictures stay offered.
    let pictures = browser.view().pictures;
    let names: Vec<_> = pictures
        .iter()
        .map(|path| path.file_name().unwrap())
        .collect();
    assert_eq!(names, ["blue.png", "broken.png", "Green.PNG"]);
    let rows: Vec<PathBuf> = browser
        .view()
        .rows
        .iter()
        .map(|row| row.path.clone())
        .collect();
    // Two tracks selected; deck B holds the first.
    browser.apply(&AppCommand::BrowserSelect { index: 0 });
    browser.apply(&AppCommand::BrowserSelectToggle { index: 1 });
    browser.set_deck_tracks([None, Some(rows[0].clone()), None, None]);

    browser.apply(&AppCommand::BrowserTrackCoverImport {
        path: pictures[2].clone(),
    });
    until(|| {
        browser.poll();
        let view = browser.view();
        coloured(view.covers.get(&rows[0]), green)
            && coloured(view.covers.get(&rows[1]), green)
            && coloured(view.deck_covers[1].as_ref(), green)
    });
    assert!(browser.take_unwritten().is_empty());
    assert!(!browser.view().covers.contains_key(&rows[2]));
    // Another picture over the one shown replaces it.
    browser.apply(&AppCommand::BrowserTrackCoverImport {
        path: pictures[0].clone(),
    });
    until(|| {
        browser.poll();
        let view = browser.view();
        coloured(view.covers.get(&rows[0]), blue)
            && coloured(view.covers.get(&rows[1]), blue)
            && coloured(view.deck_covers[1].as_ref(), blue)
    });

    // A picture Platter cannot show is refused before any track is written.
    browser.apply(&AppCommand::BrowserTrackCoverImport {
        path: pictures[1].clone(),
    });
    until(|| {
        browser.poll();
        !browser.collection_busy()
    });
    let why = browser.take_unwritten();
    assert_eq!(why.len(), 1, "{why:?}");
    assert!(why[0].contains("broken.png"), "{why:?}");
    assert!(coloured(browser.view().covers.get(&rows[0]), blue));

    // While the setting keeps tags out of the files, a delete changes nothing.
    browser.set_tag_writing(TagWriting::PlatterOnly);
    browser.apply(&AppCommand::BrowserTrackCoverDelete);
    until(|| {
        browser.poll();
        !browser.collection_busy()
    });
    assert!(!browser.collection_busy());
    browser.set_tag_writing(TagWriting::Everything);
    browser.apply(&AppCommand::BrowserTrackCoverDelete);
    until(|| {
        browser.poll();
        let view = browser.view();
        !view.covers.contains_key(&rows[0])
            && !view.covers.contains_key(&rows[1])
            && view.deck_covers[1].is_none()
            && !browser.collection_busy()
    });
    assert!(browser.take_unwritten().is_empty());
}
