//! What each deck learned of its track, handed to the collection to keep: the key and the
//! loudness once the analysis of the load has finished, the grid and the cues each time they
//! settle after a change.
//!
//! Behaviour note: `docs/behavior/library.md`, *Cues, grid and analysis*.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use platter_core::{Cues, DeckId, HotcueSlot, TrackMemory};
use platter_engine::DeckSnapshot;
use platter_ui_egui::collection::Kept;
use platter_ui_egui::keys;

/// How long the grid and the cues must stay as they are before they are kept, so that a
/// drag of the tempo writes once rather than at every frame.
pub const SETTLE: Duration = Duration::from_millis(500);

struct Deck {
    path: PathBuf,
    /// What the collection has, once the analysis of the load has finished; before that the
    /// deck may still show the grid and the cues of its previous track.
    kept: Option<TrackMemory>,
    /// What the deck shows, and since when.
    seen: TrackMemory,
    since: Instant,
}

#[derive(Default)]
pub struct Keeping {
    decks: [Option<Deck>; DeckId::COUNT],
}

impl Keeping {
    /// Looks at the decks as they are at `now` and returns what the collection should keep.
    pub fn observe(&mut self, decks: &[DeckSnapshot], now: Instant) -> Vec<(PathBuf, Kept)> {
        let mut out = Vec::new();
        for (state, deck) in self.decks.iter_mut().zip(decks) {
            let path = deck.track.as_ref().map(|track| &track.path);
            let memory = memory_of(deck);
            if state.as_ref().map(|state| &state.path) != path {
                if let Some(left) = state.take() {
                    out.extend(flush(left));
                }
                *state = path.map(|path| Deck {
                    path: path.clone(),
                    kept: None,
                    seen: memory,
                    since: now,
                });
                continue;
            }
            let Some(state) = state else {
                continue;
            };
            if memory != state.seen {
                state.seen = memory;
                state.since = now;
            }
            match (state.kept, deck.analysis) {
                (None, Some(analysis)) => {
                    let key = analysis.key.map(keys::number_of);
                    state.kept = Some(state.seen);
                    out.push((
                        state.path.clone(),
                        Kept {
                            // What came back from the collection is written again unchanged.
                            memory: (!state.seen.is_empty()).then_some(state.seen),
                            analysis: Some((key, analysis.loudness)),
                        },
                    ));
                }
                (Some(kept), _)
                    if kept != state.seen && now.duration_since(state.since) >= SETTLE =>
                {
                    state.kept = Some(state.seen);
                    out.push((
                        state.path.clone(),
                        Kept {
                            memory: Some(state.seen),
                            analysis: None,
                        },
                    ));
                }
                _ => {}
            }
        }
        out
    }

    /// Everything seen and not kept yet, as when the engine is built again or the
    /// application closes.
    pub fn flush(&mut self) -> Vec<(PathBuf, Kept)> {
        self.decks
            .iter_mut()
            .filter_map(|state| {
                let deck = state.as_mut()?;
                let (path, seen) = (deck.path.clone(), deck.seen);
                if deck.kept.is_some_and(|kept| kept != seen) {
                    deck.kept = Some(seen);
                    return Some((
                        path,
                        Kept {
                            memory: Some(seen),
                            analysis: None,
                        },
                    ));
                }
                None
            })
            .collect()
    }

    /// Whether a change waits to settle before it is kept.
    pub fn waiting(&self) -> bool {
        self.decks
            .iter()
            .flatten()
            .any(|deck| deck.kept.is_some_and(|kept| kept != deck.seen))
    }
}

fn flush(deck: Deck) -> Option<(PathBuf, Kept)> {
    deck.kept.filter(|kept| *kept != deck.seen).map(|_| {
        (
            deck.path,
            Kept {
                memory: Some(deck.seen),
                analysis: None,
            },
        )
    })
}

/// The grid and the cues of a deck, without the selected cue, which is not kept.
fn memory_of(deck: &DeckSnapshot) -> TrackMemory {
    let mut slots = [None; HotcueSlot::COUNT];
    for (slot, cue) in deck.cues.iter() {
        if let Some(place) = slots.get_mut(slot.index()) {
            *place = Some(*cue);
        }
    }
    TrackMemory {
        grid: deck.grid,
        cues: Cues::from_slots(slots),
    }
}

#[cfg(test)]
mod tests {
    use platter_core::{Bpm, Cue};
    use platter_engine::{Key, Mode, TrackAnalysis};
    use platter_ui_egui::sample;

    use super::*;

    fn decks() -> Vec<DeckSnapshot> {
        let mut decks = sample::playing().engine.decks.to_vec();
        for deck in &mut decks {
            deck.analysis = None;
            deck.cues = Cues::default();
            deck.grid = platter_core::BeatGrid::default();
        }
        decks
    }

    fn analysed(deck: &mut DeckSnapshot) {
        deck.analysis = Some(TrackAnalysis {
            tempo: None,
            key: Some(Key {
                tonic: 9,
                mode: Mode::Minor,
            }),
            loudness: Some(-9.5),
        });
    }

    #[test]
    fn the_analysis_is_kept_once_and_a_change_once_it_settles() {
        let mut keeping = Keeping::default();
        let start = Instant::now();
        let mut decks = decks();
        assert!(keeping.observe(&decks, start).is_empty());
        // Nothing is kept while the analysis of the load runs.
        decks[0].cues.store(HotcueSlot::ALL[0], Cue::at(3.0));
        assert!(keeping.observe(&decks, start).is_empty());
        analysed(&mut decks[0]);
        let kept = keeping.observe(&decks, start);
        let path = decks[0].track.as_ref().unwrap().path.clone();
        let [(at, first)] = kept.as_slice() else {
            panic!("{kept:?}");
        };
        assert_eq!(*at, path);
        // A minor: tonic 9 plus 12.
        assert_eq!(first.analysis, Some((Some(21), Some(-9.5))));
        assert_eq!(
            first.memory.map(|memory| memory.cues.selected()),
            Some(None)
        );
        assert!(keeping.observe(&decks, start).is_empty());

        // A change is kept once it has stayed as it is for a while, and only once.
        decks[0].grid.set_bpm(Bpm::new(126.0).unwrap());
        assert!(keeping.observe(&decks, start).is_empty());
        assert!(keeping.waiting());
        assert!(keeping.observe(&decks, start + SETTLE / 2).is_empty());
        let later = start + SETTLE * 2;
        let kept = keeping.observe(&decks, later);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].1.memory.unwrap().grid, decks[0].grid);
        assert_eq!(kept[0].1.analysis, None);
        assert!(!keeping.waiting());
        assert!(keeping.observe(&decks, later + SETTLE * 2).is_empty());
        // Selecting a cue changes nothing that is kept.
        decks[0].cues.select(HotcueSlot::ALL[0]);
        assert!(keeping.observe(&decks, later + SETTLE * 4).is_empty());
    }

    #[test]
    fn a_change_not_settled_yet_is_kept_when_the_track_leaves_or_on_a_flush() {
        let mut keeping = Keeping::default();
        let now = Instant::now();
        let mut decks = decks();
        analysed(&mut decks[0]);
        analysed(&mut decks[1]);
        // The first look meets the tracks, the second takes in their analysis.
        keeping.observe(&decks, now);
        keeping.observe(&decks, now);
        decks[0].cues.store(HotcueSlot::ALL[2], Cue::at(9.0));
        decks[1].cues.store(HotcueSlot::ALL[3], Cue::at(4.0));
        keeping.observe(&decks, now);
        let first = decks[0].track.as_ref().unwrap().path.clone();
        // Another track comes into deck A: its last cues go with its own path.
        decks[0].track.as_mut().unwrap().path = PathBuf::from("another.flac");
        let kept = keeping.observe(&decks, now);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(kept[0].0, first);
        assert!(
            kept[0]
                .1
                .memory
                .unwrap()
                .cues
                .get(HotcueSlot::ALL[2])
                .is_some()
        );
        let flushed = keeping.flush();
        assert_eq!(flushed.len(), 1, "{flushed:?}");
        assert_eq!(flushed[0].0, decks[1].track.as_ref().unwrap().path);
        assert!(keeping.flush().is_empty());
    }

    #[test]
    fn an_ejected_deck_keeps_nothing_more() {
        let mut keeping = Keeping::default();
        let now = Instant::now();
        let mut decks = decks();
        keeping.observe(&decks, now);
        decks[0].track = None;
        assert!(keeping.observe(&decks, now).is_empty());
        assert!(!keeping.waiting());
    }
}
