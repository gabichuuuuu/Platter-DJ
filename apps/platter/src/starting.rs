//! What the library does when Platter starts: the import of the music folders, one after the
//! other, then the check of the collection. Behaviour note: `docs/behavior/settings.md`,
//! Library.

use std::collections::VecDeque;
use std::path::PathBuf;

use platter_ui_egui::command::{AppCommand, SettingsPage};

/// The work at start still to do.
#[derive(Debug)]
pub struct Starting {
    folders: VecDeque<PathBuf>,
    check: bool,
}

/// What the window does in a frame about the start.
#[derive(Debug, PartialEq)]
pub enum Next {
    /// The library has not answered yet, or is busy: ask again in the next frame.
    Wait,
    /// Send these commands, then ask again.
    Send(Vec<AppCommand>),
    /// Nothing is left to do.
    Done,
}

impl Starting {
    /// The folders are imported when `import` says so, the collection checked when `check`
    /// does.
    pub fn new(folders: &[PathBuf], import: bool, check: bool) -> Self {
        Self {
            folders: if import {
                folders.iter().cloned().collect()
            } else {
                VecDeque::new()
            },
            check,
        }
    }

    /// The next step, given whether the library opened (`None` while it has not answered)
    /// and whether it is still working on something.
    pub fn next(&mut self, available: Option<bool>, busy: bool) -> Next {
        match available {
            Some(false) => return Next::Done,
            None => return Next::Wait,
            Some(true) if busy => return Next::Wait,
            Some(true) => {}
        }
        if let Some(path) = self.folders.pop_front() {
            return Next::Send(vec![AppCommand::LibraryImportFolder { path }]);
        }
        if std::mem::take(&mut self.check) {
            // The maintenance opens over the Library page of the preferences, as when the user
            // opens it.
            return Next::Send(vec![
                AppCommand::SettingsOpen {
                    page: SettingsPage::Library,
                },
                AppCommand::LibraryMaintenanceOpen,
                AppCommand::LibraryCheck,
            ]);
        }
        Next::Done
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use platter_ui_egui::command::{AppCommand, SettingsPage};

    use super::{Next, Starting};

    fn import(path: &str) -> Next {
        Next::Send(vec![AppCommand::LibraryImportFolder {
            path: PathBuf::from(path),
        }])
    }

    #[test]
    fn the_folders_are_imported_in_their_order_each_when_the_library_is_free_then_checked() {
        let folders = [PathBuf::from("first"), PathBuf::from("second")];
        let mut starting = Starting::new(&folders, true, true);
        // The library has not answered yet.
        assert_eq!(starting.next(None, true), Next::Wait);
        assert_eq!(starting.next(Some(true), false), import("first"));
        // The import runs.
        assert_eq!(starting.next(Some(true), true), Next::Wait);
        // A folder that is gone ends its import at once with an error: the next one follows.
        assert_eq!(starting.next(Some(true), false), import("second"));
        assert_eq!(starting.next(Some(true), true), Next::Wait);
        assert_eq!(
            starting.next(Some(true), false),
            Next::Send(vec![
                AppCommand::SettingsOpen {
                    page: SettingsPage::Library
                },
                AppCommand::LibraryMaintenanceOpen,
                AppCommand::LibraryCheck,
            ])
        );
        assert_eq!(starting.next(Some(true), false), Next::Done);
    }

    #[test]
    fn nothing_happens_when_both_are_off_or_the_library_is_unavailable() {
        let folders = [PathBuf::from("first")];
        assert_eq!(
            Starting::new(&folders, false, false).next(Some(true), false),
            Next::Done
        );
        let mut checking = Starting::new(&folders, false, true);
        assert!(matches!(checking.next(Some(true), false), Next::Send(_)));
        let mut starting = Starting::new(&folders, true, true);
        assert_eq!(starting.next(Some(false), false), Next::Done);
    }
}
