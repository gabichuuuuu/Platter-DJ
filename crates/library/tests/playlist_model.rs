//! The operations on a playlist, checked against a list in memory with random sequences.
//! Behaviour note: `docs/behavior/library.md`, section *Playlists and folders*.

// The model and its runner are not tests themselves.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::path::{Path, PathBuf};

use platter_library::{
    EntryId, Library, NodeId, NodeKind, SqliteLibrary, Timestamp, TrackId, TrackTags,
};
use proptest::prelude::*;

const TRACKS: usize = 4;

#[derive(Clone, Debug)]
enum Step {
    /// Tracks by index, and a position or the end.
    Add(Vec<usize>, Option<usize>),
    /// Entries by their place in the list.
    Remove(Vec<usize>),
    Move(usize, usize),
    RemoveRepeats,
    Clear,
    RemoveTrack(usize),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        4 => (prop::collection::vec(0..TRACKS, 1..4), prop::option::of(0usize..12))
            .prop_map(|(tracks, at)| Step::Add(tracks, at)),
        2 => prop::collection::vec(0usize..12, 1..3).prop_map(Step::Remove),
        3 => (0usize..12, 0usize..14).prop_map(|(from, to)| Step::Move(from, to)),
        1 => Just(Step::RemoveRepeats),
        1 => Just(Step::Clear),
        1 => (0..TRACKS).prop_map(Step::RemoveTrack),
    ]
}

fn directory(case: usize) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("library-model-{case}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

/// The playlist as the model has it: the track and the entry of each place.
type Model = Vec<(TrackId, EntryId)>;

fn run(case: usize, steps: &[Step]) -> Result<(), TestCaseError> {
    let mut library = SqliteLibrary::open(&directory(case).join("library.db")).unwrap();
    let mut tracks: Vec<Option<TrackId>> = (0..TRACKS)
        .map(|index| {
            let path = format!("/track/{index}");
            Some(
                library
                    .import(Path::new(&path), &TrackTags::default(), Timestamp(0))
                    .unwrap(),
            )
        })
        .collect();
    let list: NodeId = library.create(None, NodeKind::Playlist, "Model").unwrap();
    let mut model: Model = Vec::new();
    for step in steps {
        match step {
            Step::Add(indices, at) => {
                let Some(ids) = indices
                    .iter()
                    .map(|index| tracks[*index])
                    .collect::<Option<Vec<_>>>()
                else {
                    continue;
                };
                library.add(list, &ids, *at).unwrap();
                let at = at.unwrap_or(model.len()).min(model.len());
                // The new entries are the ones the model does not know yet.
                let known: Vec<EntryId> = model.iter().map(|(_, entry)| *entry).collect();
                let fresh: Vec<EntryId> = library
                    .entries(list)
                    .unwrap()
                    .iter()
                    .map(|entry| entry.id)
                    .filter(|id| !known.contains(id))
                    .collect();
                prop_assert_eq!(fresh.len(), ids.len());
                let added: Vec<_> = ids.into_iter().zip(fresh).collect();
                model.splice(at..at, added);
            }
            Step::Remove(places) => {
                let entries: Vec<EntryId> = places
                    .iter()
                    .filter_map(|place| model.get(*place).map(|(_, entry)| *entry))
                    .collect();
                library.remove_entries(list, &entries).unwrap();
                model.retain(|(_, entry)| !entries.contains(entry));
            }
            Step::Move(from, to) => {
                let Some((_, entry)) = model.get(*from).copied() else {
                    continue;
                };
                library.move_entry(list, entry, *to).unwrap();
                let moved = model.remove(*from);
                let to = (*to).min(model.len());
                model.insert(to, moved);
            }
            Step::RemoveRepeats => {
                library.remove_repeats(list).unwrap();
                let mut seen = Vec::new();
                model.retain(|(track, _)| {
                    let first = !seen.contains(track);
                    seen.push(*track);
                    first
                });
            }
            Step::Clear => {
                library.clear(list).unwrap();
                model.clear();
            }
            Step::RemoveTrack(index) => {
                let Some(track) = tracks[*index].take() else {
                    continue;
                };
                library.remove_track(track).unwrap();
                model.retain(|(kept, _)| *kept != track);
            }
        }
        let stored: Model = library
            .entries(list)
            .unwrap()
            .iter()
            .map(|entry| (entry.track, entry.id))
            .collect();
        prop_assert_eq!(&stored, &model, "after {:?}", step);
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn a_playlist_behaves_as_a_list(steps in prop::collection::vec(step(), 1..24)) {
        // Each case gets its own file; the cases of one run share the process.
        use std::sync::atomic::{AtomicUsize, Ordering};
        static CASE: AtomicUsize = AtomicUsize::new(0);
        run(CASE.fetch_add(1, Ordering::Relaxed), &steps)?;
    }
}
