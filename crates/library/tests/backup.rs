//! Backups of the library file, in folders made in a temporary directory.
//!
//! Behaviour note: `docs/behavior/library.md`, section *Backups*.

#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use platter_library::{BACKUPS_KEPT, Timestamp, back_up};

fn directory(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("backup-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn names(folder: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_copy_is_made_when_the_file_changed_and_named_by_the_time_in_utc() {
    let root = directory("changes");
    let file = root.join("library.sqlite");
    let backups = root.join("Backup");
    // No library yet: nothing to copy.
    assert_eq!(back_up(&file, &backups, Timestamp(0)).unwrap(), None);

    std::fs::write(&file, b"first").unwrap();
    let copy = back_up(&file, &backups, Timestamp(1_700_000_000)).unwrap();
    assert_eq!(
        copy.as_deref().and_then(Path::file_name).unwrap(),
        "library-20231114-221320.sqlite"
    );
    assert_eq!(std::fs::read(copy.unwrap()).unwrap(), b"first");
    // The same bytes again: no copy.
    assert_eq!(
        back_up(&file, &backups, Timestamp(1_700_000_100)).unwrap(),
        None
    );
    std::fs::write(&file, b"second").unwrap();
    assert!(
        back_up(&file, &backups, Timestamp(1_700_000_200))
            .unwrap()
            .is_some()
    );
    assert_eq!(names(&backups).len(), 2);

    // A file SQLite left in the middle of a change is not copied.
    std::fs::write(&file, b"third").unwrap();
    std::fs::write(root.join("library.sqlite-journal"), b"journal").unwrap();
    assert_eq!(
        back_up(&file, &backups, Timestamp(1_700_000_300)).unwrap(),
        None
    );
    assert_eq!(names(&backups).len(), 2);
}

#[test]
fn only_the_newest_copies_are_kept_and_other_files_are_left_alone() {
    let root = directory("kept");
    let file = root.join("library.sqlite");
    let backups = root.join("Backup");
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::write(backups.join("notes.txt"), b"mine").unwrap();
    std::fs::write(backups.join("library-old.sqlite"), b"mine too").unwrap();
    for run in 0..BACKUPS_KEPT + 5 {
        std::fs::write(&file, run.to_string()).unwrap();
        let at = Timestamp(1_700_000_000 + 60 * i64::try_from(run).unwrap());
        assert!(back_up(&file, &backups, at).unwrap().is_some());
    }
    let names = names(&backups);
    assert!(names.contains(&"notes.txt".to_owned()));
    assert!(names.contains(&"library-old.sqlite".to_owned()));
    let copies: Vec<_> = names
        .iter()
        .filter(|name| name.starts_with("library-2"))
        .collect();
    assert_eq!(copies.len(), BACKUPS_KEPT);
    // The five oldest went: the first one kept is the sixth run, five minutes later.
    assert_eq!(copies[0], "library-20231114-221820.sqlite");
}

#[test]
fn a_moment_is_split_into_its_date_and_time_in_utc() {
    assert_eq!(Timestamp(0).utc(), (1970, 1, 1, 0, 0, 0));
    assert_eq!(Timestamp(951_782_400).utc(), (2000, 2, 29, 0, 0, 0));
    assert_eq!(Timestamp(1_735_689_599).utc(), (2024, 12, 31, 23, 59, 59));
    assert_eq!(Timestamp(-1).utc(), (1969, 12, 31, 23, 59, 59));
}
