//! The stored cues of a track: eight slots, the hotcues, and the cue that is selected.
//!
//! Positions and lengths are seconds of the track. Behaviour notes: `docs/behavior/cues.md`.

use crate::error::ValueError;
use crate::loops::LoopRegion;

/// Distance within which a position counts as standing on a cue, in seconds.
const ON_A_CUE_SECONDS: f64 = 0.001;

/// One of the eight slots of a deck.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct HotcueSlot(u8);

impl HotcueSlot {
    pub const COUNT: usize = 8;
    pub const ALL: [Self; Self::COUNT] = [
        Self(0),
        Self(1),
        Self(2),
        Self(3),
        Self(4),
        Self(5),
        Self(6),
        Self(7),
    ];

    /// The slot with the number a user sees, 1 to 8.
    pub fn from_number(number: f64) -> Result<Self, ValueError> {
        let max = Self::COUNT as f64;
        let number = ValueError::check("slot", number, 1.0, max)?;
        Self::ALL
            .into_iter()
            .find(|slot| f64::from(slot.number()) == number)
            .ok_or(ValueError::OutOfRange {
                name: "slot",
                min: 1.0,
                max,
            })
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }

    /// The number a user sees, 1 to 8.
    pub fn number(self) -> u8 {
        self.0 + 1
    }
}

/// What a cue is for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CueKind {
    #[default]
    Cue,
    FadeIn,
    FadeOut,
    Load,
    Loop,
}

impl CueKind {
    pub const ALL: [Self; 5] = [
        Self::Cue,
        Self::FadeIn,
        Self::FadeOut,
        Self::Load,
        Self::Loop,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Cue => "cue",
            Self::FadeIn => "fade_in",
            Self::FadeOut => "fade_out",
            Self::Load => "load",
            Self::Loop => "loop",
        }
    }
}

/// The name of a cue: text of a bounded length kept in place, so the audio thread can hold
/// and copy it without touching the heap.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CueName {
    bytes: [u8; Self::MAX_BYTES],
    len: u8,
}

impl CueName {
    pub const MAX_BYTES: usize = 48;

    /// The name `text` gives: without control characters, and cut at the last whole
    /// character that fits.
    pub fn new(text: &str) -> Self {
        let mut name = Self::default();
        let mut len = 0;
        for character in text.chars().filter(|character| !character.is_control()) {
            let mut encoded = [0; 4];
            let encoded = character.encode_utf8(&mut encoded).as_bytes();
            let Some(place) = name.bytes.get_mut(len..len + encoded.len()) else {
                break;
            };
            place.copy_from_slice(encoded);
            len += encoded.len();
        }
        // `len` is at most `MAX_BYTES`, which fits.
        name.len = len as u8;
        name
    }

    pub fn as_str(&self) -> &str {
        self.bytes
            .get(..usize::from(self.len))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Default for CueName {
    fn default() -> Self {
        Self {
            bytes: [0; Self::MAX_BYTES],
            len: 0,
        }
    }
}

impl std::fmt::Debug for CueName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.as_str().fmt(f)
    }
}

/// A stored cue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cue {
    pub position: f64,
    kind: CueKind,
    /// Length of the loop of a cue of the kind [`CueKind::Loop`]; zero for the others.
    length: f64,
    pub name: CueName,
}

impl Cue {
    /// A cue of the kind [`CueKind::Cue`] at `position`.
    pub fn at(position: f64) -> Self {
        Self {
            position,
            kind: CueKind::Cue,
            length: 0.0,
            name: CueName::default(),
        }
    }

    /// A cue that stores a loop.
    pub fn looping(region: LoopRegion) -> Self {
        Self {
            kind: CueKind::Loop,
            length: region.seconds(),
            ..Self::at(region.start)
        }
    }

    pub fn kind(&self) -> CueKind {
        self.kind
    }

    /// The loop a cue of the kind [`CueKind::Loop`] stores.
    pub fn region(&self) -> Option<LoopRegion> {
        (self.kind == CueKind::Loop).then_some(LoopRegion {
            start: self.position,
            end: self.position + self.length,
        })
    }
}

/// The cues of a track by slot, and the one that is selected.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cues {
    slots: [Option<Cue>; HotcueSlot::COUNT],
    selected: Option<HotcueSlot>,
}

impl Cues {
    pub fn get(&self, slot: HotcueSlot) -> Option<&Cue> {
        self.slots.get(slot.index()).and_then(Option::as_ref)
    }

    /// The slot of the selected cue.
    pub fn selected(&self) -> Option<HotcueSlot> {
        self.selected
    }

    pub fn selected_cue(&self) -> Option<&Cue> {
        self.get(self.selected?)
    }

    /// The stored cues with their slots, in the order of the slots.
    pub fn iter(&self) -> impl Iterator<Item = (HotcueSlot, &Cue)> {
        HotcueSlot::ALL
            .into_iter()
            .filter_map(|slot| Some((slot, self.get(slot)?)))
    }

    pub fn first_free(&self) -> Option<HotcueSlot> {
        HotcueSlot::ALL
            .into_iter()
            .find(|slot| self.get(*slot).is_none())
    }

    /// Stores `cue` in an empty slot and selects it. A taken slot keeps its cue.
    pub fn store(&mut self, slot: HotcueSlot, cue: Cue) -> bool {
        match self.slots.get_mut(slot.index()) {
            Some(place @ None) => {
                *place = Some(cue);
                self.selected = Some(slot);
                true
            }
            _ => false,
        }
    }

    /// Selects the cue of a slot that holds one.
    pub fn select(&mut self, slot: HotcueSlot) -> bool {
        let changed = self.get(slot).is_some() && self.selected != Some(slot);
        if changed {
            self.selected = Some(slot);
        }
        changed
    }

    /// Empties a slot. The selection goes with its cue.
    pub fn delete(&mut self, slot: HotcueSlot) -> bool {
        let Some(place @ Some(_)) = self.slots.get_mut(slot.index()) else {
            return false;
        };
        *place = None;
        if self.selected == Some(slot) {
            self.selected = None;
        }
        true
    }

    /// Moves the selected cue to `slot`; a cue that was there takes the slot it leaves.
    pub fn map_selected(&mut self, slot: HotcueSlot) -> bool {
        match self.selected {
            Some(from) if from != slot => {
                self.slots.swap(from.index(), slot.index());
                self.selected = Some(slot);
                true
            }
            _ => false,
        }
    }

    pub fn rename_selected(&mut self, name: CueName) -> bool {
        self.edit_selected(|cue| cue.name = name)
    }

    /// Gives the selected cue another kind. `length` is the loop it gets when it becomes
    /// one; without it the cue does not become a loop.
    pub fn retype_selected(&mut self, kind: CueKind, length: Option<f64>) -> bool {
        let length = match (kind, length) {
            (CueKind::Loop, Some(length)) if length.is_finite() && length > 0.0 => length,
            (CueKind::Loop, _) => return false,
            _ => 0.0,
        };
        self.edit_selected(|cue| {
            if cue.kind != kind {
                cue.kind = kind;
                cue.length = length;
            }
        })
    }

    fn edit_selected(&mut self, edit: impl FnOnce(&mut Cue)) -> bool {
        let Some(cue) = self
            .selected
            .and_then(|slot| self.slots.get_mut(slot.index()))
            .and_then(Option::as_mut)
        else {
            return false;
        };
        let before = *cue;
        edit(cue);
        *cue != before
    }

    /// The slot of the nearest cue before `at`, skipping one `at` stands on.
    pub fn previous(&self, at: f64) -> Option<HotcueSlot> {
        self.iter()
            .filter(|(_, cue)| cue.position < at - ON_A_CUE_SECONDS)
            .max_by(|(_, left), (_, right)| left.position.total_cmp(&right.position))
            .map(|(slot, _)| slot)
    }

    /// The slot of the nearest cue after `at`, skipping one `at` stands on.
    pub fn next(&self, at: f64) -> Option<HotcueSlot> {
        self.iter()
            .filter(|(_, cue)| cue.position > at + ON_A_CUE_SECONDS)
            .min_by(|(_, left), (_, right)| left.position.total_cmp(&right.position))
            .map(|(slot, _)| slot)
    }
}

#[cfg(test)]
mod tests {
    use super::{Cue, CueKind, CueName, Cues, HotcueSlot};
    use crate::loops::LoopRegion;

    fn slot(number: u8) -> HotcueSlot {
        HotcueSlot::from_number(f64::from(number)).unwrap()
    }

    #[test]
    fn a_slot_takes_the_whole_numbers_from_one_to_eight() {
        assert_eq!(slot(1).index(), 0);
        assert_eq!(slot(8).number(), 8);
        for bad in [0.0, 9.0, 1.5, -1.0, f64::NAN, f64::INFINITY] {
            assert!(HotcueSlot::from_number(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_name_is_cut_at_a_whole_character_and_loses_control_characters() {
        assert_eq!(CueName::new("Drop\n\tin").as_str(), "Dropin");
        assert!(CueName::new("").is_empty());
        // Sixteen characters of three bytes fill the name; the seventeenth does not fit.
        let long = "€".repeat(20);
        assert_eq!(CueName::new(&long).as_str(), "€".repeat(16));
        // Forty-seven bytes and then a character of two: it is left out whole.
        let odd = format!("{}é", "a".repeat(47));
        assert_eq!(CueName::new(&odd).as_str(), "a".repeat(47));
    }

    #[test]
    fn a_stored_cue_is_selected_and_a_taken_slot_keeps_its_cue() {
        let mut cues = Cues::default();
        assert!(cues.store(slot(3), Cue::at(1.0)));
        assert_eq!(cues.selected(), Some(slot(3)));
        assert!(!cues.store(slot(3), Cue::at(2.0)));
        assert_eq!(cues.get(slot(3)).unwrap().position, 1.0);
        assert_eq!(cues.first_free(), Some(slot(1)));
        for free in HotcueSlot::ALL {
            cues.store(free, Cue::at(0.0));
        }
        assert_eq!(cues.first_free(), None);
    }

    #[test]
    fn deleting_the_selected_cue_leaves_none_selected() {
        let mut cues = Cues::default();
        cues.store(slot(1), Cue::at(1.0));
        cues.store(slot(2), Cue::at(2.0));
        assert!(cues.delete(slot(1)));
        assert_eq!(cues.selected(), Some(slot(2)));
        assert!(cues.delete(slot(2)));
        assert_eq!(cues.selected(), None);
        assert!(!cues.delete(slot(2)));
        assert!(!cues.select(slot(2)));
    }

    #[test]
    fn mapping_moves_the_selected_cue_and_swaps_with_a_taken_slot() {
        let mut cues = Cues::default();
        cues.store(slot(1), Cue::at(1.0));
        cues.store(slot(2), Cue::at(2.0));
        assert!(cues.map_selected(slot(5)));
        assert_eq!(cues.get(slot(2)), None);
        assert_eq!(cues.selected_cue().unwrap().position, 2.0);
        assert!(cues.map_selected(slot(1)));
        assert_eq!(cues.get(slot(1)).unwrap().position, 2.0);
        assert_eq!(cues.get(slot(5)).unwrap().position, 1.0);
        assert_eq!(cues.selected(), Some(slot(1)));
        assert!(!cues.map_selected(slot(1)));
    }

    #[test]
    fn a_cue_becomes_a_loop_only_with_a_length_and_loses_it_with_another_kind() {
        let mut cues = Cues::default();
        assert!(!cues.retype_selected(CueKind::Load, None));
        cues.store(slot(1), Cue::at(1.0));
        assert!(!cues.retype_selected(CueKind::Loop, None));
        assert!(!cues.retype_selected(CueKind::Loop, Some(f64::NAN)));
        assert!(cues.retype_selected(CueKind::Loop, Some(2.0)));
        let region = cues.selected_cue().unwrap().region().unwrap();
        assert_eq!((region.start, region.end), (1.0, 3.0));
        // A loop keeps the length it has.
        assert!(!cues.retype_selected(CueKind::Loop, Some(4.0)));
        assert!(cues.retype_selected(CueKind::FadeOut, None));
        assert_eq!(cues.selected_cue().unwrap().region(), None);
        assert_eq!(cues.selected_cue().unwrap().kind(), CueKind::FadeOut);
    }

    #[test]
    fn a_stored_loop_starts_at_its_cue() {
        let cue = Cue::looping(LoopRegion {
            start: 4.0,
            end: 6.0,
        });
        assert_eq!(cue.position, 4.0);
        assert_eq!(cue.kind(), CueKind::Loop);
        assert_eq!(cue.region().unwrap().seconds(), 2.0);
    }

    #[test]
    fn previous_and_next_follow_the_positions_and_skip_the_cue_under_the_deck() {
        let mut cues = Cues::default();
        cues.store(slot(1), Cue::at(30.0));
        cues.store(slot(2), Cue::at(10.0));
        cues.store(slot(3), Cue::at(20.0));
        assert_eq!(cues.next(0.0), Some(slot(2)));
        assert_eq!(cues.next(10.0005), Some(slot(3)));
        assert_eq!(cues.next(30.0), None);
        assert_eq!(cues.previous(25.0), Some(slot(3)));
        assert_eq!(cues.previous(20.0005), Some(slot(2)));
        assert_eq!(cues.previous(10.0), None);
    }

    #[test]
    fn renaming_needs_a_selected_cue() {
        let mut cues = Cues::default();
        assert!(!cues.rename_selected(CueName::new("Intro")));
        cues.store(slot(4), Cue::at(1.0));
        assert!(cues.rename_selected(CueName::new("Intro")));
        assert!(!cues.rename_selected(CueName::new("Intro")));
        assert_eq!(cues.selected_cue().unwrap().name.as_str(), "Intro");
    }
}
