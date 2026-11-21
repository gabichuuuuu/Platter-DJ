//! The layouts of the window: what each one shows, the four built in and the personal ones.
//!
//! Behaviour note: `docs/behavior/interface.md`, *Layouts*. Design:
//! `docs/design/DESIGN.md`, *Layouts (M4)*.

use platter_core::DeckId;

use crate::command::{DeckLayout, DeckSize};

/// The most personal layouts kept.
pub const MAX_PERSONAL: usize = 16;
/// The longest name of a personal layout, in characters.
pub const MAX_NAME_CHARS: usize = 40;

/// What a layout shows. The status header and the browser are always there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub decks: DeckLayout,
    /// The size of the decks of the two-deck layout.
    pub size: DeckSize,
    pub effects: bool,
    pub mixer: bool,
    /// The parallel waveform lanes, which only the two-deck layout has.
    pub lanes: bool,
    /// Deck A alone across the window.
    pub single: bool,
}

impl Arrangement {
    /// The decks the window shows, left column first in each row.
    pub fn shown(self) -> &'static [DeckId] {
        if self.single {
            &[DeckId::A]
        } else {
            self.decks.decks()
        }
    }

    /// Whether the window draws the lanes: only with two decks.
    pub fn has_lanes(self) -> bool {
        self.lanes && self.decks == DeckLayout::Two
    }
}

impl Default for Arrangement {
    fn default() -> Self {
        BuiltIn::Mixing.arrangement()
    }
}

/// The layouts that come with Platter, first in the list, which cannot be changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltIn {
    Mixing,
    Parallel,
    Preparation,
    Browser,
}

impl BuiltIn {
    pub const ALL: [Self; 4] = [
        Self::Mixing,
        Self::Parallel,
        Self::Preparation,
        Self::Browser,
    ];

    /// The key of its name in the translations.
    pub fn text(self) -> &'static str {
        match self {
            Self::Mixing => "layouts.mixing",
            Self::Parallel => "layouts.parallel",
            Self::Preparation => "layouts.preparation",
            Self::Browser => "layouts.browser",
        }
    }

    pub fn arrangement(self) -> Arrangement {
        let all = Arrangement {
            decks: DeckLayout::Four,
            size: DeckSize::Full,
            effects: true,
            mixer: true,
            lanes: false,
            single: false,
        };
        match self {
            Self::Mixing => all,
            Self::Parallel => Arrangement {
                decks: DeckLayout::Two,
                size: DeckSize::Compact,
                lanes: true,
                ..all
            },
            Self::Preparation => Arrangement {
                decks: DeckLayout::Two,
                effects: false,
                mixer: false,
                lanes: true,
                single: true,
                ..all
            },
            Self::Browser => Arrangement {
                decks: DeckLayout::Two,
                size: DeckSize::Compact,
                effects: false,
                mixer: false,
                ..all
            },
        }
    }
}

/// A zone a personal layout shows or hides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    Effects,
    Mixer,
    Lanes,
}

impl Zone {
    pub const ALL: [Self; 3] = [Self::Effects, Self::Mixer, Self::Lanes];

    pub fn name(self) -> &'static str {
        match self {
            Self::Effects => "effects",
            Self::Mixer => "mixer",
            Self::Lanes => "lanes",
        }
    }
}

pub const ZONES: &[&str] = &["effects", "mixer", "lanes"];

/// A layout of the user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Personal {
    pub name: String,
    pub arrangement: Arrangement,
}

/// One row of the list of layouts, as the Layouts page shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    BuiltIn(BuiltIn),
    Personal(String),
}

/// The layouts of the user and the one in use: the built-in ones first, then the personal
/// ones in their order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Layouts {
    pub personal: Vec<Personal>,
    /// The position of the active layout in the whole list.
    pub active: usize,
}

impl Layouts {
    /// How many layouts the list holds.
    pub fn len(&self) -> usize {
        BuiltIn::ALL.len() + self.personal.len()
    }

    /// Never: the built-in layouts are always there.
    pub fn is_empty(&self) -> bool {
        false
    }

    pub fn rows(&self) -> Vec<Row> {
        BuiltIn::ALL
            .into_iter()
            .map(Row::BuiltIn)
            .chain(
                self.personal
                    .iter()
                    .map(|layout| Row::Personal(layout.name.clone())),
            )
            .collect()
    }

    /// The active personal layout, if a personal one is active.
    fn personal_index(&self) -> Option<usize> {
        self.active
            .checked_sub(BuiltIn::ALL.len())
            .filter(|index| *index < self.personal.len())
    }

    pub fn builtin_active(&self) -> bool {
        self.personal_index().is_none()
    }

    /// What the active layout shows; `Mixing` when the active one is gone.
    pub fn arrangement(&self) -> Arrangement {
        match self.personal_index() {
            Some(index) => self.personal[index].arrangement,
            None => BuiltIn::ALL
                .get(self.active)
                .copied()
                .unwrap_or(BuiltIn::Mixing)
                .arrangement(),
        }
    }

    /// Makes layout `index` of the list active; nothing when there is none.
    pub fn select(&mut self, index: usize) -> bool {
        if index < self.len() {
            self.active = index;
            true
        } else {
            false
        }
    }

    /// Copies the active layout last in the list, named after it with a number, and makes the
    /// copy active. `base` is the name the active layout shows.
    pub fn duplicate(&mut self, base: &str) -> bool {
        if self.personal.len() >= MAX_PERSONAL {
            return false;
        }
        let name = (2..)
            .map(|number| format!("{base} {number}"))
            .find(|name| self.personal.iter().all(|layout| &layout.name != name))
            .unwrap_or_default();
        self.personal.push(Personal {
            name: name.chars().take(MAX_NAME_CHARS).collect(),
            arrangement: self.arrangement(),
        });
        self.active = self.len() - 1;
        true
    }

    /// Renames the active personal layout; refused for a built-in one, a name of spaces or
    /// one another personal layout has.
    pub fn rename(&mut self, name: &str) -> bool {
        let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
        let Some(index) = self.personal_index() else {
            return false;
        };
        let taken = self
            .personal
            .iter()
            .enumerate()
            .any(|(other, layout)| other != index && layout.name == name);
        if name.is_empty() || taken {
            return false;
        }
        self.personal[index].name = name;
        true
    }

    /// Removes the active personal layout; `Mixing` becomes active.
    pub fn delete(&mut self) -> bool {
        let Some(index) = self.personal_index() else {
            return false;
        };
        self.personal.remove(index);
        self.active = 0;
        true
    }

    /// Moves the active personal layout one place up or down among the personal ones, and
    /// keeps it active.
    pub fn shift(&mut self, up: bool) -> bool {
        let Some(index) = self.personal_index() else {
            return false;
        };
        let Some(to) = (if up {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|to| *to < self.personal.len())
        }) else {
            return false;
        };
        self.personal.swap(index, to);
        self.active = BuiltIn::ALL.len() + to;
        true
    }

    /// Changes what the active personal layout shows; refused while a built-in one is active.
    pub fn change(&mut self, change: impl FnOnce(&mut Arrangement)) -> bool {
        let Some(index) = self.personal_index() else {
            return false;
        };
        change(&mut self.personal[index].arrangement);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{BuiltIn, Layouts, MAX_PERSONAL, Row, ZONES, Zone};
    use crate::command::{DeckLayout, DeckSize};

    /// `docs/behavior/interface.md`, *Layouts*, and its edge cases.
    #[test]
    fn personal_layouts_are_copied_renamed_moved_and_deleted() {
        let mut layouts = Layouts::default();
        assert!(layouts.builtin_active());
        assert_eq!(layouts.arrangement(), BuiltIn::Mixing.arrangement());
        // A built-in layout cannot be changed, renamed or removed.
        assert!(!layouts.change(|arrangement| arrangement.mixer = false));
        assert!(!layouts.rename("Mine"));
        assert!(!layouts.delete());

        assert!(layouts.select(1));
        assert!(layouts.duplicate("Parallel"));
        assert!(layouts.duplicate("Parallel 2"));
        assert_eq!(layouts.active, 5);
        assert_eq!(layouts.arrangement(), BuiltIn::Parallel.arrangement());
        assert!(layouts.change(|arrangement| arrangement.size = DeckSize::Full));
        assert_eq!(layouts.arrangement().size, DeckSize::Full);
        assert!(!layouts.rename("   "));
        assert!(!layouts.rename("Parallel 2"), "a name already used");
        assert!(layouts.rename(" Warm-up "));
        assert!(layouts.shift(true));
        assert_eq!(layouts.active, 4);
        assert!(!layouts.shift(true), "the first personal layout");
        assert_eq!(
            layouts.rows()[4..],
            [
                Row::Personal("Warm-up".to_owned()),
                Row::Personal("Parallel 2".to_owned())
            ]
        );
        assert!(layouts.delete());
        assert_eq!(layouts.active, 0);
        assert_eq!(layouts.len(), 5);
        assert!(!layouts.select(5));
    }

    #[test]
    fn the_list_takes_sixteen_personal_layouts_and_a_lost_one_is_mixing() {
        let mut layouts = Layouts::default();
        for _ in 0..MAX_PERSONAL {
            assert!(layouts.duplicate("Mixing"));
        }
        assert!(!layouts.duplicate("Mixing"));
        assert_eq!(layouts.personal[1].name, "Mixing 3");
        let lost = Layouts {
            active: 99,
            ..Layouts::default()
        };
        assert_eq!(lost.arrangement(), BuiltIn::Mixing.arrangement());
        let zones: Vec<_> = Zone::ALL.into_iter().map(Zone::name).collect();
        assert_eq!(zones, ZONES);
    }

    #[test]
    fn each_built_in_layout_shows_what_its_note_says() {
        let preparation = BuiltIn::Preparation.arrangement();
        assert_eq!(preparation.shown(), [platter_core::DeckId::A]);
        assert!(preparation.has_lanes() && !preparation.mixer && !preparation.effects);
        let browser = BuiltIn::Browser.arrangement();
        assert_eq!(browser.decks, DeckLayout::Two);
        assert!(!browser.has_lanes() && !browser.mixer && !browser.effects);
        assert!(!BuiltIn::Mixing.arrangement().has_lanes());
        assert!(BuiltIn::Parallel.arrangement().has_lanes());
    }
}
